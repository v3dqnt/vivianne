"use client";

import React, { useMemo, useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import { HugeiconsIcon } from "@hugeicons/react";
import {
  Settings02Icon,
  PlusSignIcon,
  Search01Icon,
  GitForkIcon,
  Copy01Icon,
  SourceCodeSquareIcon,
  Cancel01Icon,
  PencilEdit02Icon,
} from "@hugeicons/core-free-icons";
import { useAgent } from "./AgentProvider";
import { Button } from "@/components/ui/button";
import { cn, formatRelativeTime, groupByRelativeDate } from "@/lib/utils";
import { RenameSessionDialog } from "./RenameSessionDialog";

export function AppSidebar({
  onNavigate,
}: {
  onNavigate?: (view: "chat" | "settings") => void;
}) {
  const {
    agentReset,
    sessions,
    loadSession,
    forkSession,
    cloneSession,
    renameSession,
  } = useAgent();
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [renameTarget, setRenameTarget] = useState<{ id: string; name: string } | null>(null);

  const filteredSessions = useMemo(() => {
    if (!query.trim()) return sessions;
    const q = query.trim().toLowerCase();
    return sessions.filter((s) => s.name?.toLowerCase().includes(q));
  }, [sessions, query]);

  const groups = useMemo(
    () => groupByRelativeDate(filteredSessions, (s) => s.created_at),
    [filteredSessions]
  );

  const handleNewChat = async () => {
    await agentReset();
    setActiveSessionId(null);
    onNavigate?.("chat");
  };

  const handleSelectSession = async (id: string) => {
    setActiveSessionId(id);
    await loadSession(id);
    onNavigate?.("chat");
  };

  const handleFork = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    const newId = await forkSession(id);
    if (newId) {
      setActiveSessionId(newId);
      onNavigate?.("chat");
    }
  };

  const handleClone = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    const newId = await cloneSession(id);
    if (newId) {
      setActiveSessionId(newId);
      onNavigate?.("chat");
    }
  };

  const handleRenameClick = (e: React.MouseEvent, id: string, name: string) => {
    e.stopPropagation();
    setRenameTarget({ id, name });
  };

  return (
    <aside className="bg-canvas-warm w-64 h-full flex flex-col flex-shrink-0 select-none">
      {/* Brand */}
      <div className="flex items-center gap-2.5 px-4 h-12 flex-shrink-0">
        <div className="w-6 h-6 bg-ink rounded-md flex items-center justify-center text-on-primary flex-shrink-0">
          <HugeiconsIcon icon={SourceCodeSquareIcon} size={13} />
        </div>
        <span className="text-[13px] font-bold text-ink tracking-tight font-sans">Vivianne</span>
      </div>

      {/* Toolbar */}
      <div className="px-3 pb-3 flex items-center gap-1.5">
        <Button onClick={handleNewChat} size="sm" className="flex-1 justify-center">
          <HugeiconsIcon icon={PlusSignIcon} size={13} />
          <span>New Chat</span>
        </Button>
        <Button
          variant={searchOpen ? "secondary" : "ghost"}
          size="icon-sm"
          onClick={() => setSearchOpen((v) => !v)}
          aria-label="Search sessions"
        >
          <HugeiconsIcon icon={Search01Icon} size={14} />
        </Button>
        <Button variant="ghost" size="icon-sm" onClick={() => onNavigate?.("settings")} aria-label="Settings">
          <HugeiconsIcon icon={Settings02Icon} size={14} />
        </Button>
      </div>

      {/* Search input */}
      <AnimatePresence initial={false}>
        {searchOpen && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="overflow-hidden px-3"
          >
            <div className="relative pb-3">
              <input
                autoFocus
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search chats..."
                className="w-full bg-canvas/40 backdrop-blur-sm border border-hairline/60 rounded-full pl-3 pr-8 py-1.5 text-xs font-sans text-ink placeholder:text-mute outline-none focus-visible:border-hairline-strong"
              />
              {query && (
                <button
                  onClick={() => setQuery("")}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-mute hover:text-ink"
                >
                  <HugeiconsIcon icon={Cancel01Icon} size={12} />
                </button>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Session History List */}
      <div className="flex-1 overflow-y-auto px-3 space-y-4 pb-2">
        {filteredSessions.length === 0 ? (
          <div className="px-1 py-4 text-xs text-mute font-sans italic">
            {query ? "No matching chats." : "No past chats."}
          </div>
        ) : (
          groups.map((group) => (
            <div key={group.label}>
              <div className="px-1 pb-1.5 text-[10px] font-bold text-mute uppercase tracking-wider font-sans">
                {group.label}
              </div>
              <div className="space-y-0.5">
                <AnimatePresence initial={false}>
                  {group.items.map((session, idx) => {
                    const isActive = activeSessionId === session.id;
                    return (
                      <motion.div
                        key={session.id}
                        initial={{ opacity: 0, y: 4 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, height: 0 }}
                        transition={{ duration: 0.2, delay: Math.min(idx * 0.02, 0.2) }}
                        onClick={() => handleSelectSession(session.id)}
                        className={cn(
                          "group w-full flex items-center justify-between px-2.5 py-2 rounded-lg text-xs font-medium cursor-pointer transition-colors font-sans",
                          isActive ? "bg-surface-soft text-ink font-semibold" : "text-body hover:text-ink hover:bg-surface-soft/60"
                        )}
                      >
                        <span className="truncate flex-1 min-w-0">{session.name}</span>

                        <div className="flex items-center gap-1 flex-shrink-0">
                          <div className="opacity-0 group-hover:opacity-100 flex items-center gap-0.5 transition-opacity">
                            <button
                              onClick={(e) => handleRenameClick(e, session.id, session.name)}
                              title="Rename Session"
                              className="p-1 text-mute hover:text-ink hover:bg-hairline rounded-md transition-colors"
                            >
                              <HugeiconsIcon icon={PencilEdit02Icon} size={11} />
                            </button>
                            <button
                              onClick={(e) => handleFork(e, session.id)}
                              title="Fork Session"
                              className="p-1 text-mute hover:text-ink hover:bg-hairline rounded-md transition-colors"
                            >
                              <HugeiconsIcon icon={GitForkIcon} size={11} />
                            </button>
                            <button
                              onClick={(e) => handleClone(e, session.id)}
                              title="Clone Session"
                              className="p-1 text-mute hover:text-ink hover:bg-hairline rounded-md transition-colors"
                            >
                              <HugeiconsIcon icon={Copy01Icon} size={11} />
                            </button>
                          </div>
                          <span className="text-[10px] text-mute font-mono group-hover:hidden">
                            {formatRelativeTime(session.created_at)}
                          </span>
                        </div>
                      </motion.div>
                    );
                  })}
                </AnimatePresence>
              </div>
            </div>
          ))
        )}
      </div>

      {/* Sidebar Footer */}
      <div className="px-3 py-3 flex-shrink-0">
        <div className="flex items-center gap-2.5 px-2 py-1.5 rounded-lg text-xs font-semibold text-ink cursor-default font-sans">
          <div className="w-6 h-6 rounded-full bg-surface-soft border border-hairline flex items-center justify-center flex-shrink-0 text-ink">
            <span className="text-[10px] font-bold">U</span>
          </div>
          <span className="truncate">User</span>
        </div>
      </div>

      <RenameSessionDialog
        open={!!renameTarget}
        onOpenChange={(open) => {
          if (!open) setRenameTarget(null);
        }}
        currentName={renameTarget?.name ?? ""}
        onRename={async (name) => {
          if (renameTarget) await renameSession(renameTarget.id, name);
        }}
      />
    </aside>
  );
}
