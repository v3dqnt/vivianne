"use client";

import React, { useState } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import { ArrowDown01Icon, ArrowRight01Icon, FlashIcon } from "@hugeicons/core-free-icons";

/**
 * Wraps a run of consecutive tool-call rows in a single collapsible
 * cluster — a lightweight grouping layer over the existing flat message
 * list, not a rewrite of tool-card rendering (each row inside still renders
 * exactly as it did before). Only default-collapsed once a burst gets long
 * enough (see `groupActivityRows`'s threshold) — a short 2-3 step burst
 * stays visibly expanded since collapsing it wouldn't save meaningful space.
 */
export function ActivityCluster({
  count,
  defaultCollapsed,
  children,
}: {
  count: number;
  defaultCollapsed: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(!defaultCollapsed);

  return (
    <div className="border border-hairline/60 rounded-2xl overflow-hidden bg-canvas/20">
      <button
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-2 px-3.5 py-2 text-xs font-medium text-body hover:bg-surface-soft/60 transition-colors"
      >
        <HugeiconsIcon icon={FlashIcon} size={12} className="text-mute" />
        <span>Agent activity — {count} steps</span>
        <span className="ml-auto text-mute">
          <HugeiconsIcon icon={open ? ArrowDown01Icon : ArrowRight01Icon} size={12} />
        </span>
      </button>
      {open && <div className="border-t border-hairline/60 divide-y divide-hairline/40">{children}</div>}
    </div>
  );
}

/** A run of consecutive tool rows is grouped once it's at least this long. */
const MIN_CLUSTER_SIZE = 2;
/** Clusters this size or larger start collapsed by default. */
const AUTO_COLLAPSE_THRESHOLD = 4;

export interface ActivityGroup<T> {
  kind: "single" | "cluster";
  items: T[];
}

/**
 * Group consecutive tool-call/tool-result rows (no user/assistant-text
 * message between them) into clusters. Non-tool rows always stay ungrouped
 * ("single"). Pure function, no React — easy to unit test independent of
 * rendering.
 */
export function groupActivityRows<T extends { role: string }>(rows: T[]): ActivityGroup<T>[] {
  const groups: ActivityGroup<T>[] = [];
  let run: T[] = [];

  const flushRun = () => {
    if (run.length === 0) return;
    if (run.length >= MIN_CLUSTER_SIZE) {
      groups.push({ kind: "cluster", items: run });
    } else {
      for (const item of run) groups.push({ kind: "single", items: [item] });
    }
    run = [];
  };

  for (const row of rows) {
    if (row.role === "toolResult" || row.role === "toolCall") {
      run.push(row);
    } else {
      flushRun();
      groups.push({ kind: "single", items: [row] });
    }
  }
  flushRun();

  return groups;
}

export function shouldCollapseCluster(count: number): boolean {
  return count >= AUTO_COLLAPSE_THRESHOLD;
}
