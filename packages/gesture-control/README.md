# gesture-control

Standalone Windows tray app: turns hand gestures (webcam + MediaPipe) into
real OS mouse/keyboard input via `enigo`, controlling whatever application
currently has focus — not just Vivianne's own webview.

It reuses the gesture pipeline from `packages/gui/src-tauri/src/gesture/`
(landmarks → smoothing → classification → `GestureEvent` stream) essentially
unchanged, and adds one new piece: `src-tauri/src/os_control.rs`, which maps
that event stream onto real `enigo` OS actions instead of DOM clicks.

## Running

From the repo root (npm workspace):

```
npm install
npm run dev -w gesture-control
```

or directly:

```
cd packages/gesture-control
npm run dev
```

This runs `tauri dev`, which builds and launches the tray app. No dev
frontend server is needed — the settings window is a single static
`src-tauri/ui/index.html`.

To build a release binary/installer:

```
npm run build -w gesture-control
```

## What it does

- A system tray icon with a right-click menu: **Start/Stop Tracking**,
  **Settings...**, **Quit**. Double-clicking the tray icon also opens
  Settings.
- The Settings window (hidden by default, opened from the tray) lets you
  start/stop tracking, tune smoothing (One-Euro min-cutoff/beta) and minimum
  detection confidence, and toggle a debug stream (landmark + gesture-event
  logging to the settings window's devtools console).
- Once started, a Python sidecar (`resources/gesture/hand_tracker.py`) opens
  the default webcam, runs MediaPipe Tasks' `HandLandmarker`, and streams one
  JSON landmark frame per line over stdout. The Rust side smooths those
  landmarks (One-Euro filter), classifies gestures, and both emits the usual
  `gesture_event` Tauri events *and* applies each event as real OS input via
  `OsController`.

### Gesture → OS action mapping

| Gesture event | OS action |
|---|---|
| `PointerMove` / pinch midpoint | Absolute mouse move (`enigo.move_mouse`), hand coords scaled to the primary display's real resolution (queried via `xcap`) |
| `PinchStart` → `PinchEnd` | Left mouse button down → up at the pinch point (so pinch-and-move drags by default) |
| `Click` | No separate action — it's the classifier's "this pinch was short enough to be a click" signal; the down/up already happened via `PinchStart`/`PinchEnd` |
| `OpenPalmHold` | Toggles a drag-arm latch: fires once to arm (next pinch holds the button down instead of releasing quickly), fires again while already dragging to release and disarm |
| `Swipe` (left/right) | `Ctrl+Win+Left`/`Ctrl+Win+Right` — Windows virtual-desktop switch. Chosen over Alt+Tab because it's a single non-modal action with no follow-up UI to release out of |
| `Adjust { delta }` (fist rotation) | Vertical scroll wheel (`enigo.scroll`), `delta` scaled ×12 into a "notches" range |
| `HandLost` | Safety: forces the mouse button up and clears the drag-arm latch, so a hand leaving frame mid-drag never leaves the button stuck down |

See doc comments at the top of `src-tauri/src/os_control.rs` for the full
rationale.

## Model download / cache

Same as the original Vivianne gesture module: `hand_tracker.py` downloads
MediaPipe's `hand_landmarker.task` model to `~/.viv/models/mediapipe/` on
first run if it isn't already there. No change to that cache path, so it's
shared with `packages/gui` if both are installed on the same machine.

Requires Python 3 on `PATH` with `mediapipe` and `opencv-python` installed:

```
pip install mediapipe opencv-python
```

## Permissions

No special entitlement is needed on Windows — `enigo` drives input through
`SendInput`, which works for any desktop app without extra permissions
(unlike macOS, where Accessibility permission is required). The one runtime
dependency is a webcam the OS will let the Python sidecar open.

## Accuracy improvements over the original `packages/gui` pipeline

These changes are local to this package only (not backported to
`packages/gui`):

1. **One-Euro filter instead of a moving average** (`src-tauri/src/gesture/smoothing.rs`).
   A fixed-window moving average trades latency for smoothness at a constant
   ratio regardless of how fast the hand is moving. One-Euro adapts that
   trade-off to signal speed: heavy smoothing while the hand is nearly still
   (kills jitter, which is what caused phantom pinch-threshold crossings),
   much lighter smoothing once the hand starts moving fast (keeps pointer
   motion and swipes responsive instead of laggy). Tunable via
   `GestureConfig.smoothing_min_cutoff` (default 1.0 Hz),
   `smoothing_beta` (default 0.007), and `smoothing_derivative_cutoff`
   (default 1.0 Hz) — exposed as sliders in the Settings window.

2. **Higher capture resolution/fps and tighter MediaPipe confidence
   thresholds** (`resources/gesture/hand_tracker.py`):
   - Requests 1280×720@30 capture instead of the camera's default (usually
     640×480) — more pixels for MediaPipe to resolve fingertip position
     from, directly reducing landmark jitter. Falls back silently to the
     camera's native mode if the driver doesn't support the request.
   - `--num-hands` now defaults to **1** (was implicitly 2 via
     `MAX_HANDS`) — single-hand tracking gets MediaPipe's full compute/
     confidence budget instead of splitting it, which is strictly better for
     a one-hand cursor-control use case.
   - `--min-confidence` default raised from 0.6 to 0.7, and tracking
     confidence is now set slightly above detection confidence
     (`min(0.95, min_confidence + 0.05)`) rather than equal to it, so a
     locked-on hand needs a bit more certainty to keep being tracked
     frame-to-frame — this reduces drift onto lower-quality tracks. This
     app drives real OS input, so trading a little detection latency for
     fewer false positives is the right call here (more disruptive to
     mis-click in a real app than in Vivianne's own DOM).
