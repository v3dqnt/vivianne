"use client";

import React, { useState } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import { Folder02Icon, LockIcon, LockPasswordIcon, Delete02Icon, PlusSignIcon, FolderAddIcon } from "@hugeicons/core-free-icons";
import { useAgent } from "./AgentProvider";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  DropdownMenuSeparator,
  DropdownMenuLabel,
} from "@/components/ui/dropdown-menu";

function folderName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

/**
 * Consolidates what used to be two separate composer controls (project-dir
 * button + read-only lock toggle) into one popover, and extends them into a
 * real multi-folder workspace: add existing folders, create new ones on
 * disk, remove folders, toggle read-only. This is a net reduction in
 * composer clutter — one trigger instead of two — even though it does more.
 */
export function WorkspacePanel() {
  const { workspaceFolders, workspaceAccessMode, setAgentWorkspace, createWorkspaceFolder } = useAgent();
  const [creatingFolder, setCreatingFolder] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  const [busy, setBusy] = useState(false);

  const addExistingFolder = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({ directory: true, multiple: false });
      if (!selected || Array.isArray(selected)) return;
      if (workspaceFolders.includes(selected)) return;
      setBusy(true);
      await setAgentWorkspace([...workspaceFolders, selected], workspaceAccessMode);
    } finally {
      setBusy(false);
    }
  };

  const removeFolder = async (path: string) => {
    setBusy(true);
    try {
      await setAgentWorkspace(workspaceFolders.filter((f) => f !== path), workspaceAccessMode);
    } finally {
      setBusy(false);
    }
  };

  const confirmCreateFolder = async () => {
    const name = newFolderName.trim();
    if (!name) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const parent = await open({ directory: true, multiple: false });
      if (!parent || Array.isArray(parent)) return;
      setBusy(true);
      const created = await createWorkspaceFolder(parent, name);
      if (created) {
        await setAgentWorkspace([...workspaceFolders, created], workspaceAccessMode);
      }
      setNewFolderName("");
      setCreatingFolder(false);
    } finally {
      setBusy(false);
    }
  };

  const toggleAccessMode = async () => {
    const next = workspaceAccessMode === "full" ? "readOnly" : "full";
    setBusy(true);
    try {
      await setAgentWorkspace(workspaceFolders, next);
    } finally {
      setBusy(false);
    }
  };

  const triggerLabel =
    workspaceFolders.length === 0 ? "No workspace" : workspaceFolders.length === 1 ? folderName(workspaceFolders[0]) : `${workspaceFolders.length} folders`;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          title={workspaceFolders.join(", ") || "Select a workspace folder"}
          className="flex items-center gap-1.5 px-2.5 py-1 bg-surface-soft hover:bg-accent text-[11px] font-semibold text-ink rounded-full border border-hairline transition-colors select-none max-w-[160px]"
        >
          <HugeiconsIcon icon={Folder02Icon} size={12} className="flex-shrink-0 text-mute" />
          <span className="truncate">{triggerLabel}</span>
          {workspaceAccessMode === "readOnly" && <HugeiconsIcon icon={LockIcon} size={10} className="flex-shrink-0 text-amber-600" />}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" side="top" className="w-80 p-2">
        <DropdownMenuLabel>Workspace folders</DropdownMenuLabel>
        {workspaceFolders.length === 0 ? (
          <p className="px-2 py-2 text-xs text-mute">No folders yet — add one below.</p>
        ) : (
          <div className="space-y-0.5 mb-1.5">
            {workspaceFolders.map((f, i) => (
              <div key={f} className="flex items-center gap-2 px-2 py-1.5 rounded-lg hover:bg-surface-soft group">
                <span className="text-[11px] font-mono text-body truncate flex-1" title={f}>
                  {folderName(f)} {i === 0 && <span className="text-mute">(primary)</span>}
                </span>
                <button
                  onClick={() => removeFolder(f)}
                  disabled={busy}
                  className="opacity-0 group-hover:opacity-100 text-mute hover:text-red-500 transition-opacity flex-shrink-0"
                  aria-label={`Remove ${f}`}
                >
                  <HugeiconsIcon icon={Delete02Icon} size={13} />
                </button>
              </div>
            ))}
          </div>
        )}

        <DropdownMenuSeparator />

        {creatingFolder ? (
          <div className="p-1.5 space-y-1.5">
            <Input
              autoFocus
              value={newFolderName}
              onChange={(e) => setNewFolderName(e.target.value)}
              placeholder="New folder name"
              className="h-8 text-xs"
              onKeyDown={(e) => {
                if (e.key === "Enter") confirmCreateFolder();
                if (e.key === "Escape") setCreatingFolder(false);
              }}
            />
            <div className="flex gap-1.5">
              <Button size="sm" variant="outline" className="flex-1" onClick={() => setCreatingFolder(false)}>
                Cancel
              </Button>
              <Button size="sm" className="flex-1" onClick={confirmCreateFolder} disabled={!newFolderName.trim() || busy}>
                Choose location…
              </Button>
            </div>
          </div>
        ) : (
          <>
            <button
              onClick={addExistingFolder}
              disabled={busy}
              className="w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-xs text-ink hover:bg-surface-soft transition-colors"
            >
              <HugeiconsIcon icon={PlusSignIcon} size={13} />
              Add existing folder…
            </button>
            <button
              onClick={() => setCreatingFolder(true)}
              disabled={busy}
              className="w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-xs text-ink hover:bg-surface-soft transition-colors"
            >
              <HugeiconsIcon icon={FolderAddIcon} size={13} />
              New folder…
            </button>
          </>
        )}

        <DropdownMenuSeparator />

        <button
          onClick={toggleAccessMode}
          disabled={busy}
          className={cn(
            "w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-xs transition-colors",
            workspaceAccessMode === "readOnly" ? "text-amber-700 hover:bg-amber-500/10" : "text-ink hover:bg-surface-soft",
          )}
        >
          <HugeiconsIcon icon={workspaceAccessMode === "full" ? LockPasswordIcon : LockIcon} size={13} />
          {workspaceAccessMode === "full" ? "Switch to read-only" : "Switch to full access"}
        </button>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
