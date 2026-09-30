import { describe, expect, it } from "vitest";
import { filterSessions, haystack, matchesQuery, windowStart, withinWindow } from "./search";
import { NOW, session } from "../test/fixtures";

describe("haystack", () => {
  it("lowercases, so a search cannot miss on case", () => {
    expect(haystack(session({ name: "API-Rewrite" }))).toContain("api-rewrite");
  });

  it("carries both the harness label and its two-letter code", () => {
    // "claude" has to find it by name, "cc" by eye-read code.
    const hay = haystack(session({ key: { harness: "claude-code" } }));
    expect(hay).toContain("claude code");
    expect(hay).toContain("cc");
  });

  it("carries every field a row is recognisable by", () => {
    const hay = haystack(
      session({
        name: "widget",
        cwd: "/home/you/code/widget",
        session_id: "abc123",
        model: "opus-5",
        terminal_title: "zsh",
      }),
    );
    for (const term of ["widget", "abc123", "opus-5", "zsh"]) {
      expect(hay, `missing ${term}`).toContain(term);
    }
  });

  it("skips absent fields instead of joining empty strings", () => {
    // A dropped field must not leave a gap: a double space would make a
    // two-word query match across the hole for no reason.
    const hay = haystack(
      session({ name: null, model: null, terminal_title: null, cwd: "/tmp" }),
    );
    expect(hay).not.toMatch(/\s\s/);
    expect(hay.trim()).toBe(hay);
    expect(hay).toContain("/tmp");
    expect(hay).toContain("claude code");
  });
});

describe("matchesQuery", () => {
  const s = session({
    name: "api-rewrite",
    cwd: "/home/you/code/harness-monitor",
    model: "claude-opus-5",
    key: { harness: "claude-code" },
  });

  it("matches everything on an empty or whitespace query", () => {
    expect(matchesQuery(s, "")).toBe(true);
    expect(matchesQuery(s, "   ")).toBe(true);
  });

  it("matches a single term anywhere in the haystack", () => {
    expect(matchesQuery(s, "rewrite")).toBe(true);
    expect(matchesQuery(s, "harness-monitor")).toBe(true);
    expect(matchesQuery(s, "opus")).toBe(true);
  });

  it("narrows rather than widens as terms are added", () => {
    // The whole reason AND-semantics exists: a second word that returns *more*
    // rows is worse than no search at all.
    expect(matchesQuery(s, "api rewrite")).toBe(true);
    expect(matchesQuery(s, "api nonexistent")).toBe(false);
  });

  it("ignores case and repeated whitespace between terms", () => {
    expect(matchesQuery(s, "  API    REWRITE  ")).toBe(true);
  });

  it("finds a session that has nothing but an id", () => {
    // antigravity exposes no name, no path and no model, so the id is the only
    // handle a user has on the row.
    const bare = session({ name: null, cwd: "", model: null, session_id: "9f2c-deadbeef" });
    expect(matchesQuery(bare, "deadbeef")).toBe(true);
    expect(matchesQuery(bare, "9f2c")).toBe(true);
  });

  it("finds a session by its harness", () => {
    expect(matchesQuery(s, "claude code")).toBe(true);
    expect(matchesQuery(s, "codex")).toBe(false);
  });
});

describe("windowStart", () => {
  it("buckets today on local midnight, not the last 24 hours", () => {
    // Being shown yesterday evening when you ask what you ran today is the
    // off-by-one that makes a date filter untrustworthy.
    expect(windowStart("today", NOW)).toBe(Date.UTC(2026, 8, 6, 0, 0, 0));
  });

  it("measures the other buckets as fixed lookbacks", () => {
    expect(windowStart("week", NOW)).toBe(NOW - 7 * 86_400_000);
    expect(windowStart("month", NOW)).toBe(NOW - 30 * 86_400_000);
  });

  it("opens all the way up for 'all'", () => {
    expect(windowStart("all", NOW)).toBe(0);
  });
});

describe("withinWindow", () => {
  it("includes a session exactly on the boundary", () => {
    const cutoff = windowStart("week", NOW);
    expect(withinWindow(session({ state_changed_at: cutoff }), "week", NOW)).toBe(true);
    expect(withinWindow(session({ state_changed_at: cutoff - 1 }), "week", NOW)).toBe(false);
  });

  it("keeps every session under 'all'", () => {
    expect(withinWindow(session({ state_changed_at: 0 }), "all", NOW)).toBe(true);
  });
});

describe("filterSessions", () => {
  const old = session({ name: "ancient", state_changed_at: NOW - 40 * 86_400_000 });
  const recent = session({ name: "today's-work", state_changed_at: NOW - 60_000 });
  const all = [old, recent];

  it("returns everything for an empty query and an open window", () => {
    expect(filterSessions(all, "", "all", NOW)).toHaveLength(2);
  });

  it("drops what falls outside the window", () => {
    expect(filterSessions(all, "", "today", NOW)).toEqual([recent]);
  });

  it("applies the query and the window together", () => {
    expect(filterSessions(all, "ancient", "today", NOW)).toEqual([]);
    expect(filterSessions(all, "today", "all", NOW)).toEqual([recent]);
  });

  it("preserves the input order", () => {
    // The UI groups and sorts downstream; filtering must not reorder.
    const first = session({ name: "b", state_changed_at: NOW - 3_000 });
    const second = session({ name: "a", state_changed_at: NOW - 1_000 });
    expect(filterSessions([first, second], "", "all", NOW).map((s) => s.name)).toEqual(["b", "a"]);
  });

  it("handles an empty list", () => {
    expect(filterSessions([], "", "all", NOW)).toEqual([]);
  });
});
