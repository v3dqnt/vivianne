"use client";

import React, { useState } from "react";
import { Button } from "@/components/ui/button";

type Status =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none" }
  | { kind: "available"; version: string }
  | { kind: "installing" }
  | { kind: "error"; message: string };

/**
 * Manual "check for updates" control, backed by the Tauri updater plugin
 * (GitHub Releases). Kept in its own component so the plugin imports are
 * dynamic — they only load when this actually renders, and the whole thing
 * degrades to an error message rather than crashing if the updater isn't
 * available (e.g. a dev build with no signed release to check against).
 */
export function UpdateChecker() {
  const [status, setStatus] = useState<Status>({ kind: "idle" });

  const check = async () => {
    setStatus({ kind: "checking" });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (!update) {
        setStatus({ kind: "none" });
        return;
      }
      setStatus({ kind: "available", version: update.version });
    } catch (err) {
      setStatus({ kind: "error", message: String(err) });
    }
  };

  const installAndRelaunch = async () => {
    setStatus({ kind: "installing" });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const { relaunch } = await import("@tauri-apps/plugin-process");
      const update = await check();
      if (!update) {
        setStatus({ kind: "none" });
        return;
      }
      await update.downloadAndInstall();
      await relaunch();
    } catch (err) {
      setStatus({ kind: "error", message: String(err) });
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-3">
        <Button size="sm" variant="outline" onClick={check} disabled={status.kind === "checking" || status.kind === "installing"}>
          {status.kind === "checking" ? "Checking…" : "Check for updates"}
        </Button>
        {status.kind === "available" && (
          <Button size="sm" onClick={installAndRelaunch}>
            Update to v{status.version} & restart
          </Button>
        )}
      </div>
      {status.kind === "none" && <p className="text-xs text-mute font-sans">You're on the latest version.</p>}
      {status.kind === "installing" && <p className="text-xs text-mute font-sans">Downloading and installing…</p>}
      {status.kind === "error" && <p className="text-xs text-amber-600 font-sans">Update check failed: {status.message}</p>}
    </div>
  );
}
