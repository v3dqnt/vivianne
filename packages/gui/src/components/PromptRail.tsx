"use client";

import React, { useMemo, useState } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import { Menu01Icon, Cancel01Icon } from "@hugeicons/core-free-icons";

export interface PromptRailMessage {
  id: string;
  role: string;
  content: string;
}

export interface PromptAnchor {
  id: string;
  index: number;
  preview: string;
  answerPreview: string;
}

const PREVIEW_LEN = 60;

function truncate(text: string, len: number) {
  const t = text.trim().replace(/\s+/g, " ");
  return t.length > len ? `${t.slice(0, len)}…` : t;
}

/** One anchor per user message, with a preview of the prompt and of the
 * next assistant text reply — a native rebuild of the "jump to prompt"
 * concept, not derived from any external source. */
export function buildPromptAnchors(messages: PromptRailMessage[]): PromptAnchor[] {
  const anchors: PromptAnchor[] = [];
  messages.forEach((msg, i) => {
    if (msg.role !== "user") return;
    const next = messages.slice(i + 1).find((m) => m.role === "assistant" && m.content.trim());
    anchors.push({
      id: msg.id,
      index: i,
      preview: truncate(msg.content, PREVIEW_LEN) || "(empty prompt)",
      answerPreview: next ? truncate(next.content, PREVIEW_LEN) : "",
    });
  });
  return anchors;
}

export function PromptRail({
  messages,
  onJump,
}: {
  messages: PromptRailMessage[];
  onJump: (messageId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const anchors = useMemo(() => buildPromptAnchors(messages), [messages]);

  if (anchors.length < 3) return null; // not worth it for a short thread

  return (
    <div className="relative flex-shrink-0">
      <button
        onClick={() => setOpen((v) => !v)}
        aria-label="Jump to prompt"
        className="p-1.5 rounded-md text-mute hover:text-ink hover:bg-surface-soft transition-colors"
      >
        <HugeiconsIcon icon={open ? Cancel01Icon : Menu01Icon} size={14} />
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 w-72 max-h-96 overflow-y-auto rounded-xl border border-hairline bg-canvas shadow-lg z-50 py-1.5">
          {anchors.map((a) => (
            <button
              key={a.id}
              onClick={() => {
                onJump(a.id);
                setOpen(false);
              }}
              className="w-full text-left px-3 py-2 hover:bg-surface-soft transition-colors"
            >
              <p className="text-xs font-semibold text-ink truncate">{a.preview}</p>
              {a.answerPreview && <p className="text-[11px] text-mute truncate mt-0.5">{a.answerPreview}</p>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
