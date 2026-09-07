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
  | "active-unknown";

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
  sessions: AgentSession[];
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

/// One hue per harness, deliberately outside the state palette (amber =
/// needs you, sky = running, zinc = idle) so the two never read as the same
/// signal.
export const HARNESS_CHIP: Record<HarnessId, string> = {
  "claude-code":
    "bg-orange-100 text-orange-700 ring-orange-300 dark:bg-orange-500/15 dark:text-orange-300 dark:ring-orange-400/30",
  "open-code":
    "bg-emerald-100 text-emerald-700 ring-emerald-300 dark:bg-emerald-500/15 dark:text-emerald-300 dark:ring-emerald-400/30",
  codex:
    "bg-violet-100 text-violet-700 ring-violet-300 dark:bg-violet-500/15 dark:text-violet-300 dark:ring-violet-400/30",
  gemini:
    "bg-blue-100 text-blue-700 ring-blue-300 dark:bg-blue-500/15 dark:text-blue-300 dark:ring-blue-400/30",
  antigravity:
    "bg-fuchsia-100 text-fuchsia-700 ring-fuchsia-300 dark:bg-fuchsia-500/15 dark:text-fuchsia-300 dark:ring-fuchsia-400/30",
};

/** Solid dot colour - used where the chip itself must stay translucent. */
export const HARNESS_DOT: Record<HarnessId, string> = {
  "claude-code": "bg-orange-400",
  "open-code": "bg-emerald-400",
  codex: "bg-violet-400",
  gemini: "bg-blue-400",
  antigravity: "bg-fuchsia-400",
};

export const HARNESS_TEXT: Record<HarnessId, string> = {
  "claude-code": "text-orange-700 dark:text-orange-300",
  "open-code": "text-emerald-700 dark:text-emerald-300",
  codex: "text-violet-700 dark:text-violet-300",
  gemini: "text-blue-700 dark:text-blue-300",
  antigravity: "text-fuchsia-700 dark:text-fuchsia-300",
};

export const HARNESS_LABEL: Record<HarnessId, string> = {
  "claude-code": "Claude Code",
  "open-code": "opencode",
  codex: "codex",
  gemini: "gemini-cli",
  antigravity: "antigravity",
};

/** Past this the quota reading is greyed out rather than extrapolated. */
export const QUOTA_STALE_MS = 15 * 60 * 1000;
