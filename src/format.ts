import type { Provider, UsageWindow } from "./types";

export const nowSec = () => Date.now() / 1000;

export type Level = "ok" | "warn" | "crit";

export function level(pct: number): Level {
  if (pct >= 90) return "crit";
  if (pct >= 75) return "warn";
  return "ok";
}

export const clampPct = (p: number) => Math.max(0, Math.min(100, p));

/** A window whose reset time has passed: we know it restarted, not what's been used since. */
export const isReset = (w: UsageWindow) => w.resetsAt !== null && w.resetsAt <= nowSec();

export function sessionWindow(p: Provider): UsageWindow | undefined {
  return p.windows.find((w) => w.id === "session") ?? p.windows[0];
}

export function weeklyWindow(p: Provider): UsageWindow | undefined {
  const s = sessionWindow(p);
  return p.windows.find((w) => w.id === "weekly" && w !== s) ?? p.windows.find((w) => w !== s);
}

/** "1h 23m", "2d 4h", "12m", "<1m". */
export function countdown(resetsAt: number | null): string {
  if (resetsAt === null) return "—";
  const s = Math.round(resetsAt - nowSec());
  if (s <= 0) return "Reset";
  if (s < 60) return "<1m";
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

const timeFmt = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" });

export function resetClock(resetsAt: number | null): string {
  if (resetsAt === null) return "";
  const ms = resetsAt * 1000;
  return ms - Date.now() < 20 * 3600 * 1000 ? timeFmt.format(ms) : dayFmt.format(ms);
}

export function ago(ms: number | null): string {
  if (ms === null) return "never";
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

/** Render `code` spans from provider messages (e.g. `codex login`). */
export function richText(s: string): string {
  return escapeHtml(s).replace(/`([^`]+)`/g, "<code>$1</code>");
}

/** Reset time without the AM/PM marker, e.g. "11:30" — Duo's big clock. */
export function clockShort(resetsAt: number | null): string {
  if (resetsAt === null) return "—";
  return timeFmt
    .formatToParts(resetsAt * 1000)
    .filter((p) => p.type === "hour" || p.type === "minute" || p.type === "literal")
    .map((p) => p.value)
    .join("")
    .trim();
}

/** 1234 -> "1.2K", 98_765_432 -> "98.8M". */
export function compactTokens(n: number): string {
  if (n < 1000) return String(n);
  const units: [number, string][] = [
    [1e9, "B"],
    [1e6, "M"],
    [1e3, "K"],
  ];
  for (const [v, u] of units) {
    if (n >= v) {
      const x = n / v;
      return `${x >= 100 ? Math.round(x) : x.toFixed(1)}${u}`;
    }
  }
  return String(n);
}
