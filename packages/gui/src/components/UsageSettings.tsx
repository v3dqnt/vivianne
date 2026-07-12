"use client";

import React, { useEffect, useMemo, useState } from "react";
import { useAgent, type UsageDay } from "./AgentProvider";

const DAY_MS = 24 * 60 * 60 * 1000;
const WEEKS = 26;

function formatDate(d: Date) {
  return d.toISOString().slice(0, 10);
}

function intensityClass(total: number, max: number) {
  if (total <= 0) return "bg-surface-soft";
  const ratio = max > 0 ? total / max : 0;
  if (ratio > 0.75) return "bg-ink";
  if (ratio > 0.5) return "bg-ink/70";
  if (ratio > 0.25) return "bg-ink/45";
  return "bg-ink/20";
}

export function UsageSettings() {
  const { getUsageHistory } = useAgent();
  const [history, setHistory] = useState<Record<string, UsageDay>>({});
  const [loading, setLoading] = useState(true);
  const [hovered, setHovered] = useState<{ date: string; day: UsageDay } | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    getUsageHistory().then((h) => {
      if (active) {
        setHistory(h);
        setLoading(false);
      }
    });
    return () => {
      active = false;
    };
  }, [getUsageHistory]);

  const { cells, maxTotal, totals } = useMemo(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const days = WEEKS * 7;
    const start = new Date(today.getTime() - (days - 1) * DAY_MS);
    // Align start to a Sunday so weeks form clean columns.
    start.setDate(start.getDate() - start.getDay());

    const list: { date: string; day: UsageDay | null; column: number; row: number }[] = [];
    let max = 0;
    let sumInput = 0;
    let sumOutput = 0;
    let sumCost = 0;
    let sumRequests = 0;

    for (let i = 0; i < WEEKS * 7; i++) {
      const d = new Date(start.getTime() + i * DAY_MS);
      const key = formatDate(d);
      const day = history[key] ?? null;
      if (day) {
        const total = day.input + day.output;
        max = Math.max(max, total);
        sumInput += day.input;
        sumOutput += day.output;
        sumCost += day.cost;
        sumRequests += day.requests;
      }
      list.push({ date: key, day, column: Math.floor(i / 7), row: i % 7 });
    }
    return { cells: list, maxTotal: max, totals: { input: sumInput, output: sumOutput, cost: sumCost, requests: sumRequests } };
  }, [history]);

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap gap-3">
        {[
          { label: "Requests", value: totals.requests.toLocaleString() },
          { label: "Input tokens", value: totals.input.toLocaleString() },
          { label: "Output tokens", value: totals.output.toLocaleString() },
          { label: "Est. cost", value: `$${totals.cost.toFixed(2)}` },
        ].map((stat) => (
          <div key={stat.label} className="flex-1 min-w-[140px] p-4 border border-hairline/60 bg-canvas/30 backdrop-blur-md rounded-2xl">
            <p className="text-[10px] font-bold uppercase tracking-wider text-mute mb-1">{stat.label}</p>
            <p className="text-lg font-bold text-ink font-mono">{stat.value}</p>
          </div>
        ))}
      </div>

      <div className="p-5 border border-hairline/60 bg-canvas/30 backdrop-blur-md rounded-2xl overflow-x-auto">
        {loading ? (
          <p className="text-xs text-mute font-sans">Loading…</p>
        ) : (
          <div className="relative inline-block">
            <div className="grid grid-flow-col gap-[3px]" style={{ gridTemplateRows: "repeat(7, 11px)" }}>
              {cells.map((cell) => (
                <div
                  key={cell.date}
                  onMouseEnter={() => cell.day && setHovered({ date: cell.date, day: cell.day })}
                  onMouseLeave={() => setHovered(null)}
                  className={`w-[11px] h-[11px] rounded-[2px] ${intensityClass(cell.day ? cell.day.input + cell.day.output : 0, maxTotal)}`}
                  title={cell.date}
                />
              ))}
            </div>
            {hovered && (
              <div className="mt-3 text-xs font-sans text-body">
                <span className="font-semibold text-ink">{hovered.date}</span> — {hovered.day.requests} request
                {hovered.day.requests === 1 ? "" : "s"}, {(hovered.day.input + hovered.day.output).toLocaleString()} tokens, $
                {hovered.day.cost.toFixed(4)}
              </div>
            )}
          </div>
        )}
        {!loading && maxTotal === 0 && (
          <p className="text-xs text-mute font-sans mt-3">No usage recorded yet — this fills in as you use real provider models.</p>
        )}
      </div>
    </div>
  );
}
