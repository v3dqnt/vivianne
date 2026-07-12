"use client";

/**
 * Voice Mode — real STT + TTS pipeline.
 *
 *   Mic → WebAudio worklet → energy VAD → 16 kHz mono Float32 chunks →
 *   Whisper (transformers.js ONNX) → wake-word gate → sendMessage →
 *   assistant reply → Kokoro TTS playback.
 *
 * Runs entirely in the Tauri webview via ONNX Runtime Web. First load
 * downloads/caches the model (~40 MB Whisper-tiny.en, ~330 MB Kokoro).
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import { HugeiconsIcon } from "@hugeicons/react";
import { Mic01Icon, MicOff01Icon, Cancel01Icon } from "@hugeicons/core-free-icons";
import { useAgent } from "./AgentProvider";
import { cn } from "@/lib/utils";
import { getWhisper, getKokoro, transcribe, speakKokoro, speakBrowser } from "@/lib/voiceEngine";
import { Persona, type PersonaState } from "./ai-elements/persona";

interface VoiceSettings {
  enabled?: boolean;
  ttsEngine?: "kokoro" | "browser" | "off";
  ttsVoice?: string;
  sttEngine?: "browser" | "whisper";
  whisperModel?: string;
}

interface VoiceModeProps {
  settings: VoiceSettings;
  /**
   * When true, this instance drives its own Tauri window's show/hide: the
   * window stays hidden while passively listening and only appears once the
   * wake word fires (through to the reply). Used by the standalone notch window.
   */
  selfManageWindow?: boolean;
}

const WAKE_WORDS = ["vivianne", "viv", "vivian"];
const SAMPLE_RATE = 16000;
const MIN_UTTERANCE_MS = 400;
const SILENCE_TAIL_MS = 900;
const ENERGY_THRESHOLD = 0.008; // RMS gate
const MAX_UTTERANCE_MS = 15000;

// Short greetings spoken right after the wake word so the user knows it heard
// them and is now listening for the actual command. Kept short for low latency.
const GREETINGS = ["Hey, I'm listening.", "Yes?", "Go ahead.", "Hi — what can I do?"];

function detectWake(t: string): boolean {
  const s = t.toLowerCase();
  return WAKE_WORDS.some((w) => s.includes(w));
}

/**
 * The instruction wrapped around each spoken command before it goes to the
 * agent. This is what makes voice mode conditionally screenshot and use tools:
 * the model reads these rules and decides which tools (if any) a given command
 * needs. It runs through the exact same agent loop as the chat window, so every
 * tool — web_search, web_fetch, bash/powershell, todo_write, sleep, computer —
 * is already available; the prompt just tells it how to behave over voice.
 */
function voicePrompt(command: string): string {
  return `[voice command] ${command}

You are Vivianne answering by voice. Follow these rules:
- Reply briefly and conversationally — your answer is read aloud, so no markdown, lists, or code blocks unless truly necessary.
- Screenshot ONLY when the answer genuinely requires seeing the screen (e.g. "what's on my screen", "what does this error say", "read this for me", "what am I looking at"). Do this by calling the computer tool with action="screenshot", then analyse the image. For everything else, do NOT take a screenshot.
- Use your other tools when they help: web_search / web_fetch for current or external facts, bash or powershell to inspect or act on the system, todo_write to track a multi-step task, sleep to wait.
- Keep it to a sentence or two unless the user asks for detail.`;
}

// ── Mic capture with energy VAD ────────────────────────────────────────────
type PipelineStatus = "idle" | "starting" | "listening" | "greeting" | "active" | "transcribing" | "responding" | "error";

interface Recorder {
  stop: () => Promise<Float32Array>;
  cancel: () => void;
}

async function openMic(): Promise<{ stream: MediaStream; ctx: AudioContext; source: MediaStreamAudioSourceNode } | null> {
  try {
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, sampleRate: SAMPLE_RATE },
    });
    const ctx = new AudioContext({ sampleRate: SAMPLE_RATE });
    if (ctx.state === "suspended") await ctx.resume();
    const source = ctx.createMediaStreamSource(stream);
    return { stream, ctx, source };
  } catch (e) {
    console.error("mic open failed", e);
    return null;
  }
}

export function VoiceMode({ settings, selfManageWindow }: VoiceModeProps) {
  const { sendMessage, messages, isBusy, agentReset } = useAgent();
  const [status, setStatus] = useState<PipelineStatus>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [transcript, setTranscript] = useState("");
  const [lastReply, setLastReply] = useState("");
  const [speaking, setSpeaking] = useState(false);

  const enabled = !!settings.enabled;
  const ttsEngine = settings.ttsEngine ?? "kokoro";
  const voice = settings.ttsVoice ?? "af_heart";
  const whisperModel = settings.whisperModel ?? "tiny.en";

  // Refs
  const runningRef = useRef(false);
  const activeRef = useRef(false); // wake word triggered
  const bufRef = useRef<Float32Array[]>([]);
  const activeStartRef = useRef(0);
  const lastVoiceMsRef = useRef(0);
  // Monotonic ms clock advanced by the audio callback; shared so activation
  // handlers can seed timers to "now" instead of 0 (which would make the
  // silence/elapsed check fire instantly and finalize an empty utterance).
  const tickRef = useRef(0);
  // The reply text already sent to TTS, tracked as a content PREFIX rather than
  // a message id. The backend stamps a fresh random id on every reload
  // (turn_end/agent_end both call loadMessages), so id-based tracking reset each
  // reload and re-spoke the whole reply on a loop. A prefix survives reloads:
  // identical text → nothing new to speak; streamed growth → speak only the delta.
  const spokenPrefixRef = useRef("");
  const speakQueueRef = useRef<Promise<void>>(Promise.resolve());
  const audioRef = useRef<{ stream: MediaStream; ctx: AudioContext; source: MediaStreamAudioSourceNode } | null>(null);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  // True from the moment a wake fires until the session returns to passive
  // listening. Guards against re-triggering a new thread mid-session and lets
  // the mic loop know a command capture is already in progress.
  const sessionInFlightRef = useRef(false);
  // The mic loop is created once (on enable) and closes over stale handlers;
  // route its two callbacks through a ref so it always calls the latest.
  const handlersRef = useRef<{ tryWakeCheck: () => void; finalizeUtterance: () => void } | null>(null);

  // ── Preload whisper (STT) as soon as voice mode is enabled ──────────────
  useEffect(() => {
    if (!enabled) return;
    setStatus("starting");
    getWhisper(whisperModel)
      .then(() => setStatus("listening"))
      .catch((e) => {
        console.error("whisper load failed", e);
        setErrorMsg("Failed to load Whisper model. Check network.");
        setStatus("error");
      });
  }, [enabled, whisperModel]);

  // ── Warm up Kokoro (TTS) too, so the greeting + first reply aren't slow ──
  useEffect(() => {
    if (!enabled || ttsEngine !== "kokoro") return;
    getKokoro().catch(() => {}); // failure falls back to browser speech at speak time
  }, [enabled, ttsEngine]);

  // ── Mic loop ────────────────────────────────────────────────────────────
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;

    (async () => {
      const audio = await openMic();
      if (cancelled) {
        audio?.stream.getTracks().forEach((t) => t.stop());
        audio?.ctx.close();
        return;
      }
      if (!audio) {
        setErrorMsg("Microphone unavailable. Grant mic permission and reload.");
        setStatus("error");
        return;
      }
      audioRef.current = audio;

      // ScriptProcessorNode is deprecated but works everywhere including WebView2.
      // AudioWorklet would need a separate worklet file — heavier setup for
      // marginal gain at 16 kHz.
      const proc = audio.ctx.createScriptProcessor(4096, 1, 1);
      processorRef.current = proc;

      const chunkMs = (4096 / SAMPLE_RATE) * 1000;
      let lastWakeCheckAt = 0; // wake-check cadence, separate from utterance timers

      proc.onaudioprocess = (ev) => {
        if (!runningRef.current) return;
        const input = ev.inputBuffer.getChannelData(0);
        // RMS energy for VAD gate
        let sum = 0;
        for (let i = 0; i < input.length; i++) sum += input[i] * input[i];
        const rms = Math.sqrt(sum / input.length);
        const isVoice = rms > ENERGY_THRESHOLD;

        tickRef.current += chunkMs;
        const tick = tickRef.current;
        if (isVoice) lastVoiceMsRef.current = tick;

        if (activeRef.current) {
          // Accumulate every chunk while active
          bufRef.current.push(new Float32Array(input));

          const elapsed = tick - activeStartRef.current;
          const silence = tick - lastVoiceMsRef.current;
          // Only end the utterance once we've actually heard speech (some voiced
          // audio arrived after activation) AND it then went quiet.
          const heardSomething = lastVoiceMsRef.current > activeStartRef.current;
          if (
            (heardSomething && silence > SILENCE_TAIL_MS && elapsed > MIN_UTTERANCE_MS) ||
            elapsed > MAX_UTTERANCE_MS
          ) {
            void handlersRef.current?.finalizeUtterance();
          }
        } else if (isVoice) {
          // Buffer while gating for wake word — keep last 3s of pre-roll audio
          bufRef.current.push(new Float32Array(input));
          const totalSamples = bufRef.current.reduce((a, b) => a + b.length, 0);
          const maxSamples = 3 * SAMPLE_RATE;
          while (bufRef.current.length > 1 && totalSamples > maxSamples) bufRef.current.shift();

          // Every ~1.2s while there's voice, run a lightweight wake-word check
          if (tick - lastWakeCheckAt > 1200 && bufRef.current.length > 0) {
            lastWakeCheckAt = tick;
            void handlersRef.current?.tryWakeCheck();
          }
        } else {
          // Silence and not active — drop stale buffer
          if (bufRef.current.length && tick - lastVoiceMsRef.current > 1500) {
            bufRef.current = [];
          }
        }
      };

      audio.source.connect(proc);
      // Chrome quirk: ScriptProcessor needs a sink to actually run.
      const silentGain = audio.ctx.createGain();
      silentGain.gain.value = 0;
      proc.connect(silentGain);
      silentGain.connect(audio.ctx.destination);
      runningRef.current = true;
      setStatus("listening");
    })();

    return () => {
      cancelled = true;
      runningRef.current = false;
      try {
        processorRef.current?.disconnect();
      } catch {}
      audioRef.current?.stream.getTracks().forEach((t) => t.stop());
      audioRef.current?.ctx.close().catch(() => {});
      audioRef.current = null;
      processorRef.current = null;
      bufRef.current = [];
      activeRef.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled]);

  // ── Concatenate current buffer into a single Float32Array ────────────────
  const flattenBuffer = useCallback((): Float32Array => {
    const chunks = bufRef.current;
    const total = chunks.reduce((a, b) => a + b.length, 0);
    const out = new Float32Array(total);
    let o = 0;
    for (const c of chunks) {
      out.set(c, o);
      o += c.length;
    }
    return out;
  }, []);

  // ── Speak a short greeting (awaited so mic capture starts only after) ─────
  const speakGreeting = useCallback(async () => {
    if (ttsEngine === "off") return;
    const g = GREETINGS[Math.floor(Math.random() * GREETINGS.length)];
    setSpeaking(true);
    try {
      if (ttsEngine === "kokoro") await speakKokoro(g, voice);
      else await speakBrowser(g);
    } catch {
      try {
        await speakBrowser(g);
      } catch {}
    } finally {
      setSpeaking(false);
    }
  }, [ttsEngine, voice]);

  // ── Send a finished command to the agent (same loop as the chat window) ───
  const processCommand = useCallback(
    async (text: string) => {
      setTranscript(text);
      setStatus("responding");
      try {
        await sendMessage(voicePrompt(text));
      } catch (e: any) {
        console.error("voice command failed", e);
        setErrorMsg(String(e?.message ?? e));
        setStatus("error");
        sessionInFlightRef.current = false;
        window.setTimeout(() => {
          setErrorMsg(null);
          setStatus("listening");
        }, 4000);
      }
    },
    [sendMessage]
  );

  // ── Finalize an active utterance → transcribe → process command ──────────
  const finalizeInFlight = useRef(false);
  const finalizeUtterance = useCallback(async () => {
    if (finalizeInFlight.current) return;
    finalizeInFlight.current = true;
    const audio = flattenBuffer();
    bufRef.current = [];
    activeRef.current = false;
    activeStartRef.current = tickRef.current;
    lastVoiceMsRef.current = tickRef.current;
    setStatus("transcribing");
    try {
      const text = (await transcribe(whisperModel, audio, SAMPLE_RATE)).trim();
      if (!text) {
        // Heard nothing usable — end the session and go back to passive.
        sessionInFlightRef.current = false;
        setStatus("listening");
        return;
      }
      await processCommand(text);
    } catch (e: any) {
      console.error("finalize failed", e);
      setErrorMsg(String(e?.message ?? e));
      setStatus("error");
      sessionInFlightRef.current = false;
      window.setTimeout(() => {
        setErrorMsg(null);
        setStatus("listening");
      }, 4000);
    } finally {
      finalizeInFlight.current = false;
    }
  }, [flattenBuffer, processCommand, whisperModel]);

  // ── Begin a voice session: fresh invocation → new thread, greet, listen ──
  // Two-phase: the wake word/shortcut only opens the session and greets; the
  // NEXT utterance is captured as the actual command (finalizeUtterance).
  const beginSession = useCallback(async () => {
    if (sessionInFlightRef.current) return;
    sessionInFlightRef.current = true;
    // Fresh voice invocation → start a brand-new agent thread.
    spokenPrefixRef.current = "";
    activeRef.current = false;
    bufRef.current = [];
    setTranscript("");
    setLastReply("");
    await agentReset();
    // Greet, then arm capture of the command utterance.
    setStatus("greeting");
    await speakGreeting();
    activeRef.current = true;
    activeStartRef.current = tickRef.current;
    lastVoiceMsRef.current = tickRef.current;
    bufRef.current = [];
    setStatus("active");
  }, [agentReset, speakGreeting]);

  // ── Wake-word check on pre-roll buffer ───────────────────────────────────
  const wakeCheckInFlight = useRef(false);
  const tryWakeCheck = useCallback(async () => {
    if (wakeCheckInFlight.current || activeRef.current || sessionInFlightRef.current) return;
    wakeCheckInFlight.current = true;
    try {
      const audio = flattenBuffer();
      if (audio.length < SAMPLE_RATE / 2) return;
      const text = await transcribe(whisperModel, audio, SAMPLE_RATE);
      if (text && detectWake(text)) {
        bufRef.current = [];
        void beginSession();
      }
    } catch (e) {
      // Ignore per-chunk failures — model may still be warming up
    } finally {
      wakeCheckInFlight.current = false;
    }
  }, [flattenBuffer, whisperModel, beginSession]);

  // Keep the mic loop's callbacks fresh (it's created once, on enable).
  handlersRef.current = { tryWakeCheck, finalizeUtterance };

  // ── Force-activate (global shortcut) — same two-phase session as wake ─────
  const forceActivate = useCallback(() => {
    void beginSession();
  }, [beginSession]);

  // Listen for the backend "voice-wake" event (Ctrl+Shift+V global shortcut).
  useEffect(() => {
    if (!enabled) return;
    if (typeof window === "undefined" || !(window as any).__TAURI_INTERNALS__) return;
    let unlisten: (() => void) | null = null;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen("voice-wake", () => forceActivate());
      } catch (e) {
        console.error("voice-wake listen failed", e);
      }
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, [enabled, forceActivate]);

  // ── Assistant reply extraction ───────────────────────────────────────────
  const extractText = (m: any): string => {
    const content = m?.content;
    if (typeof content === "string") return content;
    if (Array.isArray(content)) {
      return content
        .filter((c: any) => c?.type === "text" || typeof c === "string")
        .map((c: any) => (typeof c === "string" ? c : c.text))
        .join(" ");
    }
    return "";
  };

  // Current (possibly still-streaming) assistant reply — the notch shows the
  // answer appearing in real time, and it's also what we speak from.
  const liveMessage = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i];
      if (m.role === "assistant") {
        const t = extractText(m).trim();
        if (t || m.isStreaming) return { id: m.id, text: t, isStreaming: !!m.isStreaming };
      }
    }
    return null;
  }, [messages]);
  const liveReply = liveMessage?.text ?? "";

  const enqueueSpeech = useCallback(
    (text: string) => {
      if (!text.trim()) return;
      speakQueueRef.current = speakQueueRef.current.then(async () => {
        setSpeaking(true);
        try {
          if (ttsEngine === "kokoro") await speakKokoro(text, voice);
          else await speakBrowser(text);
        } catch (e) {
          console.error("[TTS] Kokoro failed, falling back to browser speech:", e);
          if (ttsEngine === "kokoro") {
            try {
              await speakBrowser(text);
            } catch {}
          }
        } finally {
          setSpeaking(false);
        }
      });
    },
    [ttsEngine, voice]
  );

  useEffect(() => {
    if (!enabled || ttsEngine === "off") return;
    // Only speak replies triggered by voice (status is responding), so opening
    // the app with old messages doesn't start narrating history.
    if (status !== "responding") return;
    if (!liveMessage) return;

    const text = liveMessage.text;
    // If the current reply no longer begins with what we've already spoken, it's
    // a genuinely different message (e.g. a second assistant turn after a tool
    // call) — start over. A reload with identical text still matches, so nothing
    // gets re-spoken.
    if (!text.startsWith(spokenPrefixRef.current)) {
      spokenPrefixRef.current = "";
    }
    const unspoken = text.slice(spokenPrefixRef.current.length);
    if (!unspoken) return;

    if (!liveMessage.isStreaming) {
      // Reply finished — speak whatever's left (including a trailing partial sentence).
      spokenPrefixRef.current = text;
      setLastReply(text.slice(0, 800));
      enqueueSpeech(unspoken.slice(0, 800));
      return;
    }

    // Still streaming — speak each completed sentence as soon as it lands
    // instead of waiting for the whole reply.
    const match = /^([\s\S]*[.!?])(?:\s+|$)/.exec(unspoken);
    if (match) {
      spokenPrefixRef.current += match[0];
      enqueueSpeech(match[1].slice(0, 800));
    }
  }, [liveMessage, enabled, ttsEngine, status, enqueueSpeech]);

  // ── Linger, then return to passive listening ─────────────────────────────
  // Leave the reply on screen until the agent is done AND speech has finished,
  // plus a short beat to read it. Safety timeout after 120s regardless.
  useEffect(() => {
    if (status !== "responding") return;
    if (isBusy || speaking) return;
    if (!lastReply && !liveReply) return;
    const t = window.setTimeout(() => {
      sessionInFlightRef.current = false;
      setStatus("listening");
      setTranscript("");
      setLastReply("");
    }, 3500);
    return () => window.clearTimeout(t);
  }, [status, isBusy, speaking, lastReply, liveReply]);

  useEffect(() => {
    if (status !== "responding") return;
    const t = window.setTimeout(() => {
      sessionInFlightRef.current = false;
      setStatus("listening");
    }, 120000);
    return () => window.clearTimeout(t);
  }, [status]);

  // When this instance owns a dedicated Tauri window (the notch), keep the
  // window hidden while passively listening and only reveal it once the wake
  // word fires (active → transcribing → responding), plus on error. Model
  // loading ("starting") stays hidden so nothing shows until you speak.
  const windowVisible =
    status === "greeting" || status === "active" || status === "transcribing" || status === "responding" || status === "error";
  // Reply panel needs more vertical room than the bare pill.
  const wantsTallWindow = status === "responding" || (status === "active" && !!transcript);
  useEffect(() => {
    if (!selfManageWindow) return;
    if (typeof window === "undefined" || !(window as any).__TAURI_INTERNALS__) return;
    let cancelled = false;
    (async () => {
      try {
        const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
        const { LogicalSize } = await import("@tauri-apps/api/dpi");
        const win = getCurrentWebviewWindow();
        if (cancelled) return;
        if (windowVisible) {
          try {
            await win.setSize(new LogicalSize(520, wantsTallWindow ? 240 : 96));
          } catch {}
          await win.show();
          try {
            await win.setAlwaysOnTop(true);
          } catch {}
        } else {
          await win.hide();
        }
      } catch (e) {
        console.error("notch self-window sync failed", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selfManageWindow, windowVisible, wantsTallWindow]);

  if (!enabled) return null;
  const visible =
    status === "greeting" ||
    status === "active" ||
    status === "transcribing" ||
    status === "responding" ||
    status === "error" ||
    status === "starting";

  const statusWord =
    status === "starting"
      ? "Loading model…"
      : status === "error"
        ? errorMsg ?? "Voice error"
        : status === "greeting"
          ? "Hi there…"
          : status === "active"
            ? "Listening — speak now"
            : status === "transcribing"
              ? "Transcribing…"
              : status === "responding"
                ? isBusy
                  ? "Thinking…"
                  : speaking
                    ? "Speaking…"
                    : "Done"
                : `Say "Vivianne"`;

  const showReply = status === "responding" && (liveReply || lastReply);

  const personaState: PersonaState =
    status === "greeting"
      ? "speaking"
      : status === "active"
        ? "listening"
        : status === "transcribing"
          ? "thinking"
          : status === "responding"
            ? isBusy
              ? "thinking"
              : speaking
                ? "speaking"
                : "idle"
            : status === "error"
              ? "idle"
              : "asleep";

  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ opacity: 0, y: -12 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -12 }}
          transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
          className="fixed top-1 left-1/2 -translate-x-1/2 z-[9998] pointer-events-auto flex flex-col items-center"
        >
          {/* Status pill */}
          <div
            className={cn(
              "bg-ink text-on-primary shadow-lg",
              "flex items-center gap-2.5 px-3 py-1.5 rounded-full",
              "text-[11px] font-mono select-none max-w-[480px]"
            )}
          >
            {status === "error" ? (
              <HugeiconsIcon icon={MicOff01Icon} size={14} className="flex-shrink-0" />
            ) : (
              <Persona state={personaState} variant="opal" className="w-6 h-6 flex-shrink-0" />
            )}
            <span className="truncate font-semibold">{statusWord}</span>
            <button
              onClick={() => {
                bufRef.current = [];
                activeRef.current = false;
                sessionInFlightRef.current = false;
                spokenPrefixRef.current = "";
                setTranscript("");
                setLastReply("");
                setStatus("listening");
                // Closing the notch ends this voice thread. The next wake starts
                // a fresh conversation; wakes without a dismiss continue this one.
                void agentReset();
              }}
              className="opacity-70 hover:opacity-100 flex-shrink-0"
              title="Close (starts a new thread next time)"
            >
              <HugeiconsIcon icon={Cancel01Icon} size={10} />
            </button>
          </div>

          {/* Heard transcript while capturing */}
          {status === "active" && transcript && (
            <div className="mt-1.5 max-w-[460px] px-3.5 py-1.5 text-[11px] font-sans text-on-primary/90 bg-ink rounded-2xl">
              “{transcript}”
            </div>
          )}

          {/* Streaming / final reply */}
          {showReply && (
            <motion.div
              initial={{ opacity: 0, y: -6 }}
              animate={{ opacity: 1, y: 0 }}
              className="mt-1.5 max-w-[460px] px-4 py-2.5 bg-ink rounded-2xl"
            >
              {transcript && (
                <div className="text-[10px] font-mono text-white/50 mb-1 truncate">You: “{transcript}”</div>
              )}
              <div className="text-[12px] font-sans leading-relaxed text-white/95 max-h-24 overflow-hidden [display:-webkit-box] [-webkit-line-clamp:4] [-webkit-box-orient:vertical]">
                {liveReply || lastReply}
              </div>
            </motion.div>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
