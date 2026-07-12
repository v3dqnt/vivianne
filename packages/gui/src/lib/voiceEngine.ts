/**
 * Voice engine — STT via native whisper.cpp (Rust backend), TTS via Kokoro
 * (in-browser ONNX). Kept out of VoiceMode.tsx so editing the component during
 * dev doesn't hot-swap the large `kokoro-js` chunk mid-load.
 *
 * STT: the WebAudio mic pipeline produces 16 kHz mono f32 PCM in the frontend;
 * we hand each VAD-segmented utterance to the Rust `cmd_whisper_transcribe`
 * command, which runs whisper.cpp. The GGML model is downloaded + cached
 * backend-side on first use.
 *
 * TTS: Kokoro-82M runs in-browser via ONNX Runtime Web; weights fetched from the
 * HF CDN on first use and cached by the browser.
 */

import { invoke } from "@tauri-apps/api/core";

/** Map a UI model id to the short whisper.cpp model id the backend understands. */
function toWhisperId(model: string): string {
  // Accept both short ids ("tiny.en") and legacy HF ids ("Xenova/whisper-tiny.en").
  const m = model.toLowerCase();
  if (m.includes("small.en")) return "small.en";
  if (m.includes("base.en")) return "base.en";
  if (m.includes("tiny.en")) return "tiny.en";
  if (m.includes("small")) return "small";
  if (m.includes("base")) return "base";
  if (m.includes("tiny")) return "tiny";
  return "tiny.en";
}

let preloadPromise: Promise<void> | null = null;

/** Preload (download + build) the whisper.cpp model so the first utterance isn't slow. */
export async function getWhisper(model: string): Promise<void> {
  const id = toWhisperId(model);
  if (!preloadPromise) {
    preloadPromise = invoke<void>("cmd_whisper_preload", { model: id }).catch((e) => {
      preloadPromise = null;
      throw e;
    });
  }
  return preloadPromise;
}

let kokoroPromise: Promise<any> | null = null;

export async function getKokoro(): Promise<any> {
  if (!kokoroPromise) {
    kokoroPromise = (async () => {
      console.log("[TTS] Importing kokoro-js...");
      const mod: any = await import("kokoro-js");
      console.log("[TTS] kokoro-js imported, loading model...");
      const tts = await mod.KokoroTTS.from_pretrained("onnx-community/Kokoro-82M-v1.0-ONNX", { dtype: "q8" });
      console.log("[TTS] Kokoro model loaded successfully");
      return tts;
    })().catch((e) => {
      console.error("[TTS] Kokoro load failed:", e);
      kokoroPromise = null;
      throw e;
    });
  }
  return kokoroPromise;
}

export async function transcribe(model: string, audio: Float32Array, _sampleRate: number): Promise<string> {
  // whisper.cpp expects 16 kHz mono f32; the mic pipeline already captures at
  // 16 kHz, so we pass the samples straight through as a plain number array
  // (Tauri IPC serializes Vec<f32> from a JS number[]).
  const id = toWhisperId(model);
  const pcm = Array.from(audio);
  const text = await invoke<string>("cmd_whisper_transcribe", { pcm, model: id, language: "en" });
  return (text ?? "").trim();
}

export async function speakKokoro(text: string, voice = "af_heart"): Promise<void> {
  const tts = await getKokoro();
  const out = await tts.generate(text, { voice });
  
  let blob: Blob;
  if (typeof out.toBlob === "function") {
    blob = out.toBlob();
  } else if (typeof out.toWav === "function") {
    const wav: Uint8Array = out.toWav();
    const buf = wav.buffer.slice(wav.byteOffset, wav.byteOffset + wav.byteLength) as ArrayBuffer;
    blob = new Blob([buf], { type: "audio/wav" });
  } else {
    throw new Error("RawAudio returned from Kokoro TTS does not have toBlob or toWav method");
  }

  const url = URL.createObjectURL(blob);
  const el = new Audio(url);
  await new Promise<void>((resolve) => {
    el.onended = () => {
      URL.revokeObjectURL(url);
      resolve();
    };
    el.onerror = () => {
      URL.revokeObjectURL(url);
      resolve();
    };
    el.play().catch(() => resolve());
  });
}

export function speakBrowser(text: string): Promise<void> {
  return new Promise((resolve) => {
    if (typeof window === "undefined" || !window.speechSynthesis) return resolve();
    const u = new SpeechSynthesisUtterance(text);
    u.onend = () => resolve();
    u.onerror = () => resolve();
    window.speechSynthesis.speak(u);
  });
}
