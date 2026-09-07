import type { AgentSession, SessionState } from "../types";

export function shortName(session: AgentSession): string {
  if (session.name) return session.name;
  const parts = session.cwd.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? session.session_id.slice(0, 8);
}

export function duration(sinceMs: number, now: number): string {
  const secs = Math.max(0, Math.floor((now - sinceMs) / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  return `${hours}h ${mins % 60}m`;
}

/** Time until an absolute ISO reset stamp. Never extrapolated past it. */
export function untilReset(iso: string | null, now: number): string | null {
  if (!iso) return null;
  const target = Date.parse(iso);
  if (Number.isNaN(target)) return null;
  const left = target - now;
  if (left <= 0) return "resetting";
  return duration(now - left, now);
}

export const STATE_LABEL: Record<SessionState, string> = {
  running: "running",
  "awaiting-input": "needs you",
  "awaiting-permission": "needs approval",
  idle: "idle",
  shell: "shell",
  "active-unknown": "active",
};

export const STATE_STYLE: Record<SessionState, string> = {
  running: "bg-sky-100 text-sky-700 ring-sky-300 dark:bg-sky-500/15 dark:text-sky-300 dark:ring-sky-400/30",
  "awaiting-input":
    "bg-amber-200 text-amber-900 ring-amber-400 dark:bg-amber-500/20 dark:text-amber-200 dark:ring-amber-400/40",
  "awaiting-permission":
    "bg-orange-200 text-orange-900 ring-orange-400 dark:bg-orange-500/20 dark:text-orange-200 dark:ring-orange-400/40",
  idle: "bg-zinc-100 text-zinc-600 ring-zinc-300 dark:bg-zinc-500/15 dark:text-zinc-300 dark:ring-zinc-400/25",
  shell:
    "bg-violet-100 text-violet-700 ring-violet-300 dark:bg-violet-500/15 dark:text-violet-300 dark:ring-violet-400/30",
  "active-unknown":
    "bg-zinc-100 text-zinc-500 ring-zinc-300 dark:bg-zinc-500/15 dark:text-zinc-400 dark:ring-zinc-400/20",
};

/**
 * Row background + left accent. Rows that want something are tinted; idle
 * rows recede. Scanning the list should not require reading it.
 */
export const ROW_TINT: Record<SessionState, string> = {
  running:
    "bg-gradient-to-r from-sky-500/20 via-sky-500/5 to-transparent border-l-2 border-sky-500 dark:from-sky-400/20 dark:via-sky-400/5 dark:border-sky-400",
  "awaiting-input":
    "bg-gradient-to-r from-amber-400/40 via-amber-400/10 to-transparent border-l-2 border-amber-500 dark:from-amber-400/25 dark:via-amber-400/8 dark:border-amber-400",
  "awaiting-permission":
    "bg-gradient-to-r from-orange-400/40 via-orange-400/10 to-transparent border-l-2 border-orange-500 dark:from-orange-400/25 dark:via-orange-400/8 dark:border-orange-400",
  idle: "bg-black/[0.03] border-l-2 border-transparent opacity-70 dark:bg-white/[0.03]",
  shell:
    "bg-gradient-to-r from-violet-500/20 to-transparent border-l-2 border-violet-500 dark:from-violet-400/20 dark:border-violet-400",
  "active-unknown":
    "bg-black/[0.03] border-l-2 border-zinc-400/60 opacity-80 dark:bg-white/[0.03] dark:border-zinc-500",
};

export const STATE_DOT: Record<SessionState, string> = {
  running: "bg-sky-500 dark:bg-sky-400",
  "awaiting-input": "bg-amber-500 dark:bg-amber-400",
  "awaiting-permission": "bg-orange-500 dark:bg-orange-400",
  idle: "bg-zinc-400 dark:bg-zinc-500",
  shell: "bg-violet-500 dark:bg-violet-400",
  "active-unknown": "bg-zinc-400 dark:bg-zinc-500",
};
