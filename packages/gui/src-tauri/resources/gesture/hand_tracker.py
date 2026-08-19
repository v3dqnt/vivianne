#!/usr/bin/env python3
"""Hand-tracking sidecar for Vivianne's gesture module.

Runs MediaPipe Tasks' HandLandmarker against a live camera feed and writes
one JSON object per line to stdout — that's the entire contract with the Rust
side (`gesture::sidecar`), which reads this stream, smooths it, and classifies
gestures. This script does landmark extraction ONLY: no gesture logic here,
by design (per the "MediaPipe as-is, don't reimplement hand tracking" split;
the interpretation layer lives in Rust so it can be tuned/tested without a
camera).

Requirements (not vendored — install into whatever Python this runs under):
    pip install mediapipe opencv-python

Usage:
    python hand_tracker.py --camera 0 --min-confidence 0.6 [--model PATH]

Output, one line per processed frame:
    {"timestampMs": 172..., "hands": [
        {"handedness": "Right", "confidence": 0.98,
         "landmarks": [{"x":0.5,"y":0.5,"z":0.0}, ... x21]}
    ], "error": null}

Fatal startup errors (camera unavailable, mediapipe missing, model download
failed) go to stderr with a non-zero exit — the Rust side surfaces stderr
lines as diagnostic log events. Per-frame issues that aren't fatal (a single
bad frame grab) are reported via the JSON `error` field so the stream
continues.
"""

import argparse
import json
import os
import sys
import time
import urllib.request

MODEL_URL = (
    "https://storage.googleapis.com/mediapipe-models/hand_landmarker/"
    "hand_landmarker/float16/1/hand_landmarker.task"
)
MAX_HANDS = 2


def eprint(*args):
    print(*args, file=sys.stderr, flush=True)


def resolve_model_path(override: str | None) -> str:
    if override:
        return override
    cache_dir = os.path.join(os.path.expanduser("~"), ".viv", "models", "mediapipe")
    os.makedirs(cache_dir, exist_ok=True)
    model_path = os.path.join(cache_dir, "hand_landmarker.task")
    if not os.path.exists(model_path):
        eprint(f"Downloading hand_landmarker.task to {model_path} ...")
        tmp_path = model_path + ".part"
        try:
            urllib.request.urlretrieve(MODEL_URL, tmp_path)
            os.replace(tmp_path, model_path)
        except Exception as e:  # noqa: BLE001 - genuinely any failure here is fatal for startup
            if os.path.exists(tmp_path):
                os.remove(tmp_path)
            raise RuntimeError(f"Failed to download hand landmarker model: {e}") from e
        eprint("Model downloaded.")
    return model_path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--camera", type=int, default=0)
    parser.add_argument("--min-confidence", type=float, default=0.6)
    parser.add_argument("--model", type=str, default=None, help="Override path to hand_landmarker.task")
    args = parser.parse_args()

    try:
        import cv2
        import mediapipe as mp
        from mediapipe.tasks import python as mp_python
        from mediapipe.tasks.python import vision as mp_vision
    except ImportError as e:
        eprint(f"Missing dependency: {e}. Install with: pip install mediapipe opencv-python")
        return 1

    try:
        model_path = resolve_model_path(args.model)
    except RuntimeError as e:
        eprint(str(e))
        return 1

    base_options = mp_python.BaseOptions(model_asset_path=model_path)
    options = mp_vision.HandLandmarkerOptions(
        base_options=base_options,
        running_mode=mp_vision.RunningMode.VIDEO,
        num_hands=MAX_HANDS,
        min_hand_detection_confidence=args.min_confidence,
        min_hand_presence_confidence=args.min_confidence,
        min_tracking_confidence=args.min_confidence,
    )

    try:
        landmarker = mp_vision.HandLandmarker.create_from_options(options)
    except Exception as e:  # noqa: BLE001
        eprint(f"Failed to initialize HandLandmarker: {e}")
        return 1

    cap = cv2.VideoCapture(args.camera)
    if not cap.isOpened():
        eprint(f"Failed to open camera index {args.camera}")
        return 1

    eprint(f"Hand tracker started (camera={args.camera}, model={model_path})")

    start_ms = time.monotonic() * 1000
    try:
        while True:
            ok, frame_bgr = cap.read()
            if not ok:
                print(json.dumps({"timestampMs": int(time.monotonic() * 1000), "hands": [], "error": "frame grab failed"}), flush=True)
                continue

            # Mirror horizontally — a selfie-style camera feed is what users
            # actually see themselves as, and pinch-point screen coordinates
            # need to match what's on screen, not the camera's raw orientation.
            frame_bgr = cv2.flip(frame_bgr, 1)
            frame_rgb = cv2.cvtColor(frame_bgr, cv2.COLOR_BGR2RGB)
            mp_image = mp.Image(image_format=mp.ImageFormat.SRGB, data=frame_rgb)

            timestamp_ms = int(time.monotonic() * 1000 - start_ms)
            try:
                result = landmarker.detect_for_video(mp_image, timestamp_ms)
            except Exception as e:  # noqa: BLE001 - keep the stream alive on a single bad frame
                print(json.dumps({"timestampMs": timestamp_ms, "hands": [], "error": str(e)}), flush=True)
                continue

            hands = []
            for i, hand_landmarks in enumerate(result.hand_landmarks):
                handedness = "Unknown"
                confidence = 0.0
                if i < len(result.handedness) and result.handedness[i]:
                    top = result.handedness[i][0]
                    handedness = top.category_name
                    confidence = top.score
                hands.append({
                    "handedness": handedness,
                    "confidence": confidence,
                    "landmarks": [{"x": lm.x, "y": lm.y, "z": lm.z} for lm in hand_landmarks],
                })

            print(json.dumps({"timestampMs": timestamp_ms, "hands": hands, "error": None}), flush=True)
    except KeyboardInterrupt:
        pass
    except BrokenPipeError:
        # Rust side closed/killed us — normal shutdown path, not an error.
        pass
    finally:
        cap.release()
        landmarker.close()

    return 0


if __name__ == "__main__":
    sys.exit(main())
