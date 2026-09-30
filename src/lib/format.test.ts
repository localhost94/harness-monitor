import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ago,
  clockTime,
  dayAndTime,
  duration,
  formatCost,
  formatTokens,
  ROW_TINT,
  shortName,
  stamp,
  STATE_LABEL,
  STATE_STYLE,
  tokenBreakdown,
  tokenHeadline,
  untilReset,
} from "./format";
import { ALL_STATES, NOW, session } from "../test/fixtures";

describe("shortName", () => {
  it("prefers the harness-reported name", () => {
    expect(shortName(session({ name: "api-rewrite" }))).toBe("api-rewrite");
  });

  it("falls back to the last path segment", () => {
    expect(shortName(session({ name: null, cwd: "/home/you/code/harness-monitor" }))).toBe(
      "harness-monitor",
    );
  });

  it("splits Windows paths too", () => {
    // The Windows build reads the same session over a mounted profile.
    expect(shortName(session({ name: null, cwd: "C:\\Users\\you\\code\\widget" }))).toBe("widget");
  });

  it("ignores trailing and doubled separators", () => {
    expect(shortName(session({ name: null, cwd: "/home/you/code/widget/" }))).toBe("widget");
  });

  it("falls back to the session id when the path has no segments", () => {
    // antigravity rows carry nothing but an id, so this is its normal shape.
    expect(shortName(session({ name: null, cwd: "", session_id: "90ba7df5-extra" }))).toBe("90ba7df5");
  });
});

describe("duration", () => {
  const at = NOW;

  it("counts seconds under a minute", () => {
    expect(duration(at, at)).toBe("0s");
    expect(duration(at - 59_000, at)).toBe("59s");
  });

  it("switches to whole minutes", () => {
    expect(duration(at - 60_000, at)).toBe("1m");
    expect(duration(at - 59 * 60_000, at)).toBe("59m");
  });

  it("switches to hours and minutes", () => {
    expect(duration(at - 3_600_000, at)).toBe("1h 0m");
    expect(duration(at - 90 * 60_000, at)).toBe("1h 30m");
  });

  it("clamps a future timestamp to zero rather than rendering a negative span", () => {
    // Only reachable with a clock skew, but "-3s" on screen is not an option.
    expect(duration(at + 3_000, at)).toBe("0s");
  });
});

describe("ago", () => {
  const at = NOW;

  it("goes on to days and months, because history rows outlive an hour", () => {
    expect(ago(at, at)).toBe("0m ago");
    expect(ago(at - 59 * 60_000, at)).toBe("59m ago");
    expect(ago(at - 3_600_000, at)).toBe("1h ago");
    expect(ago(at - 23 * 3_600_000, at)).toBe("23h ago");
    expect(ago(at - 24 * 3_600_000, at)).toBe("1d ago");
    expect(ago(at - 29 * 86_400_000, at)).toBe("29d ago");
    expect(ago(at - 30 * 86_400_000, at)).toBe("1mo ago");
    expect(ago(at - 360 * 86_400_000, at)).toBe("1y ago");
  });

  it("rounds down at every boundary", () => {
    // A minute short of a month is 29 days, not a truncated "1mo": the unit a
    // human reads must never claim to be the next one up.
    expect(ago(at - 30 * 86_400_000 + 60_000, at)).toBe("29d ago");
    expect(ago(at - 330 * 86_400_000, at)).toBe("11mo ago");
  });

  it("never prints a 12-month row - it is a year", () => {
    // Quirk of flooring the month unit: 12 months is exactly one year, so the
    // "12mo ago" string is unreachable. Pinned so a rewrite notices.
    expect(ago(at - 360 * 86_400_000, at)).toBe("1y ago");
    expect(ago(at - 365 * 86_400_000, at)).toBe("1y ago");
  });

  it("clamps a future timestamp", () => {
    expect(ago(at + 60_000, at)).toBe("0m ago");
  });
});

describe("stamp", () => {
  it("renders the absolute time behind a relative one", () => {
    // Asserted by component, not by whole string: the exact ordering and
    // punctuation of toLocaleString is the locale's business, and an ICU
    // upgrade is not a regression in this app.
    const out = stamp(NOW, NOW);
    expect(out).toContain("14:05");
    expect(out).toContain("Sep");
  });

  it("adds the year only when it is not the current one", () => {
    // The year is the case that actually confuses, so it appears only then.
    expect(stamp(NOW, NOW)).not.toContain("2026");
    expect(stamp(Date.UTC(2025, 0, 2, 3, 4), NOW)).toContain("2025");
  });

  it("returns nothing for an unparseable stamp rather than 'Invalid Date'", () => {
    expect(stamp(Number.NaN, NOW)).toBe("");
  });
});

describe("clockTime", () => {
  it("answers when, not how long", () => {
    expect(clockTime("2026-09-06T13:00:00Z")).toBe("13:00");
  });

  it("passes through the absent and the unparseable", () => {
    expect(clockTime(null)).toBeNull();
    expect(clockTime("")).toBeNull();
    expect(clockTime("not a date")).toBeNull();
  });
});

describe("dayAndTime", () => {
  // Unlike `stamp`, this one has no `now` parameter - it reads the wall clock
  // to decide whether to print a weekday. So the clock has to be pinned, or
  // the same-day case only passes on the day it was written.
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });

  it("drops the weekday when the reset lands today", () => {
    const today = new Date(NOW + 6 * 3_600_000).toISOString();
    expect(dayAndTime(today)).toBe("20:05");
  });

  it("prefixes the weekday when it lands another day", () => {
    // A reset days out is ambiguous as a bare time, which is the whole point.
    const wednesday = new Date(NOW + 3 * 86_400_000).toISOString();
    expect(dayAndTime(wednesday)).toBe("Wed 14:05");
  });

  it("passes through the absent and the unparseable", () => {
    expect(dayAndTime(null)).toBeNull();
    expect(dayAndTime("nonsense")).toBeNull();
  });
});

describe("untilReset", () => {
  it("counts down to a future reset", () => {
    expect(untilReset(new Date(NOW + 90 * 60_000).toISOString(), NOW)).toBe("1h 30m");
  });

  it("says so once the window is spent, and never extrapolates past it", () => {
    const past = new Date(NOW - 1).toISOString();
    expect(untilReset(past, NOW)).toBe("resetting");
    expect(untilReset(new Date(NOW).toISOString(), NOW)).toBe("resetting");
  });

  it("passes through the absent and the unparseable", () => {
    expect(untilReset(null, NOW)).toBeNull();
    expect(untilReset("later", NOW)).toBeNull();
  });
});

describe("formatTokens", () => {
  it("prints small counts verbatim", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(412)).toBe("412");
  });

  it("abbreviates thousands", () => {
    expect(formatTokens(1_000)).toBe("1k");
    expect(formatTokens(953_000)).toBe("953k");
  });

  it("carries one decimal into the millions, where it still matters", () => {
    expect(formatTokens(1_000_000)).toBe("1.0M");
    expect(formatTokens(9_999_999)).toBe("10.0M");
  });

  it("drops the decimal past ten million, where it is noise", () => {
    expect(formatTokens(10_000_000)).toBe("10M");
    expect(formatTokens(242_058_096)).toBe("242M");
  });

  it("rounds the thousands axis", () => {
    expect(formatTokens(1_499)).toBe("1k");
    expect(formatTokens(1_500)).toBe("2k");
  });
});

describe("formatCost", () => {
  it("keeps cents below ten, where they are the interesting part", () => {
    expect(formatCost(0)).toBe("$0.00");
    expect(formatCost(1.239)).toBe("$1.24");
    expect(formatCost(9.999)).toBe("$10.00");
  });

  it("drops the cents from ten up", () => {
    expect(formatCost(10)).toBe("$10");
    expect(formatCost(1234.5)).toBe("$1235");
  });
});

describe("tokenHeadline", () => {
  it("is input plus output plus reasoning, and nothing else", () => {
    const tokens = {
      input: 1_386,
      output: 951_739,
      reasoning: 12_000,
      cache_read: 242_058_096,
      cache_write: 51_204,
    };
    expect(tokenHeadline(tokens)).toBe(965_125);
  });
});

describe("tokenBreakdown", () => {
  it("names the fields that have counts, and only those", () => {
    const base = { input: 0, output: 0, reasoning: 0, cache_read: 0, cache_write: 0 };
    // The grouping separator is the locale's business, so match the shape.
    expect(tokenBreakdown({ ...base, input: 1_386, output: 951_739 })).toMatch(
      /^in [\d,  ]+ · out [\d,  ]+$/,
    );
  });

  it("adds reasoning and cache when they are non-zero", () => {
    const parts = tokenBreakdown({
      input: 0,
      output: 0,
      reasoning: 7,
      cache_read: 8,
      cache_write: 9,
    });
    expect(parts).toBe("in 0 · out 0 · reasoning 7 · cache read 8 · cache write 9");
  });

  it("omits reasoning and cache entirely when they are zero", () => {
    // A tooltip listing five zeroes is noise; only real counters earn a label.
    const parts = tokenBreakdown({ input: 0, output: 0, reasoning: 0, cache_read: 0, cache_write: 0 });
    expect(parts).not.toContain("reasoning");
    expect(parts).not.toContain("cache");
  });
});

describe("state lookup tables", () => {
  it("cover every state the backend can send", () => {
    // The Rust enum is `#[serde(rename_all = "kebab-case")]`; a state added
    // there and not here renders as undefined, so the keys are pinned.
    expect(Object.keys(STATE_LABEL).sort()).toEqual([...ALL_STATES].sort());
    expect(Object.keys(STATE_STYLE).sort()).toEqual([...ALL_STATES].sort());
    expect(Object.keys(ROW_TINT).sort()).toEqual([...ALL_STATES].sort());
  });

  it("give every state a non-empty label and style", () => {
    for (const state of ALL_STATES) {
      expect(STATE_LABEL[state], `label for ${state}`).toBeTruthy();
      expect(STATE_STYLE[state], `style for ${state}`).toBeTruthy();
      expect(ROW_TINT[state], `tint for ${state}`).toBeTruthy();
    }
  });

  it("keeps the one attention state the only hatched row", () => {
    // The caution hatch is the single texture in the list; anything else using
    // it would spend the one signal the layout has.
    const hatched = ALL_STATES.filter((s) => ROW_TINT[s].includes("hm-hatch"));
    expect(hatched).toEqual(["awaiting-permission"]);
  });

  it("says ended, not idle, for a dead process", () => {
    // A row frozen at busy for months is not calmly waiting for anyone.
    expect(STATE_LABEL.ended).toBe("ended");
    expect(STATE_LABEL.idle).toBe("idle");
  });
});
