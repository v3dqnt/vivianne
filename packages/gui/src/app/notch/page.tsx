"use client";

/**
 * Notch window — a separate always-on-top transparent Tauri window that hosts
 * only the VoiceMode component. Declared in tauri.conf.json under label "notch".
 *
 * The window is created by the main window when voice mode is enabled, but it
 * keeps ITSELF hidden while passively listening (selfManageWindow) and only
 * reveals when the wake word is detected. It shares backend session state with
 * the main window through Rust (shared AppState; events broadcast to all
 * windows).
 */

import React, { useEffect, useState } from "react";
import { VoiceMode } from "@/components/VoiceMode";
import { useAgent } from "@/components/AgentProvider";

export default function NotchPage() {
  const { getSettings } = useAgent();
  const [voiceSettings, setVoiceSettings] = useState<any>({ enabled: false });

  // Strip the main-window body chrome (bg-canvas, min-height flex) so the
  // transparent Tauri notch window shows through. Must run AFTER hydration —
  // mutating <body> styles at module scope causes a React hydration mismatch.
  useEffect(() => {
    document.body.style.background = "transparent";
    document.body.style.minHeight = "0";
    document.documentElement.style.background = "transparent";
  }, []);

  useEffect(() => {
    let alive = true;
    const load = () => {
      getSettings().then((s) => {
        if (!alive) return;
        setVoiceSettings(s?.voice || {});
      });
    };
    load();
    const t = window.setInterval(load, 3000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [getSettings]);

  // Center the notch horizontally on the primary monitor (the tauri.conf.json
  // position is a hardcoded guess; real monitor width is only known at runtime).
  useEffect(() => {
    if (typeof window === "undefined" || !(window as any).__TAURI_INTERNALS__) return;
    (async () => {
      try {
        const { getCurrentWindow, currentMonitor, LogicalPosition } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        const monitor = await currentMonitor();
        if (!monitor) return;
        const scale = monitor.scaleFactor || 1;
        const screenW = monitor.size.width / scale;
        const winW = 520;
        await win.setPosition(new LogicalPosition(Math.round((screenW - winW) / 2), 12));
      } catch (e) {
        console.error("notch centering failed", e);
      }
    })();
  }, []);

  return (
    <div className="fixed inset-0 bg-transparent overflow-hidden flex items-start justify-center pt-1">
      <VoiceMode settings={voiceSettings} selfManageWindow />
    </div>
  );
}
