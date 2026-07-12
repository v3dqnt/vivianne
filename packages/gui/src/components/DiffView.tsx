"use client";

import React from "react";
import { diffLines, type Change } from "diff";
import { cn } from "@/lib/utils";

/**
 * Real line-level diff (via `diffLines`), rendered as a unified-style view
 * with a +/- gutter — replaces the old two-static-`<pre>`-blocks approach
 * that just showed the whole old_string and whole new_string with no actual
 * diffing. `oldText === undefined` (no pre-image available, e.g. a brand
 * new file) renders as an all-green "added" diff, which is correct
 * behavior rather than a special case.
 */
export function DiffView({ oldText, newText }: { oldText?: string; newText: string }) {
  const changes: Change[] = diffLines(oldText ?? "", newText);

  let oldLineNo = 1;
  let newLineNo = 1;
  const rows: { kind: "add" | "remove" | "context"; text: string; oldNo?: number; newNo?: number }[] = [];

  for (const part of changes) {
    const lines = part.value.split("\n");
    // A trailing split empty string from the final newline isn't a real line.
    if (lines[lines.length - 1] === "") lines.pop();

    for (const line of lines) {
      if (part.added) {
        rows.push({ kind: "add", text: line, newNo: newLineNo++ });
      } else if (part.removed) {
        rows.push({ kind: "remove", text: line, oldNo: oldLineNo++ });
      } else {
        rows.push({ kind: "context", text: line, oldNo: oldLineNo++, newNo: newLineNo++ });
      }
    }
  }

  return (
    <div className="font-mono text-[11px] max-h-72 overflow-y-auto border-b border-hairline">
      {rows.map((row, i) => (
        <div
          key={i}
          className={cn(
            "flex",
            row.kind === "add" && "bg-emerald-50 text-emerald-800",
            row.kind === "remove" && "bg-rose-50 text-rose-800",
            row.kind === "context" && "text-body",
          )}
        >
          <span className="w-9 flex-shrink-0 text-right pr-2 text-mute/70 select-none tabular-nums">
            {row.oldNo ?? ""}
          </span>
          <span className="w-9 flex-shrink-0 text-right pr-2 text-mute/70 select-none tabular-nums">
            {row.newNo ?? ""}
          </span>
          <span className="w-4 flex-shrink-0 select-none">{row.kind === "add" ? "+" : row.kind === "remove" ? "-" : ""}</span>
          <span className="whitespace-pre-wrap flex-1 pr-3">{row.text}</span>
        </div>
      ))}
    </div>
  );
}
