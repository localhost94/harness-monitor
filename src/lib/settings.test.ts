import { describe, expect, it } from "vitest";
import type { HarnessId } from "../types";
import {
  ALL_HARNESSES,
  DEFAULT_SETTINGS,
  ENDED_CHOICES,
  INTERVAL_CHOICES,
  TARGET_CHOICES,
  TIMEOUT_CHOICES,
  needsRestart,
  toggleHarness,
} from "./settings";

describe("defaults", () => {
  it("are the values the app had hardcoded before settings existed", () => {
    // These four were literals in Rust. If one drifts, the app silently
    // changes behaviour for everyone who never opens the popover.
    expect(DEFAULT_SETTINGS.interval_ms).toBe(1_500);
    expect(DEFAULT_SETTINGS.max_ended).toBe(100);
    expect(DEFAULT_SETTINGS.suppress_when_focused).toBe(true);
    expect(DEFAULT_SETTINGS.rerun_focus).toBe(true);
    expect(DEFAULT_SETTINGS.enabled).toEqual(ALL_HARNESSES);
  });

  it("mute starts off, because a widget that greets you shouting is the bug", () => {
    expect(DEFAULT_SETTINGS.muted).toBe(false);
  });
});

describe("the offered choices", () => {
  it("only offer values the backend will accept unclamped", () => {
    // `settings.rs` clamps interval to 500..60000, ended to 10..1000 and the
    // resume timeout to 3000..300000. Offering anything outside those would be
    // a control that saves a number the backend then changes.
    for (const { value } of INTERVAL_CHOICES) {
      expect(value, `interval ${value}`).toBeGreaterThanOrEqual(500);
      expect(value, `interval ${value}`).toBeLessThanOrEqual(60_000);
    }
    for (const { value } of ENDED_CHOICES) {
      expect(value, `ended ${value}`).toBeGreaterThanOrEqual(10);
      expect(value, `ended ${value}`).toBeLessThanOrEqual(1_000);
    }
    for (const { value } of TIMEOUT_CHOICES) {
      expect(value, `timeout ${value}`).toBeGreaterThanOrEqual(3_000);
      expect(value, `timeout ${value}`).toBeLessThanOrEqual(300_000);
    }
  });

  it("include the current default in each list, so it can be shown as active", () => {
    // A segmented control whose current value is not among the options renders
    // with nothing selected, which looks broken.
    expect(INTERVAL_CHOICES.map((c) => c.value)).toContain(DEFAULT_SETTINGS.interval_ms);
    expect(ENDED_CHOICES.map((c) => c.value)).toContain(DEFAULT_SETTINGS.max_ended);
    expect(TIMEOUT_CHOICES.map((c) => c.value)).toContain(DEFAULT_SETTINGS.rerun_timeout_ms);
    expect(TARGET_CHOICES.map((c) => c.value)).toContain(DEFAULT_SETTINGS.rerun_target);
  });

  it("cover every launch target the backend accepts", () => {
    expect(TARGET_CHOICES.map((c) => c.value).sort()).toEqual(["auto", "herdr", "terminal"]);
    for (const choice of TARGET_CHOICES) {
      expect(choice.note, choice.value).toBeTruthy();
    }
  });
});

describe("needsRestart", () => {
  const applied = DEFAULT_SETTINGS;

  it("ignores settings the running pipeline already reads", () => {
    // These apply on the next click, so promising a restart for them would send
    // people restarting for nothing.
    expect(
      needsRestart(applied, {
        ...applied,
        muted: true,
        suppress_when_focused: false,
        rerun_focus: false,
        rerun_timeout_ms: 60_000,
        rerun_target: "herdr",
      }),
    ).toBe(false);
  });

  it("flags the three the scanner was started with", () => {
    expect(needsRestart(applied, { ...applied, interval_ms: 3_000 })).toBe(true);
    expect(needsRestart(applied, { ...applied, max_ended: 250 })).toBe(true);
    expect(
      needsRestart(applied, { ...applied, enabled: ["claude-code", "open-code", "codex", "gemini"] }),
    ).toBe(true);
  });

  it("ignores the order the harnesses are listed in", () => {
    // The backend stores a set; reordering is not a change, and reporting one
    // would put a restart note on screen for nothing.
    const reordered = [...applied.enabled].reverse();
    expect(needsRestart(applied, { ...applied, enabled: reordered })).toBe(false);
  });
});

describe("toggleHarness", () => {
  it("removes a harness and puts it back", () => {
    const off = toggleHarness(DEFAULT_SETTINGS.enabled, "codex");
    expect(off).not.toContain("codex");
    expect(off).toHaveLength(4);
    expect(toggleHarness(off, "codex")).toEqual(DEFAULT_SETTINGS.enabled);
  });

  it("keeps a stable order, so two settings files do not differ by shuffling", () => {
    const shuffled = ["codex", "antigravity", "claude-code", "gemini", "open-code"] as HarnessId[];
    expect(toggleHarness(shuffled, "claude-code")).toEqual([
      "open-code",
      "codex",
      "gemini",
      "antigravity",
    ]);
  });

  it("refuses to switch off the last harness", () => {
    // Zero enabled harnesses hides every session, which reads as a broken app
    // rather than a choice. The backend enforces this too; doing it here means
    // the control cannot show a state the backend would refuse.
    let enabled: HarnessId[] = [...ALL_HARNESSES];
    for (const harness of ALL_HARNESSES) {
      const next = toggleHarness(enabled, harness);
      if (next.length === 0) break;
      enabled = next;
    }
    expect(enabled.length).toBeGreaterThan(0);
    expect(toggleHarness(["codex"], "codex")).toEqual(["codex"]);
  });
});
