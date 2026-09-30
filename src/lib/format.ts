import type { AgentSession, SessionState, TokenCounts } from "../types";

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

/**
 * Same idea as `duration`, but for history: goes on to days and months, and
 * rounds rather than counting seconds. "3d" is what you want to know about a
 * session from last week - "72h 4m" is the same fact in a form nobody reads.
 */
export function ago(sinceMs: number, now: number): string {
  const mins = Math.max(0, Math.floor((now - sinceMs) / 60_000));
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return months < 12 ? `${months}mo ago` : `${Math.floor(months / 12)}y ago`;
}

/**
 * The absolute time behind a relative one, for a tooltip: "3 Sep 14:05".
 *
 * A history row says "2d ago", which is unreadable as a date and imprecise
 * across a month boundary - this is the anchor it needs. The year appears only
 * when it is not the current one, which is the case that actually confuses.
 */
export function stamp(atMs: number, now: number): string {
  const at = new Date(atMs);
  if (Number.isNaN(at.getTime())) return "";
  const sameYear = at.getFullYear() === new Date(now).getFullYear();
  return at.toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    ...(sameYear ? {} : { year: "numeric" }),
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

/**
 * Wall-clock time of a reset, in the viewer's own timezone: "13:00".
 *
 * A countdown answers "how long", a clock answers "when I can start again",
 * and the second one is what you plan around.
 */
export function clockTime(iso: string | null): string | null {
  if (!iso) return null;
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return null;
  return at.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false });
}

/** Same, plus the weekday - for a reset that is days out, the time alone is ambiguous. */
export function dayAndTime(iso: string | null): string | null {
  if (!iso) return null;
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return null;
  const sameDay = at.toDateString() === new Date().toDateString();
  const time = at.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
  if (sameDay) return time;
  return `${at.toLocaleDateString(undefined, { weekday: "short" })} ${time}`;
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

/** Compact token count: 1.2M, 953k, 412. */
export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1)}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
  return `${n}`;
}

export function formatCost(n: number): string {
  return n >= 10 ? `$${n.toFixed(0)}` : `$${n.toFixed(2)}`;
}

/**
 * The headline token figure is input + output: cache reads run into the
 * hundreds of millions on a long Claude Code session and would swamp
 * everything else in a chip this size. The full breakdown goes in the tooltip.
 */
export function tokenHeadline(t: TokenCounts): number {
  return t.input + t.output + t.reasoning;
}

export function tokenBreakdown(t: TokenCounts): string {
  const parts = [
    `in ${t.input.toLocaleString()}`,
    `out ${t.output.toLocaleString()}`,
  ];
  if (t.reasoning) parts.push(`reasoning ${t.reasoning.toLocaleString()}`);
  if (t.cache_read) parts.push(`cache read ${t.cache_read.toLocaleString()}`);
  if (t.cache_write) parts.push(`cache write ${t.cache_write.toLocaleString()}`);
  return parts.join(" · ");
}

export const STATE_LABEL: Record<SessionState, string> = {
  running: "running",
  "awaiting-input": "needs you",
  "awaiting-permission": "needs approval",
  idle: "idle",
  shell: "shell",
  "active-unknown": "active",
  ended: "ended",
};

/**
 * Chips are ranked, not coloured: a request for approval is a solid stamp of
 * ink, a request for input is a hollow one, work in progress is a hairline
 * outline, and anything passive is a ghost. Reading down a column of chips
 * tells you the urgency without decoding a single hue.
 */
export const STATE_STYLE: Record<SessionState, string> = {
  running:
    "bg-black/[0.05] text-black/80 ring-1 ring-inset ring-black/30 dark:bg-white/[0.06] dark:text-white/80 dark:ring-white/25",
  "awaiting-input":
    "bg-transparent text-black ring-2 ring-black dark:text-white dark:ring-white",
  "awaiting-permission":
    "bg-black text-white ring-2 ring-black dark:bg-white dark:text-black",
  idle: "bg-transparent text-black/45 ring-1 ring-inset ring-black/20 dark:text-white/45 dark:ring-white/20",
  shell:
    "bg-transparent text-black/60 ring-1 ring-inset ring-black/20 dark:text-white/60 dark:ring-white/20",
  "active-unknown":
    "bg-transparent text-black/50 ring-1 ring-inset ring-black/15 dark:text-white/50 dark:ring-white/15",
  ended: "bg-transparent text-black/35 ring-1 ring-inset ring-black/10 dark:text-white/35 dark:ring-white/10",
};

/**
 * Row background + left rule. Rows that want something are printed darker;
 * idle rows recede to paper. A permission row also carries the caution
 * hatch, the one texture in the list, so the state you must not miss is the
 * state that does not look like every other block.
 *
 * `ended` sits below `idle` rather than beside it: an idle session is a process
 * that is still there and quietly waiting, so it keeps a rule. An ended one is
 * a photograph, and gets neither ink nor a gradient.
 */
export const ROW_TINT: Record<SessionState, string> = {
  running:
    "bg-gradient-to-r from-black/[0.10] to-transparent border-l-2 border-black/55 dark:from-white/[0.10] dark:border-white/55",
  "awaiting-input":
    "bg-gradient-to-r from-black/[0.13] to-transparent border-l-2 border-black dark:from-white/[0.13] dark:border-white",
  "awaiting-permission":
    "hm-hatch bg-gradient-to-r from-black/[0.10] to-transparent border-l-2 border-black dark:from-white/[0.10] dark:border-white",
  idle: "border-l-2 border-transparent opacity-60",
  shell:
    "bg-gradient-to-r from-black/[0.06] to-transparent border-l-2 border-black/35 dark:from-white/[0.06] dark:border-white/35",
  "active-unknown":
    "border-l-2 border-black/30 opacity-80 dark:border-white/30",
  ended: "border-l-2 border-black/10 opacity-60 dark:border-white/10",
};
