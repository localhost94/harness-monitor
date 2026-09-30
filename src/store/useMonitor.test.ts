import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessId } from "../types";
import * as tauri from "../test/tauri";

/**
 * The store reads `location.search` and `localStorage` at module scope, not
 * inside `create()`. So each scenario has to set the world up *before* the
 * module is imported, and each import has to be a fresh one - hence
 * `resetModules` + dynamic import rather than a static import at the top.
 */
async function loadStore() {
  vi.resetModules();
  return (await import("./useMonitor")).useMonitor;
}

function inTauri() {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
}

beforeEach(() => {
  localStorage.clear();
  window.history.replaceState({}, "", "/");
});

afterEach(() => {
  localStorage.clear();
  window.history.replaceState({}, "", "/");
});

describe("nextShape", () => {
  it("cycles the three shapes in order and wraps", async () => {
    const { nextShape } = await import("./useMonitor");
    expect(nextShape("pill")).toBe("line");
    expect(nextShape("line")).toBe("vertical");
    expect(nextShape("vertical")).toBe("pill");
  });

  it("is a fixed three-cycle, so the button always comes back round", async () => {
    const { nextShape } = await import("./useMonitor");
    let shape: "pill" | "line" | "vertical" = "pill";
    const seen = new Set<string>();
    for (let i = 0; i < 6; i += 1) {
      shape = nextShape(shape);
      seen.add(shape);
    }
    expect(seen).toEqual(new Set(["line", "vertical", "pill"]));
  });
});

describe("initial preferences", () => {
  it("defaults to light, because a dark widget on a dark IDE is invisible", async () => {
    const store = await loadStore();
    expect(store.getState().theme).toBe("light");
  });

  it("restores a persisted theme", async () => {
    localStorage.setItem("hm.theme", "dark");
    const store = await loadStore();
    expect(store.getState().theme).toBe("dark");
  });

  it("lets a query param win over storage, for headless previews", async () => {
    // main.tsx mounts before anything can pass a prop, so the override has to
    // be read at module init.
    localStorage.setItem("hm.theme", "light");
    window.history.replaceState({}, "", "/?theme=dark");
    const store = await loadStore();
    expect(store.getState().theme).toBe("dark");
  });

  it("ignores a nonsense theme param rather than adopting it", async () => {
    window.history.replaceState({}, "", "/?theme=chartreuse");
    const store = await loadStore();
    expect(store.getState().theme).toBe("light");
  });

  it("defaults the usage pager to Claude Code, the only harness with a window", async () => {
    const store = await loadStore();
    expect(store.getState().usageHarness).toBe("claude-code");
  });

  it("migrates the v0.1.1 two-valued orientation key", async () => {
    // An older install stored a boolean-shaped preference under its own key.
    localStorage.setItem("hm.orientation", "vertical");
    const store = await loadStore();
    expect(store.getState().shape).toBe("vertical");
  });

  it("falls back to the pill for an unrecognised stored shape", async () => {
    localStorage.setItem("hm.shape", "diagonal");
    const store = await loadStore();
    expect(store.getState().shape).toBe("pill");
  });

  it("honours the shape preview param", async () => {
    window.history.replaceState({}, "", "/?shape=vertical");
    const store = await loadStore();
    expect(store.getState().shape).toBe("vertical");
  });

  it("migrates the old showEnded boolean to the finished view", async () => {
    localStorage.setItem("hm.showEnded", "1");
    const store = await loadStore();
    expect(store.getState().view).toBe("finished");
  });

  it("honours the legacy ?ended=1 preview param", async () => {
    window.history.replaceState({}, "", "/?ended=1");
    const store = await loadStore();
    expect(store.getState().view).toBe("finished");
  });

  it("starts on the live view by default", async () => {
    const store = await loadStore();
    expect(store.getState().view).toBe("live");
  });

  it("seeds the query from ?q= but never persists it", async () => {
    // A stale query would greet you with an empty list and no visible reason.
    window.history.replaceState({}, "", "/?q=deploy");
    const store = await loadStore();
    expect(store.getState().query).toBe("deploy");

    store.getState().setQuery("other");
    expect(localStorage.getItem("hm.query")).toBeNull();
  });

  it("restores the date filter, and rejects an unknown one", async () => {
    localStorage.setItem("hm.dateFilter", "month");
    expect((await loadStore()).getState().dateFilter).toBe("month");

    localStorage.setItem("hm.dateFilter", "fortnight");
    expect((await loadStore()).getState().dateFilter).toBe("all");
  });
});

describe("persistence", () => {
  it("writes the theme it switched to", async () => {
    const store = await loadStore();
    store.getState().toggleTheme();
    expect(store.getState().theme).toBe("dark");
    expect(localStorage.getItem("hm.theme")).toBe("dark");
  });

  it("writes the view and the date filter", async () => {
    const store = await loadStore();
    store.getState().setView("finished");
    store.getState().setDateFilter("week");
    expect(localStorage.getItem("hm.view")).toBe("finished");
    expect(localStorage.getItem("hm.dateFilter")).toBe("week");
  });

  it("survives storage that throws, which is what private mode does", async () => {
    const store = await loadStore();
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    try {
      // The choice just will not survive a restart; the app must not die.
      expect(() => store.getState().toggleTheme()).not.toThrow();
      expect(store.getState().theme).toBe("dark");
    } finally {
      setItem.mockRestore();
    }
  });

  describe("when reading storage throws", () => {
    // Private mode and blocked third-party storage both throw on read, and
    // this module reads at import time - before React or main.tsx run at all.
    // An uncaught throw here is a blank window with no error anywhere.
    async function loadWithBrokenStorage() {
      const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
        throw new Error("SecurityError");
      });
      try {
        return await loadStore();
      } finally {
        getItem.mockRestore();
      }
    }

    it("starts light rather than failing to start", async () => {
      expect((await loadWithBrokenStorage()).getState().theme).toBe("light");
    });

    it("starts on Claude Code", async () => {
      expect((await loadWithBrokenStorage()).getState().usageHarness).toBe("claude-code");
    });

    it("starts as the pill", async () => {
      expect((await loadWithBrokenStorage()).getState().shape).toBe("pill");
    });

    it("starts on the live view", async () => {
      expect((await loadWithBrokenStorage()).getState().view).toBe("live");
    });

    it("starts on the open date range", async () => {
      expect((await loadWithBrokenStorage()).getState().dateFilter).toBe("all");
    });

    it("still honours a query-param override over the broken storage", async () => {
      window.history.replaceState({}, "", "/?theme=dark&shape=vertical&view=finished&dates=week");
      const store = await loadWithBrokenStorage();
      expect(store.getState().theme).toBe("dark");
      expect(store.getState().shape).toBe("vertical");
      expect(store.getState().view).toBe("finished");
      expect(store.getState().dateFilter).toBe("week");
    });
  });
});

describe("cycleUsage", () => {
  const available: HarnessId[] = ["claude-code", "open-code", "codex"];

  it("steps forward and backward, wrapping at both ends", async () => {
    const store = await loadStore();
    const { cycleUsage } = store.getState();

    cycleUsage(1, available);
    expect(store.getState().usageHarness).toBe("open-code");
    cycleUsage(1, available);
    expect(store.getState().usageHarness).toBe("codex");
    cycleUsage(1, available);
    expect(store.getState().usageHarness).toBe("claude-code");

    cycleUsage(-1, available);
    expect(store.getState().usageHarness).toBe("codex");
  });

  it("does nothing when there is nothing to page through", async () => {
    const store = await loadStore();
    store.getState().cycleUsage(1, []);
    expect(store.getState().usageHarness).toBe("claude-code");
  });

  it("lands on the first entry when the current harness has disappeared", async () => {
    // indexOf returns -1 for a harness that is no longer installed, and
    // stepping from -1 lands on the first entry - which is what you want.
    const store = await loadStore();
    store.setState({ usageHarness: "gemini" });
    store.getState().cycleUsage(1, available);
    expect(store.getState().usageHarness).toBe("claude-code");
  });

  it("remembers the harness it paged to", async () => {
    const store = await loadStore();
    store.getState().cycleUsage(1, available);
    expect(localStorage.getItem("hm.usageHarness")).toBe("open-code");
  });
});

describe("init", () => {
  it("shows the fixture when there is no Tauri bridge", async () => {
    // Design review and the preview harness run in a plain browser, where an
    // invoke cannot resolve and the UI would otherwise hang forever.
    const store = await loadStore();
    await store.getState().init();
    expect(store.getState().snapshot).not.toBeNull();
    expect(store.getState().receivedAt).not.toBeNull();
  });

  it("applies the ?only= previews the states that otherwise never appear", async () => {
    for (const [only, expected] of [
      ["running", 3],
      ["idle", 5],
      ["input", 4],
      ["none", 0],
    ] as const) {
      window.history.replaceState({}, "", `/?only=${only}`);
      const store = await loadStore();
      await store.getState().init();
      expect(store.getState().snapshot?.sessions.length, only).toBe(expected);
    }
  });

  it("keeps the history list intact under ?only=", async () => {
    // The preview reshapes what is running, not what happened to run.
    window.history.replaceState({}, "", "/?only=none");
    const store = await loadStore();
    await store.getState().init();
    expect(store.getState().snapshot?.ended.length).toBeGreaterThan(0);
  });

  it("forces every session idle for the ?only=idle preview", async () => {
    window.history.replaceState({}, "", "/?only=idle");
    const store = await loadStore();
    await store.getState().init();
    const sessions = store.getState().snapshot?.sessions ?? [];
    expect(sessions.every((s) => s.state === "idle")).toBe(true);
    // And clears the reason, which would otherwise contradict the new state.
    expect(sessions.every((s) => s.waiting_for === null)).toBe(true);
  });

  it("expands by default in the preview, unless ?collapsed says otherwise", async () => {
    const store = await loadStore();
    await store.getState().init();
    expect(store.getState().expanded).toBe(true);
  });

  it("repeats the fixture for ?many= so the list can overflow", async () => {
    window.history.replaceState({}, "", "/?many=3");
    const store = await loadStore();
    await store.getState().init();
    const sessions = store.getState().snapshot?.sessions ?? [];
    expect(sessions.length).toBeGreaterThan(3);
    // Each copy has to be a distinct process or the list would collapse it.
    expect(new Set(sessions.map((s) => s.key.pid)).size).toBe(sessions.length);
  });

  it("honours ?collapsed", async () => {
    window.history.replaceState({}, "", "/?collapsed");
    const store = await loadStore();
    await store.getState().init();
    expect(store.getState().expanded).toBe(false);
  });

  it("fetches a snapshot and the mute flag over the bridge", async () => {
    inTauri();
    const store = await loadStore();
    tauri.resolves("get_snapshot", { taken_at: 1, detected: [], sessions: [], ended: [] });
    tauri.resolves("is_muted", true);

    await store.getState().init();
    expect(store.getState().muted).toBe(true);
    expect(store.getState().receivedAt).not.toBeNull();
    expect(tauri.calls.map(([c]) => c)).toContain("get_snapshot");
  });

  it("still starts the data path when the resize is refused", async () => {
    // A rejected resize used to abort init right here, leaving the pill on "no
    // sessions" forever with a perfectly good stream behind it.
    inTauri();
    const store = await loadStore();
    tauri.rejects("set_shape", "window too small");
    tauri.resolves("get_snapshot", { taken_at: 1, detected: [], sessions: [], ended: [] });
    tauri.resolves("is_muted", false);

    await store.getState().init();
    expect(store.getState().error).toContain("Could not resize");
    expect(store.getState().snapshot).not.toBeNull();
  });

  it("stays up when the snapshot fetch itself fails", async () => {
    inTauri();
    const store = await loadStore();
    tauri.rejects("get_snapshot", "agent not running");
    tauri.resolves("is_muted", false);

    await expect(store.getState().init()).resolves.toBeUndefined();
    expect(store.getState().snapshot).toBeNull();
  });

  it("defaults to unmuted when the mute flag cannot be read", async () => {
    inTauri();
    const store = await loadStore();
    tauri.resolves("get_snapshot", null);
    tauri.rejects("is_muted", "no such file");

    await store.getState().init();
    expect(store.getState().muted).toBe(false);
  });

  it("applies a snapshot pushed over the event channel", async () => {
    inTauri();
    const store = await loadStore();
    tauri.resolves("get_snapshot", null);
    tauri.resolves("is_muted", false);
    await store.getState().init();

    tauri.emit("snapshot", { taken_at: 2, detected: ["codex"], sessions: [], ended: [] });
    expect(store.getState().snapshot?.detected).toEqual(["codex"]);

    tauri.emit("muted", true);
    expect(store.getState().muted).toBe(true);
  });
});

describe("focusSession", () => {
  it("does nothing outside a Tauri window", async () => {
    const store = await loadStore();
    await store.getState().focusSession("pane:1");
    expect(tauri.calls).toEqual([]);
  });

  it("sends the target over the bridge", async () => {
    inTauri();
    const store = await loadStore();
    await store.getState().focusSession("wA:p1");
    expect(tauri.calls).toContainEqual(["focus_session", { target: "wA:p1" }]);
  });

  it("says so when the jump fails, because a silent no-op looks like a dead button", async () => {
    inTauri();
    const store = await loadStore();
    tauri.rejects("focus_session", "no such pane");
    await store.getState().focusSession("gone");
    expect(store.getState().error).toContain("Could not jump");
  });

  it("clears the error once it has been shown", async () => {
    vi.useFakeTimers();
    inTauri();
    const store = await loadStore();
    tauri.rejects("focus_session", "no such pane");
    await store.getState().focusSession("gone");
    expect(store.getState().error).not.toBeNull();

    vi.advanceTimersByTime(5_000);
    expect(store.getState().error).toBeNull();
  });
});

describe("shape and expansion", () => {
  it("paints the panel before resizing, so a refusal cannot swallow the click", async () => {
    inTauri();
    tauri.rejects("set_shape", "too small");
    const store = await loadStore();
    await store.getState().toggleExpanded();
    expect(store.getState().expanded).toBe(true);
    expect(store.getState().error).toContain("Could not resize");
  });

  it("cycles the shape and tells the backend", async () => {
    inTauri();
    const store = await loadStore();
    await store.getState().cycleShape();
    expect(store.getState().shape).toBe("line");
    expect(tauri.calls).toContainEqual(["set_shape", { shape: "line", expanded: false }]);
  });
});

describe("toggleMuted", () => {
  it("sends the new value before adopting it locally", async () => {
    inTauri();
    const store = await loadStore();
    await store.getState().toggleMuted();
    expect(store.getState().muted).toBe(true);
    expect(tauri.calls).toContainEqual(["set_muted", { muted: true }]);
  });
});
