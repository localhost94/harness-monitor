import type { HarnessId } from "../types";

/**
 * The settings the *pipeline* reads, mirrored from `src-tauri/src/settings.rs`.
 *
 * Deliberately not the whole preference set. Theme, widget shape and the
 * starting tab are read synchronously while the store is created, because a
 * floating widget is on screen before any IPC has resolved and getting that
 * wrong shows a flash of the wrong theme. They stay in localStorage; the
 * settings popover edits them there. The rule is whether the *first render* or
 * the *running pipeline* needs the value.
 *
 * Every name here matches a serde field, and serde's `rename_all = "kebab-case"`
 * on the enums means the wire values are kebab-case too.
 */
export interface Settings {
  muted: boolean;
  suppress_when_focused: boolean;
  interval_ms: number;
  max_ended: number;
  enabled: HarnessId[];
  rerun_target: LaunchTarget;
  rerun_focus: boolean;
  rerun_timeout_ms: number;
}

export type LaunchTarget = "auto" | "herdr" | "terminal";

/** What the backend sends back, including the two answers that are not settings. */
export interface SettingsView {
  settings: Settings;
  /**
   * False on a fresh install. Not used for migration any more - the appearance
   * preferences that needed migrating never left localStorage - but it is the
   * only way the UI can tell "you have never set anything" from "you set
   * everything back to the default", and the popover says so.
   */
  loaded_from_file: boolean;
  /** True when the scan settings differ from what the running scanner started with. */
  needs_restart: boolean;
  /** Set when the file could not be written; the values are still returned. */
  error: string | null;
}

export const DEFAULT_SETTINGS: Settings = {
  muted: false,
  suppress_when_focused: true,
  interval_ms: 1_500,
  max_ended: 100,
  enabled: ["claude-code", "open-code", "codex", "gemini", "antigravity"],
  rerun_target: "auto",
  rerun_focus: true,
  rerun_timeout_ms: 30_000,
};

export const ALL_HARNESSES: HarnessId[] = [
  "claude-code",
  "open-code",
  "codex",
  "gemini",
  "antigravity",
];

/** Clamped to the same bounds `settings.rs` clamps to, so the UI cannot offer a
 *  value the backend will silently change. */
export const INTERVAL_CHOICES: { value: number; label: string }[] = [
  { value: 1_000, label: "1s" },
  { value: 1_500, label: "1.5s" },
  { value: 3_000, label: "3s" },
  { value: 5_000, label: "5s" },
];

export const ENDED_CHOICES: { value: number; label: string }[] = [
  { value: 25, label: "25" },
  { value: 50, label: "50" },
  { value: 100, label: "100" },
  { value: 250, label: "250" },
];

export const TIMEOUT_CHOICES: { value: number; label: string }[] = [
  { value: 10_000, label: "10s" },
  { value: 30_000, label: "30s" },
  { value: 60_000, label: "1m" },
  { value: 120_000, label: "2m" },
];

export const TARGET_CHOICES: { value: LaunchTarget; label: string; note: string }[] = [
  { value: "auto", label: "auto", note: "a herdr pane if herdr is installed" },
  { value: "herdr", label: "herdr", note: "fail rather than open a plain window" },
  { value: "terminal", label: "terminal", note: "always a new terminal window" },
];

/**
 * The scan settings, and only those.
 *
 * Mirrors `Settings::needs_restart` in Rust. The UI uses it to decide whether to
 * show the "applies on restart" note; the backend uses its own copy to answer
 * the same question authoritatively, because a note that can be wrong is worse
 * than no note.
 */
export function needsRestart(applied: Settings, edited: Settings): boolean {
  return (
    applied.interval_ms !== edited.interval_ms ||
    applied.max_ended !== edited.max_ended ||
    !sameHarnesses(applied.enabled, edited.enabled)
  );
}

function sameHarnesses(a: HarnessId[], b: HarnessId[]): boolean {
  if (a.length !== b.length) return false;
  const set = new Set(a);
  return b.every((h) => set.has(h));
}

/** Add or remove one harness, always in `ALL_HARNESSES` order. */
export function toggleHarness(enabled: HarnessId[], harness: HarnessId): HarnessId[] {
  const next = enabled.includes(harness)
    ? enabled.filter((h) => h !== harness)
    : [...enabled, harness];
  // An empty list would hide every session, which reads as a broken app rather
  // than a choice. The backend enforces this too; doing it here as well means
  // the toggle cannot show a state the backend will refuse.
  if (next.length === 0) return [...enabled];
  // Normalised on the way out as well as in, so removing a harness reorders a
  // list that had been shuffled rather than half-fixing it.
  return ALL_HARNESSES.filter((h) => next.includes(h));
}
