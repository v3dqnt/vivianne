//! Sidecar process management — MediaPipe itself runs out-of-process as a
//! Python script (`hand_tracker.py`, bundled as a resource) that reads the
//! camera, runs MediaPipe Tasks' hand landmarker, and writes one JSON line
//! per frame to stdout. This module owns that child process, the reader
//! loop, and wiring frames through smoothing → classification → emitted
//! `GestureEvent`s.
//!
//! Why a line-delimited JSON stdout pipe rather than a socket: it's the
//! lowest-latency IPC that needs zero setup (no port to pick, no bind/connect
//! handshake), Python's stdout is already a pipe the OS buffers efficiently,
//! and at ~30-60 small JSON messages/sec the overhead difference against a
//! local socket is not the bottleneck — MediaPipe's own inference time is.

use std::io::ErrorKind;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex as AsyncMutex;

use super::classifier::GestureClassifier;
use super::config::GestureConfig;
use super::events::GestureEvent;
use super::landmarks::SidecarFrame;
use super::smoothing::LandmarkSmoother;
use crate::os_control::OsController;

const GESTURE_DEBUG_EVENT: &str = "gesture_debug_frame";

/// Maximum accepted size for one sidecar JSON frame line. Real frames are a
/// few hundred bytes; the cap only exists so a wedged or hostile child can't
/// grow our memory without bound by printing an endless run with no newline.
const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Debug-mode payload — smoothed landmarks plus current classifier state, for
/// the calibration overlay. Only emitted while `debug` is enabled (raw
/// landmark streams at 30-60fps aren't worth the IPC traffic otherwise).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugFrame<'a> {
    landmarks: &'a [super::landmarks::Landmark],
    tracking: bool,
    events: &'a [GestureEvent],
}

struct SidecarState {
    child: Child,
    debug: bool,
}

pub struct GestureSidecar {
    state: AsyncMutex<Option<SidecarState>>,
}

impl GestureSidecar {
    pub fn new() -> Self {
        Self { state: AsyncMutex::new(None) }
    }

    pub async fn is_running(&self) -> bool {
        self.state.lock().await.is_some()
    }

    pub async fn set_debug(&self, debug: bool) {
        if let Some(s) = self.state.lock().await.as_mut() {
            s.debug = debug;
        }
    }

    pub async fn start(
        self: &Arc<Self>,
        app: AppHandle,
        os: Arc<OsController>,
        script_path: std::path::PathBuf,
        camera_index: u32,
        config: GestureConfig,
        debug: bool,
    ) -> Result<(), String> {
        // Held across the entire start sequence so two racing starts (tray
        // toggle vs settings button) can't both pass the check and spawn two
        // camera-holding children.
        let mut guard = self.state.lock().await;
        if guard.is_some() {
            return Err("Gesture tracking is already running".to_string());
        }
        if !script_path.exists() {
            return Err(format!("hand_tracker.py not found at {}", script_path.display()));
        }

        let python = python_binary();
        let mut child = Command::new(&python)
            .arg(&script_path)
            .arg("--camera")
            .arg(camera_index.to_string())
            .arg("--min-confidence")
            .arg(config.min_detection_confidence.to_string())
            .arg("--num-hands")
            .arg("1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                format!(
                    "Failed to launch hand-tracking sidecar ({python}): {e}. \
                     Make sure Python 3 with `mediapipe` and `opencv-python` installed is on PATH."
                )
            })?;

        let stdout = child.stdout.take().ok_or("Sidecar has no stdout")?;
        let stderr = child.stderr.take().ok_or("Sidecar has no stderr")?;

        *guard = Some(SidecarState { child, debug });
        drop(guard);

        let this = self.clone();
        let app_out = app.clone();
        tokio::spawn(async move {
            this.run_reader_loop(app_out, os, stdout, config).await;
        });

        // Sidecar stderr is diagnostic (MediaPipe/OpenCV init logs, tracebacks
        // on crash) — surfaced as a distinct event so a failed camera open or
        // missing model file is visible instead of the process just dying
        // silently.
        let app_err = app.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log::warn!("[gesture-sidecar] {line}");
                let _ = app_err.emit("gesture_sidecar_log", line);
            }
        });

        Ok(())
    }

    async fn run_reader_loop(
        self: Arc<Self>,
        app: AppHandle,
        os: Arc<OsController>,
        stdout: tokio::process::ChildStdout,
        config: GestureConfig,
    ) {
        let mut smoother = LandmarkSmoother::with_params(
            config.smoothing_min_cutoff,
            config.smoothing_beta,
            config.smoothing_derivative_cutoff,
        );
        let mut classifier = GestureClassifier::new(config.clone());
        let mut reader = BufReader::new(stdout);

        loop {
            let line = match read_frame_line(&mut reader).await {
                Ok(Some(l)) => l,
                Ok(None) => break, // sidecar closed stdout — process exited
                Err(e) => {
                    log::warn!("[gesture-sidecar] stdout read error: {e}");
                    break;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let frame: SidecarFrame = match serde_json::from_str(&line) {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("[gesture-sidecar] malformed frame ({e}): {line}");
                    continue;
                }
            };
            if let Some(err) = frame.error {
                log::warn!("[gesture-sidecar] reported error: {err}");
                continue;
            }

            // Single-hand focus: the highest-confidence detected hand drives
            // gestures. Multi-hand gestures aren't in scope here — extending
            // to two-handed gestures later just means keying the
            // smoother/classifier pair by handedness instead of one shared
            // pair, the event enum doesn't need to change.
            let best = frame
                .hands
                .iter()
                .filter(|h| h.confidence >= config.min_detection_confidence)
                .max_by(|a, b| a.confidence.total_cmp(&b.confidence));

            let smoothed = smoother.push(best, config.hand_lost_frames);
            let now = Instant::now();
            let events = classifier.process(smoothed.as_deref(), now);

            // Applied directly rather than round-tripped through the Tauri
            // event bus: the bus is writable from the webview, so routing OS
            // input through it would let renderer-side code synthesize real
            // mouse input. The sidecar reader loop is the only trusted
            // producer, so it applies events itself.
            for event in &events {
                os.apply(event);
            }

            let debug = self.state.lock().await.as_ref().is_some_and(|s| s.debug);
            if debug {
                if let Some(lm) = smoothed.as_deref() {
                    let _ = app.emit(
                        GESTURE_DEBUG_EVENT,
                        DebugFrame { landmarks: lm, tracking: true, events: &events },
                    );
                } else {
                    let _ = app.emit(GESTURE_DEBUG_EVENT, DebugFrame { landmarks: &[], tracking: false, events: &events });
                }
            }

            if !self.is_running().await {
                break;
            }
        }

        // Reader loop ended (process exited or stdout closed) — clear state
        // so `start()` can be called again instead of permanently reporting
        // "already running".
        *self.state.lock().await = None;
        let _ = app.emit("gesture_sidecar_stopped", ());
    }

    pub async fn stop(&self) -> Result<(), String> {
        let mut guard = self.state.lock().await;
        let Some(mut s) = guard.take() else {
            return Ok(()); // already stopped — not an error, matches other registries' quiet-degrade convention
        };
        s.child.kill().await.map_err(|e| format!("Failed to stop sidecar: {e}"))?;
        Ok(())
    }
}

impl Default for GestureSidecar {
    fn default() -> Self {
        Self::new()
    }
}

/// Interpreter used to run the sidecar script. Resolution order:
/// 1. `GESTURE_CONTROL_PYTHON` env var — absolute path to a pinned
///    interpreter; the recommended way for deployments that don't want the
///    binary resolved from PATH at all.
/// 2. `python` (Windows) / `python3` (elsewhere) from PATH — fine for dev,
///    but a same-named executable earlier in PATH would win, which is
///    exactly why the env override above exists.
fn python_binary() -> String {
    if let Ok(pinned) = std::env::var("GESTURE_CONTROL_PYTHON") {
        if !pinned.trim().is_empty() {
            return pinned;
        }
    }
    if cfg!(windows) { "python".to_string() } else { "python3".to_string() }
}

/// Reads one `\n`-terminated line from `reader`, refusing to buffer more
/// than [`MAX_FRAME_BYTES`] — unlike `BufRead::lines()`, which happily
/// allocates for an arbitrarily long run without a newline. Returns
/// `Ok(None)` on clean EOF, or a partial final line if the stream ends
/// mid-line.
async fn read_frame_line(
    reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
) -> std::io::Result<Option<String>> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            // EOF — flush any trailing unterminated data, else signal end.
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
            };
        }
        match chunk.iter().position(|&b| b == b'\n') {
            Some(nl) => {
                bytes.extend_from_slice(&chunk[..nl]);
                reader.consume(nl + 1);
            }
            None => {
                bytes.extend_from_slice(chunk);
                let consumed = chunk.len();
                reader.consume(consumed);
            }
        }
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "sidecar frame exceeds size limit",
            ));
        }
    }
}

/// Locate `hand_tracker.py` relative to the app's resource directory (bundled
/// via `tauri.conf.json`'s `bundle.resources`); falls back to the source tree
/// path in dev so `tauri dev` works without a full bundle step.
pub fn resolve_script_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join("gesture").join("hand_tracker.py");
        if bundled.exists() {
            return Ok(bundled);
        }
    }
    let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/gesture/hand_tracker.py");
    if dev_path.exists() {
        return Ok(dev_path);
    }
    Err("Could not locate hand_tracker.py in resources (bundled or dev tree)".to_string())
}
