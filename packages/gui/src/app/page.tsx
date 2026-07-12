"use client";

import React, { useState, useEffect } from "react";
import { motion, AnimatePresence } from "motion/react";
import { HugeiconsIcon } from "@hugeicons/react";
import { Alert02Icon } from "@hugeicons/core-free-icons";
import { AppSidebar } from "@/components/Sidebar";
import { ChatInterface } from "@/components/ChatInterface";
import { SettingsPage } from "@/components/SettingsPage";
import { ExtensionUIHandler } from "@/components/ExtensionUIHandler";
import { TitleBar } from "@/components/TitleBar";
import { useAgent } from "@/components/AgentProvider";

export default function Home() {
  const [currentView, setCurrentView] = useState<"chat" | "settings">("chat");
  const [isBrowserOnly, setIsBrowserOnly] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const { getSettings } = useAgent();

  useEffect(() => {
    if (typeof window !== "undefined" && !(window as any).__TAURI_INTERNALS__) {
      setIsBrowserOnly(true);
    }
  }, []);

  // Silent update check on launch. Best-effort and fully non-blocking — any
  // failure (no signed release yet, offline, updater unavailable in a dev
  // build) is swallowed. A found update is surfaced via the manual control in
  // Settings → Usage rather than an intrusive auto-install here.
  useEffect(() => {
    if (typeof window === "undefined" || !(window as any).__TAURI_INTERNALS__) return;
    (async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const update = await check();
        if (update) console.info(`[updater] update available: v${update.version}`);
      } catch {
        /* no-op: updater not available / no release to check */
      }
    })();
  }, []);

  // Ensure the notch window EXISTS (loaded, running its mic + Whisper pipeline)
  // whenever voice mode is enabled. The notch keeps itself hidden while
  // passively listening and reveals only on the wake word (see VoiceMode's
  // selfManageWindow). When voice is disabled we hide the window so the
  // pipeline effectively stops on the next settings poll inside it.
  useEffect(() => {
    if (typeof window === "undefined" || !(window as any).__TAURI_INTERNALS__) return;
    let alive = true;
    let lastEnabled: boolean | null = null;
    const sync = async () => {
      try {
        const s = await getSettings();
        if (!alive) return;
        const enabled = !!s?.voice?.enabled;
        if (enabled === lastEnabled) return;
        lastEnabled = enabled;
        const { WebviewWindow, getAllWebviewWindows } = await import("@tauri-apps/api/webviewWindow");
        let notch: any = await WebviewWindow.getByLabel("notch");
        if (!notch) {
          const all = await getAllWebviewWindows();
          notch = all.find((w: any) => w.label === "notch") ?? null;
        }
        if (!notch && enabled) {
          console.warn("[voice] notch window not found; creating at runtime");
          notch = new WebviewWindow("notch", {
            url: "notch",
            width: 520,
            height: 96,
            resizable: false,
            decorations: false,
            transparent: true,
            alwaysOnTop: true,
            skipTaskbar: true,
            focus: false,
            shadow: false,
            visible: false,
            x: 400,
            y: 12,
          });
        }
        // We intentionally do NOT show() here — the notch reveals itself on the
        // wake word. When disabled, hide it so it disappears immediately.
        if (!enabled && notch) {
          await notch.hide();
        }
      } catch (e) {
        console.error("notch sync failed", e);
      }
    };
    sync();
    const t = window.setInterval(sync, 2000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [getSettings]);

  const toggleSidebar = () => setSidebarCollapsed((v) => !v);

  return (
    <div className="flex flex-col w-full h-screen bg-canvas overflow-hidden font-sans relative">
      {/* Custom OS title bar — draggable, with min/max/close */}
      <TitleBar />

      <div className="flex flex-1 min-h-0 overflow-hidden">
        <motion.div
          animate={{ width: sidebarCollapsed ? 0 : 256 }}
          transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
          className="h-full overflow-hidden flex-shrink-0"
        >
          <AppSidebar onNavigate={setCurrentView} />
        </motion.div>

        <main className="flex-1 flex flex-col h-full min-w-0 relative">
          <ChatInterface onToggleSidebar={toggleSidebar} onOpenSettings={() => setCurrentView("settings")} />
        </main>
      </div>

      {/* Settings — modal over the chat instead of a full page */}
      <AnimatePresence>
        {currentView === "settings" && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15, ease: [0.16, 1, 0.3, 1] }}
            className="fixed inset-0 z-40 flex items-center justify-center p-6 md:p-10"
          >
            {/* Backdrop — click to close */}
            <div
              className="absolute inset-0 bg-ink/25"
              onClick={() => setCurrentView("chat")}
            />
            <motion.div
              initial={{ opacity: 0, y: 14, scale: 0.985 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 14, scale: 0.985 }}
              transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
              className="relative w-full max-w-5xl h-[86vh] rounded-3xl overflow-hidden flex flex-col bg-canvas border border-hairline shadow-xl"
            >
              <button
                onClick={() => setCurrentView("chat")}
                aria-label="Close settings"
                className="absolute top-4 right-4 z-10 w-8 h-8 rounded-full bg-ink/10 hover:bg-ink/20 text-ink flex items-center justify-center transition-colors"
              >
                ✕
              </button>
              <div className="flex-1 min-h-0 overflow-hidden">
                <SettingsPage />
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Extension UI overlay */}
      <ExtensionUIHandler />

      {/* Browser Warning Overlay */}
      <AnimatePresence>
        {isBrowserOnly && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-[9999] flex items-center justify-center bg-canvas/95 backdrop-blur-sm p-6"
          >
            <motion.div
              initial={{ opacity: 0, y: 10, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
              className="max-w-[480px] p-8 rounded-2xl border border-hairline bg-canvas text-center"
            >
              <div className="w-12 h-12 rounded-full bg-surface-soft border border-hairline flex items-center justify-center mx-auto mb-4">
                <HugeiconsIcon icon={Alert02Icon} size={22} className="text-ink" />
              </div>
              <h2 className="text-lg font-semibold font-sans text-ink mb-2">Running in Web Browser</h2>
              <p className="text-body text-sm font-sans leading-relaxed mb-5">
                The backend connection (Tauri IPC) is not active in this tab. Please use the native{" "}
                <strong className="text-ink">Vivianne App</strong> window launched by your terminal instead.
              </p>
              <div className="bg-surface-soft border border-hairline rounded-xl px-4 py-3 text-left">
                <code className="text-xs font-mono text-ink">npm run tauri dev</code>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
