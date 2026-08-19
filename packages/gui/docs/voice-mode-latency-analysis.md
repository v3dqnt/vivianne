# Voice Mode — Latency Analysis & Optimization Roadmap

## Architecture Overview

The voice pipeline spans three layers:

| Layer | Files | Function |
|-------|-------|----------|
| **React UI** | `VoiceMode.tsx` | Mic capture, energy VAD, wake-word gating, state machine, TTS streaming orchestration |
| **JS bridge** | `voiceEngine.ts` | `getWhisper`/`transcribe` → Tauri IPC, `speakKokoro`/`speakBrowser` |
| **Rust backend** | `voice/mod.rs` | `cmd_whisper_preload`, `cmd_whisper_transcribe` — native whisper.cpp via `whisper-rs` bindings |

The same files exist in both `packages/gui/` and `packages/gui-remix/` and are essentially identical.

---

## End-to-End Flow (Wake-Word Path)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  PASSIVE LISTENING                                                         │
│                                                                             │
│  Mic → ScriptProcessorNode (4096 samples @ 16 kHz, ~256ms chunks)          │
│    → RMS energy gate (threshold 0.008)                                     │
│    → Buffer up to 3s of pre-roll audio                                     │
│    → Every ~1.2s while voice detected:                                      │
│        flattenBuffer → Array.from(audio) → IPC invoke                      │
│        → Rust whisper.cpp inference on pre-roll buffer                     │
│        → Check text for wake words ["vivianne", "viv", "vivian"]           │
└─────────────────────────────────────────────────────────────────────────────┘
        │
        ▼  wake word detected
┌─────────────────────────────────────────────────────────────────────────────┐
│  SESSION START (Two-Phase)                                                  │
│                                                                             │
│  1. agentReset() — reset agent thread (~100-500ms)                         │
│  2. speakGreeting() — TTS: "Hey, I'm listening." (~500-2000ms)             │
│  3. Arm command capture: activeRef = true                                  │
└─────────────────────────────────────────────────────────────────────────────┘
        │
        ▼  user speaks command
┌─────────────────────────────────────────────────────────────────────────────┐
│  COMMAND CAPTURE                                                            │
│                                                                             │
│  Buffer all audio chunks                                                   │
│  Wait for SILENCE_TAIL_MS (900ms) of silence OR MAX_UTTERANCE_MS (15s)     │
│  Finalize utterance                                                        │
└─────────────────────────────────────────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  TRANSCRIPTION                                                              │
│                                                                             │
│  flattenBuffer() → merge all chunks into one Float32Array                  │
│  Array.from(audio) → copy to number[] for IPC                              │
│  invoke("cmd_whisper_transcribe") → Tauri IPC serialization                │
│  Rust: spawn_blocking → whisper.cpp inference                              │
│  Returns transcribed text                                                   │
└─────────────────────────────────────────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  AGENT INFERENCE (Dominant Latency)                                        │
│                                                                             │
│  processCommand(text) → sendMessage(voicePrompt(text))                     │
│    → LLM inference + potential tool calls (screenshots, web search, etc.)  │
│    → Streaming reply returned via messages array                           │
└─────────────────────────────────────────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  TTS PLAYBACK                                                               │
│                                                                             │
│  useEffect monitors messages → extracts assistant text                     │
│  Sentence-boundary detection: /^[\s\S]*[.!?]/                              │
│  Per sentence: enqueueSpeech() → speakKokoro()                             │
│    → kokoro-js ONNX Runtime Web inference                                  │
│    → toBlob() → URL.createObjectURL → new Audio() → play()                │
│  Or fallback: speakBrowser() → SpeechSynthesisUtterance                    │
└─────────────────────────────────────────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  RETURN TO PASSIVE                                                          │
│                                                                             │
│  Wait until agent done AND speech finished                                 │
│  Linger 3.5s on screen                                                     │
│  Safety timeout 120s                                                        │
│  sessionInFlightRef = false → back to "listening"                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Detailed Latency Budget

### Stage A: Mic → VAD → Wake Word Detection

| Source | Est. Latency | Details |
|--------|-------------|---------|
| ScriptProcessorNode buffering | ~256ms | Fixed 4096-sample buffer @ 16 kHz |
| Wake check cadence | **up to 1,200ms** | Waits ~1.2s of sustained voice energy before running inference |
| Whisper inference (wake) | **200-800ms** | Transcribes up to 3s of pre-roll buffer; runs on `spawn_blocking` thread |
| IPC overhead (wake) | 20-100ms | `Array.from` copy + Tauri serialization |
| **Total A: wake detection** | **~1,400-2,000ms** | From saying "Vivianne" to session activation |

### Stage B: Greeting + Session Init

| Source | Est. Latency | Details |
|--------|-------------|---------|
| `agentReset()` | 100-500ms | Async agent thread reset |
| `speakGreeting()` TTS | **500-2,000ms** | Kokoro `generate()` + audio element playback |
| **Total B: greet delay** | **~600-2,500ms** | User cannot speak command until greeting finishes |

### Stage C: Command Capture

| Source | Est. Latency | Details |
|--------|-------------|---------|
| User speaks | variable | Natural speech duration |
| **SILENCE_TAIL_MS** | **900ms forced wait** | Must hear 900ms of silence before utterance is finalized |
| **Total C: VAD tail** | **~900ms dead time** | Pure latency — no work being done |

### Stage D: Transcription

| Source | Est. Latency | Details |
|--------|-------------|---------|
| `Array.from(audio)` | 10-100ms | Allocates JS Number[] from Float32Array |
| Tauri IPC serialization | 20-200ms | Serializes number[] over Tauri bridge |
| `spawn_blocking` yield | ~1ms | Trivial |
| whisper.cpp inference | **200-800ms** | tiny.en on CPU, utterance-length dependent |
| **Total D: transcribe** | **~250-1,100ms** | From speech end to transcribed text |

### Stage E: Agent / LLM Inference

| Source | Est. Latency | Details |
|--------|-------------|---------|
| `sendMessage(voicePrompt(text))` | **2,000-10,000ms** | Full agent loop via provider API |
| Tool calls (if any) | **2,000-15,000ms**+ | Screenshots, web searches, etc. |
| **Total E: agent** | **~2,000-25,000ms** | By far the dominant cost |

### Stage F: TTS Playback

| Source | Est. Latency | Details |
|--------|-------------|---------|
| Sentence boundary wait | up to ~500ms | Waits for complete sentence from stream |
| Kokoro `generate()` | **200-1,000ms** | ONNX Runtime Web inference per sentence |
| Blob URL + Audio() init | 50-200ms | `toBlob()`, `URL.createObjectURL`, `new Audio()` |
| **Total F: speak sentence** | **~250-1,700ms** | Per sentence, sequential |

### Total Perceived Latency

| Scenario | Total (wake→hear reply) |
|----------|------------------------|
| Simple command, no tools | **~6-12 seconds** |
| Command needing tool calls | **~8-25+ seconds** |
| First use (model download) | **~40-370 seconds** (Whisper + Kokoro warm-up) |

---

## Optimization Recommendations

### Tier 1: High Impact (each saves 500ms–2s+)

#### 1. Replace Full Whisper Wake-Word Detection with Keyword Spotting

**Problem**: Running whisper.cpp inference (`200-800ms`, ~40MB model loaded) every ~1.2s just to check for "vivianne" is extremely wasteful. This is the largest frontend-side latency tax.

**Solution**: Use a lightweight keyword-spotting engine that runs fully in-browser:

| Engine | Model Size | Latency | License |
|--------|-----------|---------|---------|
| **Picovoice Porcupino** | ~100KB | <50ms | Freemium |
| **Sensory TrulyHandsfree** | ~100KB | <30ms | Requires license |
| **Vosk** (small keyword model) | ~1MB | ~100ms | Apache 2.0 |

**Gain**: Wake detection drops from ~1,400ms to **~50-200ms**. Check can run on every 256ms chunk instead of every 1.2s. Near-zero CPU during passive listening.

**Changes needed**:
- `voiceEngine.ts`: Add keyword-spotter init + `detectWakeKeyword(audio: Float32Array): boolean`
- `VoiceMode.tsx`: Replace periodic `tryWakeCheck()` → full whisper with per-chunk keyword check
- Remove `flattenBuffer()` + whisper IPC calls during passive listening entirely

---

#### 2. Eliminate the Verbal Greeting

**Problem**: The greeting TTS ("Hey, I'm listening.") delays command capture by 500-2,000ms. The user just said the wake word — they don't need spoken confirmation.

**Solution**: Replace verbal TTS with a short audio chirp (pre-generated 100ms WAV) and the Persona orb waking animation.

**Gain**: Removes **500-2,000ms** from the critical path. Command capture starts ~1s sooner.

**Changes needed**:
- `VoiceMode.tsx`: `beginSession()` calls `playChirp()` (instant) instead of `speakGreeting()` (TTS)
- Add a pre-loaded short WAV asset or generate via `AudioContext.createOscillator()` at startup

---

#### 3. Stream Partial Transcriptions Instead of VAD End Detection

**Problem**: The 900ms silence tail is pure dead time. Worse, transcription only begins *after* the silence tail — another 250-1,100ms. The user waits ~1-2s after speaking before anything happens.

**Solution**: Send audio chunks to whisper **continuously** during speech (every 1-2s of audio or per-utterance). Show partial results on the notch. When silence is detected, there's zero additional transcription latency — the final text is already known.

**Gain**: Eliminates **900ms silence tail** + **250-1,100ms transcription wait** from the critical path (1.2-2s total). Those happen in parallel with speaking.

**Caveat**: Whisper accuracy may degrade on partial utterances. Mitigations:
- Use `no_context=false` (allow cross-chunk context) during command capture
- Only finalize when silence is detected → re-transcribe the full utterance once as a verification pass (can run in background while already processing the partial text)
- Consider a streaming ASR engine (e.g., Whisper `generate` in streaming mode, or Vosk) that produces time-aligned partial results natively

**Changes needed**:
- `VoiceMode.tsx`: Start transcription loop during `active` state instead of only at `finalizeUtterance`
- `voice/mod.rs`: Potentially add a `cmd_whisper_transcribe_streaming` that accepts frequent short chunks and accumulates context

---

#### 4. Start TTS from Partial Tokens (Before Sentence Boundaries)

**Problem**: The current sentence-boundary regex waits for a `.!?` before speaking. This means latency compounds: wait for the first sentence to fully stream, then generate TTS (500-1000ms), then play.

**Solution**: Start TTS generation from the *first streaming tokens* of the assistant reply. Since TTS generation takes time and the LLM is still generating, overlap the two. Use a fixed small prefix (e.g., first 30 chars) to start TTS immediately, rather than waiting for sentence boundaries.

**Gain**: **500-1,500ms** shaved off time-to-first-audio.

**Changes needed**:
- `VoiceMode.tsx`: In the TTS streaming `useEffect`, trigger TTS on any new text (not just sentence boundaries)
- Manage a queue of "already spoken" text to avoid repeating

---

#### 5. Move TTS to the Rust Backend

**Problem**: Kokoro-82M (~330MB ONNX model) runs in-browser via ONNX Runtime Web:
- Browser must fetch and cache large model weights
- ML inference competes with UI thread
- Audio output goes through blob URL → Audio element (extra round-trip)

**Solution**: Run TTS in Rust via a lightweight engine:

| Engine | Size | Quality | Notes |
|--------|------|---------|-------|
| **Piper-TTS** | ~20-100MB per voice | Good | ONNX-based, same quality as Kokoro, Rust-native |
| **espeak-ng** | ~5MB | Robotic | Ultra-low latency (~5ms), ideal for first-pass |
| **OuteTTS** | ~40MB | Good | Pure Rust TTS, newer |

**Gain**: More predictable performance, no JS main-thread contention for ML inference. Could also stream audio bytes back via a Tauri `Channel` for true streaming playback.

**Changes needed**:
- New Rust module `packages/gui/src-tauri/src/tts/` implementing Piper or similar
- `voiceEngine.ts`: Replace `speakKokoro()` with `invoke("cmd_tts_speak", { text, voice })` returning audio data
- `VoiceMode.tsx`: Stream audio via binary channel and play via `AudioContext` + `AudioBufferSourceNode` (lower latency than `new Audio(url)`)

---

### Tier 2: Medium Impact (each saves 50-500ms)

#### 6. Replace `Array.from(audio)` with Raw IPC

**Problem**: `Array.from()` on a Float32Array with 80k-160k elements allocates a new array of JS Number objects. Tauri then serializes it again. Double copy.

**Solution**: Use Tauri's binary IPC mechanism. The `invoke` command can accept `Vec<u8>` directly if we send the raw `ArrayBuffer` through Tauri's custom protocol. Or use a Tauri `Channel` with binary frames.

**Gain**: **50-200ms** saved per transcription call.

---

#### 7. Pre-Warm Kokoro with the Selected Voice

**Problem**: Even with pre-loading, the first `speakKokoro()` call may incur model warm-up time (first inference is often slower).

**Solution**: After `getKokoro()` resolves, generate a tiny silent frame with the target voice to warm up the ONNX runtime.

**Gain**: **100-300ms** saved on first TTS call.

---

#### 8. Adaptive Silence Tail

**Problem**: Fixed 900ms silence tail is a one-size-fits-all. Fast speakers wait too long; slow speakers may get cut off.

**Solution**: Adjust based on detected speech cadence:
- Track average pause length in recent speech
- Start at 900ms, decrease to 300ms if user speaks in short bursts
- Increase to 1500ms if user tends to pause mid-thought

**Gain**: **200-500ms** saved for fluent speakers.

---

#### 9. AudioWorklet Instead of ScriptProcessorNode

**Problem**: ScriptProcessorNode runs on the main thread and is deprecated. Audio processing can stall during React renders or IPC calls.

**Solution**: Implement an `AudioWorkletProcessor` in a separate file. Offloads audio processing to a dedicated audio thread.

**Gain**: **5-20ms** per chunk, but more importantly eliminates audio dropout risk during busy UI periods.

**Changes needed**:
- New file: `packages/gui/src/lib/vad-processor.ts` (AudioWorkletProcessor)
- `VoiceMode.tsx`: Replace `ScriptProcessorNode` with `AudioWorkletNode`

---

### Tier 3: Lower Impact (under 50ms or incremental)

#### 10. Cache Common TTS Phrases

Pre-generate and cache common phrases like "Thinking...", "I don't understand", error messages, and the greeting chirp during model pre-load.

#### 11. Hybrid TTS: Browser Speech for First Words, Kokoro for Remainder

Browser's `SpeechSynthesisUtterance` starts speaking in <50ms. Start with browser TTS for the first few words of the reply, then seamlessly switch to higher-quality Kokoro audio when it's ready. This eliminates the 500-1000ms TTS generation wait before the user hears anything.

#### 12. Adjustable VAD Energy Threshold

Current threshold of 0.008 RMS may be too high for quiet speech (forcing the user to speak louder) or too low for noisy environments. Make it configurable or adaptive.

#### 13. Reduce ScriptProcessorNode Buffer Size

Reduce from 4096 samples (256ms) to 2048 (128ms) or 1024 (64ms). Smaller chunks mean finer-grained VAD and faster wake-word response. Trade-off: slightly more CPU overhead from more frequent callbacks.

---

## Recommended Implementation Order

```
Week 1-2:    Keyword spotting (Tier 1 #1)
             Remove greeting (Tier 1 #2)
             → Wakes instantly, saves 2-4s on every interaction

Week 2-3:    Streaming transcription (Tier 1 #3)
             Partial token TTS (Tier 1 #4)
             → Saves another 1-2s, makes speech feel real-time

Week 3-4:    Rust TTS backend (Tier 1 #5)
             Raw IPC for audio (Tier 2 #6)
             AudioWorklet (Tier 2 #9)
             → Consistent performance, no main-thread contention

Ongoing:     Adaptive silence (Tier 2 #8)
             TTS caching (Tier 3 #10)
             Hybrid TTS (Tier 3 #11)
             Threshold tuning (Tier 3 #12)
```

---

## Appendix: Key Source Locations

| File | Role |
|------|------|
| `packages/gui/src/components/VoiceMode.tsx` | Frontend orchestrator (pipeline state machine) |
| `packages/gui/src/lib/voiceEngine.ts` | STT/TTS bridge (Tauri IPC + Kokoro) |
| `packages/gui/src-tauri/src/voice/mod.rs` | Native whisper.cpp STT |
| `packages/gui/src-tauri/src/lib.rs` | Command registration, global shortcut setup |
| `packages/gui-remix/...` | Mirror copies of all files above |

Both `gui/` and `gui-remix/` contain identical copies — any changes need to be made in both.
