import type { AgentSession, FidelityTier, SessionKey, SessionState, Snapshot } from "../types";

/**
 * A session builder, for tests only.
 *
 * The adapters' real output is a struct literal in Rust with sixteen fields,
 * and only four of them matter to any given test. Hand-writing the rest in
 * every test file is how a test ends up asserting against a field it forgot it
 * was asserting about.
 */

// `Omit` before `Partial`: intersecting them would leave `key` as the full
// `SessionKey`, and every test would have to spell out a pid it does not care
// about.
type Overrides = Omit<Partial<AgentSession>, "key"> & { key?: Partial<SessionKey> };

let nextPid = 41000;

export function session(overrides: Overrides = {}): AgentSession {
  const { key, ...rest } = overrides;
  return {
    key: {
      harness: "claude-code",
      pid_domain: "linux:test:pid:[1]",
      pid: (nextPid += 1),
      proc_start: "1000",
      ...key,
    },
    session_id: "90ba7df5-9c0c-4996-b592-6b86ae15339c",
    cwd: "/home/you/code/harness-monitor",
    name: "harness-monitor",
    state: "running" as SessionState,
    state_changed_at: 1_700_000_000_000,
    started_at: 1_699_999_000_000,
    waiting_for: null,
    model: "claude-opus-5",
    tokens: { input: 0, output: 0, reasoning: 0, cache_read: 0, cache_write: 0 },
    cost: null,
    is_background: false,
    tier: "full" as FidelityTier,
    jump_target: null,
    terminal_title: null,
    liveness: "alive",
    ...rest,
  };
}

export function quotaSnapshot(overrides: Partial<Snapshot["quota"]> = {}): NonNullable<Snapshot["quota"]> {
  return {
    at: 1_700_000_000_000,
    source: "statusline",
    tier: "default_claude_max_5x",
    five_hour_pct: 64,
    five_hour_resets_at: "2026-09-06T13:00:00Z",
    seven_day_pct: 34,
    seven_day_resets_at: "2026-09-09T02:00:00Z",
    ...overrides,
  };
}

export function snapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    taken_at: 1_700_000_000_000,
    detected: ["claude-code", "open-code", "codex", "gemini", "antigravity"],
    sessions: [],
    ended: [],
    quota: null,
    reseed: false,
    ...overrides,
  };
}

/** The `state` values, in the order the differ treats them as escalating. */
export const ALL_STATES: SessionState[] = [
  "running",
  "awaiting-input",
  "awaiting-permission",
  "idle",
  "shell",
  "active-unknown",
  "ended",
];

/** Deterministic base instant: a fixed UTC time, so nothing depends on "now". */
export const NOW = Date.UTC(2026, 8, 6, 14, 5, 0);
