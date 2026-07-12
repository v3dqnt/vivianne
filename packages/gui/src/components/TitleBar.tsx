"use client";

import { useEffect, useState } from "react";

export function TitleBar() {
  const [isTauri, setIsTauri] = useState(false);
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    const tauri = !!(window as any).__TAURI_INTERNALS__;
    setIsTauri(tauri);
    if (!tauri) return;
    const checkMax = async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      setIsMaximized(await getCurrentWindow().isMaximized());
    };
    checkMax();
    window.addEventListener("resize", checkMax);
    return () => window.removeEventListener("resize", checkMax);
  }, []);

  const minimize = async () => {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().minimize();
  };

  const toggleMaximize = async () => {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().toggleMaximize();
  };

  const close = async () => {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
  };

  if (!isTauri) return null;

  return (
    <div
      data-tauri-drag-region
      className="h-8 w-full flex items-center justify-between flex-shrink-0 select-none px-2 bg-canvas/60 backdrop-blur-sm z-50"
    >
      {/* App name — centered in the drag region */}
      <div data-tauri-drag-region className="flex-1 flex items-center pl-1">
        <span className="text-[11px] font-semibold font-sans text-mute tracking-tight pointer-events-none">
          Vivianne
        </span>
      </div>

      {/* Window controls — Windows style, right side */}
      <div className="flex items-center gap-0.5 flex-shrink-0">
        <button
          onClick={minimize}
          title="Minimize"
          className="w-8 h-8 flex items-center justify-center rounded text-mute hover:text-ink hover:bg-surface-soft transition-colors text-sm"
        >
          <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
            <rect width="10" height="1" />
          </svg>
        </button>
        <button
          onClick={toggleMaximize}
          title={isMaximized ? "Restore" : "Maximize"}
          className="w-8 h-8 flex items-center justify-center rounded text-mute hover:text-ink hover:bg-surface-soft transition-colors"
        >
          {isMaximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
              <rect x="2" y="0" width="8" height="8" />
              <polyline points="0,2 0,10 8,10" />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
              <rect x="0" y="0" width="10" height="10" />
            </svg>
          )}
        </button>
        <button
          onClick={close}
          title="Close"
          className="w-8 h-8 flex items-center justify-center rounded text-mute hover:text-white hover:bg-red-500 transition-colors text-xs"
        >
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.5">
            <line x1="0" y1="0" x2="10" y2="10" />
            <line x1="10" y1="0" x2="0" y2="10" />
          </svg>
        </button>
      </div>
    </div>
  );
}
