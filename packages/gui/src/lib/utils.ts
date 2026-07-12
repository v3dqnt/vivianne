import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** Compact relative time for list rows: "5m", "1h", "20h", "2d" — falls back to a short date past 7 days. */
export function formatRelativeTime(iso: string | undefined | null): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const diffMs = Date.now() - date.getTime();
  const minutes = Math.floor(diffMs / 60000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Groups a list of items with a date string into "Today" / "Yesterday" / weekday / "Mon D, YYYY" buckets, most recent first. */
export function groupByRelativeDate<T>(items: T[], getDate: (item: T) => string | undefined | null): Array<{ label: string; items: T[] }> {
  const now = new Date();
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();

  const labelFor = (iso: string | undefined | null): string => {
    if (!iso) return "Earlier";
    const date = new Date(iso);
    if (Number.isNaN(date.getTime())) return "Earlier";
    const startOfDay = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
    const dayDiff = Math.round((startOfToday - startOfDay) / DAY_MS);
    if (dayDiff <= 0) return "Today";
    if (dayDiff === 1) return "Yesterday";
    if (dayDiff < 7) return date.toLocaleDateString(undefined, { weekday: "long" });
    return date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
  };

  const groups = new Map<string, T[]>();
  for (const item of items) {
    const label = labelFor(getDate(item));
    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(item);
  }
  return Array.from(groups.entries()).map(([label, items]) => ({ label, items }));
}
