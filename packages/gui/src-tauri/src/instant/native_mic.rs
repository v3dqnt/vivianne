//! Microphone capture in Rust, for web views that can't open the microphone.
//!
//! Viv Instant normally records in its own window with `getUserMedia`. On
//! macOS that only exists in WKWebView from macOS 12, so on 10.15 and 11 the
//! notch could hear nothing. There, the overlay asks for this instead: Core
//! Audio capture (through `cpal`), mixed to mono and resampled to 16 kHz —
//! exactly the frames the in-page capture produces — sent to the overlay as
//! `instant:mic` events. Everything after that (voice detection, endpointing,
//! transcription) is the same code either way (`useInstantMic.ts`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(target_os = "macos")]
use tauri::Emitter;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const TARGET_RATE: f64 = 16_000.0;
/// Frames of 512 samples (32 ms), the size the in-page capture sends.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const FRAME: usize = 512;

static RUNNING: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

/// Start capturing. The stream lives on its own thread (a `cpal` stream can't
/// move between threads on every platform) until [`cmd_instant_mic_stop`].
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn cmd_instant_mic_start(_app: tauri::AppHandle) -> Result<(), String> {
    // Windows' WebView2 records with getUserMedia; nothing calls this there.
    Err("Native microphone capture is only used on macOS.".into())
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn cmd_instant_mic_start(app: tauri::AppHandle) -> Result<(), String> {
    stop();
    let alive = Arc::new(AtomicBool::new(true));
    *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) = Some(alive.clone());
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let started = (|| -> Result<cpal::Stream, String> {
            let device = cpal::default_host().default_input_device().ok_or("No microphone found.")?;
            let config = device.default_input_config().map_err(|e| format!("Couldn't read the microphone's format: {e}"))?;
            let channels = config.channels() as usize;
            let ratio = config.sample_rate().0 as f64 / TARGET_RATE;
            let mut resampler = Resampler { ratio, pos: 0.0, prev: 0.0, out: Vec::with_capacity(FRAME * 2) };
            let emit_app = app.clone();
            let mut push = move |mono: &mut dyn Iterator<Item = f32>| {
                resampler.feed(mono);
                while resampler.out.len() >= FRAME {
                    let frame: Vec<f32> = resampler.out.drain(..FRAME).collect();
                    let _ = emit_app.emit_to(crate::instant::window::LABEL, "instant:mic", frame);
                }
            };
            let err = |e| log::warn!("[instant/mic] {e}");
            let stream = match config.sample_format() {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &config.clone().into(),
                    move |data: &[f32], _| push(&mut data.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32)),
                    err,
                    None,
                ),
                cpal::SampleFormat::I16 => device.build_input_stream(
                    &config.clone().into(),
                    move |data: &[i16], _| push(&mut data.chunks(channels).map(|c| c.iter().map(|s| *s as f32 / 32768.0).sum::<f32>() / channels as f32)),
                    err,
                    None,
                ),
                other => return Err(format!("Unsupported microphone format: {other:?}")),
            }
            .map_err(|e| {
                let msg = e.to_string();
                if msg.to_lowercase().contains("permission") {
                    "Microphone access was refused. Allow Vivianne under System Settings → Privacy & Security → Microphone.".to_string()
                } else {
                    format!("Couldn't start the microphone: {msg}")
                }
            })?;
            stream.play().map_err(|e| format!("Couldn't start the microphone: {e}"))?;
            Ok(stream)
        })();
        match started {
            Ok(stream) => {
                let _ = ready_tx.send(Ok(()));
                while alive.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                drop(stream);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        }
    });
    tokio::task::spawn_blocking(move || ready_rx.recv().unwrap_or_else(|_| Err("The microphone thread stopped.".into())))
        .await
        .map_err(|e| e.to_string())?
}

fn stop() {
    if let Some(alive) = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).take() {
        alive.store(false, Ordering::SeqCst);
    }
}

#[tauri::command]
pub async fn cmd_instant_mic_stop() -> Result<(), String> {
    stop();
    Ok(())
}

/// Linear-interpolation resampler to 16 kHz. Plenty for speech recognition:
/// the models band-limit to 8 kHz themselves.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Resampler {
    /// Input samples per output sample.
    ratio: f64,
    /// Position of the next output sample, in input samples from `prev`.
    pos: f64,
    prev: f32,
    out: Vec<f32>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Resampler {
    fn feed(&mut self, input: &mut dyn Iterator<Item = f32>) {
        for cur in input {
            // Emit every output sample that falls between `prev` and `cur`.
            while self.pos <= 1.0 {
                self.out.push(self.prev + (cur - self.prev) * self.pos as f32);
                self.pos += self.ratio;
            }
            self.pos -= 1.0;
            self.prev = cur;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Resampler;

    #[test]
    fn resamples_48k_to_16k() {
        let mut r = Resampler { ratio: 3.0, pos: 0.0, prev: 0.0, out: Vec::new() };
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.001).sin()).collect();
        r.feed(&mut input.into_iter());
        assert!((r.out.len() as i64 - 16_000).abs() <= 1, "{}", r.out.len());
    }

    #[test]
    fn passes_16k_through() {
        let mut r = Resampler { ratio: 1.0, pos: 0.0, prev: 0.0, out: Vec::new() };
        r.feed(&mut vec![0.5f32; 1000].into_iter());
        assert!((r.out.len() as i64 - 1000).abs() <= 1);
    }
}
