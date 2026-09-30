export type HarnessId =
  | "claude-code"
  | "open-code"
  | "codex"
  | "gemini"
  | "antigravity";

export type SessionState =
  | "running"
  | "awaiting-input"
  | "awaiting-permission"
  | "idle"
  | "shell"
  | "active-unknown"
  /** Over: the process is gone, or the row fell outside its recency window. */
  | "ended";

/**
 * Whether the process behind a session is still running.
 *
 * `unknown` is not a synonym for alive - on a host with no procfs nothing can
 * be checked, and such a session is listed as ended rather than shown as live,
 * because "cannot be disproved" is not evidence.
 */
export type Liveness = "alive" | "dead" | "unknown";

export type FidelityTier = "full" | "usage-only" | "presence-only";

export interface SessionKey {
  harness: HarnessId;
  pid_domain: string;
  pid: number;
  proc_start: string;
}

export interface TokenCounts {
  input: number;
  output: number;
  reasoning: number;
  cache_read: number;
  cache_write: number;
}

export interface AgentSession {
  key: SessionKey;
  session_id: string;
  cwd: string;
  name: string | null;
  state: SessionState;
  state_changed_at: number;
  started_at: number;
  waiting_for: string | null;
  model: string | null;
  tokens: TokenCounts | null;
  cost: number | null;
  is_background: boolean;
  tier: FidelityTier;
  /** herdr pane id, when something on this host knows where the session lives. */
  jump_target: string | null;
  /** Terminal tab title - the fastest way for a human to recognise the window. */
  terminal_title: string | null;
  liveness: Liveness;
}

export interface QuotaSnapshot {
  at: number;
  source: string;
  tier: string | null;
  five_hour_pct: number | null;
  five_hour_resets_at: string | null;
  seven_day_pct: number | null;
  seven_day_resets_at: string | null;
}

export interface Snapshot {
  taken_at: number;
  detected: HarnessId[];
  /** Confirmed-alive sessions, plus any whose liveness this host cannot check. */
  sessions: AgentSession[];
  /**
   * Sessions this machine can see that are no longer running. Display-only:
   * the backend never feeds these to the differ, so nothing here can notify.
   */
  ended: AgentSession[];
  quota: QuotaSnapshot | null;
  reseed: boolean;
}

/// Two-letter code shown on every row and in the collapsed pill, so a glance
/// tells you WHICH agent is waiting, not just that one is.
export const HARNESS_CODE: Record<HarnessId, string> = {
  "claude-code": "CC",
  "open-code": "OC",
  codex: "CX",
  gemini: "GM",
  antigravity: "AG",
};

/// The interface has no hue left to spend, so the two letters are the whole
/// of a harness's identity: one chip style for all of them, and the code is
/// what tells you which agent you are looking at.
export const HARNESS_CHIP =
  "bg-black/[0.06] text-black/80 ring-1 ring-inset ring-black/15 dark:bg-white/[0.08] dark:text-white/85 dark:ring-white/20";

/** The same chip, struck solid: this harness has something waiting on you. */
export const HARNESS_CHIP_ALERT =
  "bg-black text-white ring-1 ring-inset ring-black dark:bg-white dark:text-black dark:ring-white";

/** A shell-less mark in the pill: filled while it has sessions, hollow while quiet. */
export const HARNESS_DOT_ACTIVE = "bg-black dark:bg-white";
export const HARNESS_DOT_QUIET = "border border-black/30 dark:border-white/30";

export const HARNESS_TEXT = "text-black/75 dark:text-white/75";

export const HARNESS_LABEL: Record<HarnessId, string> = {
  "claude-code": "Claude Code",
  "open-code": "opencode",
  codex: "codex",
  gemini: "gemini-cli",
  antigravity: "antigravity",
};

/** Past this the quota reading is greyed out rather than extrapolated. */
export const QUOTA_STALE_MS = 15 * 60 * 1000;
