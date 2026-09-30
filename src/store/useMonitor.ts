import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { MOCK_SNAPSHOT } from "../lib/mock";
import {
  DEFAULT_SETTINGS,
  needsRestart,
  type Settings,
  type SettingsView,
} from "../lib/settings";
import type { HarnessId, Snapshot } from "../types";

type Theme = "light" | "dark";
/**
 * The three ways the widget can be folded, in cycle order. A single enum
 * rather than a pair of booleans because "one line and vertical at once" is
 * not a shape, and two booleans would happily ask for it.
 */
const SHAPES = ["pill", "line", "vertical"] as const;
export type Shape = (typeof SHAPES)[number];

/** The shape one click of the shape button lands on. */
export function nextShape(shape: Shape): Shape {
  return SHAPES[(SHAPES.indexOf(shape) + 1) % SHAPES.length];
}

/**
 * The panel has two jobs and they want different layouts: "what needs me right
 * now" and "what did I run". Filtering and grouping suit the first; a search
 * box, a date range and a flat reverse-chronological list suit the second. One
 * list trying to be both is a compromise for both.
 */
type PanelView = "live" | "finished";

/** Date buckets for the history view, oldest last. */
const DATE_FILTERS = ["all", "today", "week", "month"] as const;
export type DateFilter = (typeof DATE_FILTERS)[number];
export const DATE_FILTER_LABEL: Record<DateFilter, string> = {
  all: "all",
  today: "today",
  week: "7 days",
  month: "30 days",
};
function isDateFilter(value: unknown): value is DateFilter {
  return DATE_FILTERS.includes(value as DateFilter);
}

interface MonitorState {
  snapshot: Snapshot | null;
  theme: Theme;
  toggleTheme: () => void;
  shape: Shape;
  cycleShape: () => Promise<void>;
  /** Set one shape outright, for a control that shows all three at once. */
  setShape: (shape: Shape) => Promise<void>;
  /** Which harness the pill's usage pager is showing. */
  usageHarness: HarnessId;
  cycleUsage: (delta: number, available: HarnessId[]) => void;
  /** Which question the panel is answering. */
  view: PanelView;
  setView: (view: PanelView) => void;
  /** Free-text filter over both views. Empty means "no filter". */
  query: string;
  setQuery: (query: string) => void;
  dateFilter: DateFilter;
  setDateFilter: (filter: DateFilter) => void;
  /** Last action error, shown briefly in the panel footer. */
  error: string | null;
  focusSession: (target: string) => Promise<void>;
  /**
   * Reopen a finished session's conversation in a new terminal.
   *
   * Not a replay: nothing records the command or prompt a session started with,
   * so this relaunches the same harness on the same conversation id. Which
   * harnesses can do that is decided in `lib/rerun.ts` before the click, not
   * here - a button that is enabled and then opens some *other* conversation is
   * worse than one that is honestly disabled.
   */
  rerunSession: (harness: HarnessId, sessionId: string, cwd: string) => Promise<void>;
  /**
   * The settings the running pipeline reads. Never null: it starts as
   * `DEFAULT_SETTINGS` so the popover has something to render before
   * `get_settings` answers, and is replaced by the file's values when they
   * land.
   */
  settings: Settings;
  /** What the scanner was actually started with, for the restart note. */
  appliedSettings: Settings | null;
  /** True when the edited scan settings are not yet the ones in force. */
  needsRestart: boolean;
  updateSettings: (patch: Partial<Settings>) => Promise<void>;
  /** Wall clock of the last snapshot received, for the "nothing arrived" hint. */
  receivedAt: number | null;
  startedAt: number;
  expanded: boolean;
  muted: boolean;
  now: number;
  init: () => Promise<void>;
  toggleExpanded: () => Promise<void>;
  toggleMuted: () => Promise<void>;
}

/**
 * Light by default: the pill floats over editors, and a dark widget on a dark
 * IDE is invisible. Dark stays available for light desktops.
 */
/**
 * The window shape is cosmetic; the snapshot stream is not.
 *
 * Returns the failure instead of throwing, because this runs *before* the
 * first `get_snapshot`: a rejected resize used to abort init right here, and
 * the pill then sat on "no sessions" forever with the agent streaming
 * perfectly good snapshots behind it and nothing on screen to say why.
 */
async function applyShape(shape: Shape, expanded: boolean): Promise<string | null> {
  if (!("__TAURI_INTERNALS__" in window)) return null;
  try {
    await invoke("set_shape", { shape, expanded });
    return null;
  } catch (err) {
    return `Could not resize the window: ${err}`;
  }
}

function readTheme(): Theme {
  // ?theme= wins, for headless previews; module init runs before any code in
  // main.tsx, so the override has to be read here.
  const override = new URLSearchParams(location.search).get("theme");
  if (override === "dark" || override === "light") return override;
  try {
    return localStorage.getItem("hm.theme") === "dark" ? "dark" : "light";
  } catch {
    return "light";
  }
}

/** Claude Code by default: it is the only harness with a plan window. */
function readUsageHarness(): HarnessId {
  const override = new URLSearchParams(location.search).get("usage");
  if (override) return override as HarnessId;
  try {
    const stored = localStorage.getItem("hm.usageHarness");
    return (stored as HarnessId) || "claude-code";
  } catch {
    return "claude-code";
  }
}

function readShape(): Shape {
  // ?shape= wins, for headless previews; module init runs before any code in
  // main.tsx, so the override has to be read here.
  const override = new URLSearchParams(location.search).get("shape");
  if (isShape(override)) return override;
  try {
    const stored = localStorage.getItem("hm.shape");
    if (isShape(stored)) return stored;
    // v0.1.1 stored a two-valued orientation under its own key.
    return localStorage.getItem("hm.orientation") === "vertical" ? "vertical" : "pill";
  } catch {
    return "pill";
  }
}

function isShape(value: unknown): value is Shape {
  return SHAPES.includes(value as Shape);
}

function readView(): PanelView {
  const override = new URLSearchParams(location.search).get("view");
  if (override === "finished" || override === "live") return override;
  // The preview harness and the released build share this key; an older
  // install stored a boolean under hm.showEnded, which meant the same thing.
  if (new URLSearchParams(location.search).get("ended") === "1") return "finished";
  try {
    return localStorage.getItem("hm.view") === "finished"
      ? "finished"
      : localStorage.getItem("hm.showEnded") === "1"
        ? "finished"
        : "live";
  } catch {
    return "live";
  }
}

function readDateFilter(): DateFilter {
  const override = new URLSearchParams(location.search).get("dates");
  if (isDateFilter(override)) return override;
  try {
    const stored = localStorage.getItem("hm.dateFilter");
    return isDateFilter(stored) ? stored : "all";
  } catch {
    return "all";
  }
}

/** Persist a preference, tolerating private mode where storage throws. */
function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private mode or blocked storage: the choice just won't survive a restart.
  }
}

export const useMonitor = create<MonitorState>((set, get) => ({
  snapshot: null,
  theme: readTheme(),
  shape: readShape(),
  usageHarness: readUsageHarness(),
  view: readView(),
  // Deliberately not persisted: a stale query would greet you with an empty
  // list and no visible reason for it. ?q= exists so the preview harness can
  // show a filtered list without a keyboard.
  query: new URLSearchParams(location.search).get("q") ?? "",
  dateFilter: readDateFilter(),
  error: null,
  receivedAt: null,
  startedAt: Date.now(),
  expanded: false,
  muted: false,
  // The defaults, not null: the popover has to render before `get_settings`
  // answers, and rendering "1.5s, all harnesses" is a better first frame than
  // rendering nothing. `init` replaces this the moment the real values land.
  settings: DEFAULT_SETTINGS,
  appliedSettings: null,
  needsRestart: false,
  now: Date.now(),

  init: async () => {
    // Opened in a plain browser (design review, no Tauri bridge): show the
    // fixture instead of hanging on an invoke that cannot resolve.
    if (!("__TAURI_INTERNALS__" in window)) {
      const params = new URLSearchParams(location.search);
      // ?only=running / ?only=idle / ?only=none exercise the other surfaces,
      // which otherwise only appear when real sessions happen to be in them.
      const only = params.get("only");
      let sessions = MOCK_SNAPSHOT.sessions;
      if (only === "running") {
        sessions = sessions.filter(
          (s) => s.state !== "awaiting-input" && s.state !== "awaiting-permission",
        );
      } else if (only === "idle") {
        sessions = sessions.map((s) => ({ ...s, state: "idle" as const, waiting_for: null }));
      } else if (only === "input") {
        sessions = sessions.filter((s) => s.state !== "awaiting-permission");
      } else if (only === "none") {
        sessions = [];
      }
      const many = Number(params.get("many") || 0);
      if (many > 1) {
        sessions = Array.from({ length: many }, (_, copy) =>
          sessions.map((s) => ({
            ...s,
            key: { ...s.key, pid: s.key.pid + copy * 1000 },
            name: copy === 0 ? s.name : `${s.name} ${copy + 1}`,
          })),
        ).flat();
      }
      set({
        snapshot: { ...MOCK_SNAPSHOT, sessions, ended: MOCK_SNAPSHOT.ended },
        receivedAt: Date.now(),
        expanded: !params.has("collapsed"),
        // The popover opens in the preview harness, so it needs something to
        // show. The defaults are the honest answer: nothing has been customised
        // because there is nothing to customise.
        settings: DEFAULT_SETTINGS,
        appliedSettings: DEFAULT_SETTINGS,
      });
      setInterval(() => set({ now: Date.now() }), 1000);
      return;
    }

    // A refused resize is reported, not fatal: the data path starts either way.
    const shapeError = await applyShape(get().shape, get().expanded);

    const [snapshot, muted, view] = await Promise.all([
      invoke<Snapshot | null>("get_snapshot").catch((err) => {
        console.error("get_snapshot failed", err);
        return null;
      }),
      invoke<boolean>("is_muted").catch(() => false),
      // The file is the only place mute survives a restart, so the store's own
      // `muted: false` is a placeholder until this answers.
      invoke<SettingsView | null>("get_settings")
        .then((answer) => answer?.settings ?? null)
        .catch((err) => {
          console.error("get_settings failed", err);
          return null;
        }),
    ]);
    set({
      snapshot,
      muted,
      receivedAt: snapshot ? Date.now() : null,
      ...(view ? { settings: view, appliedSettings: view, muted: view.muted } : {}),
      ...(shapeError ? { error: shapeError } : {}),
    });

    await listen<Snapshot>("snapshot", (event) =>
      set({ snapshot: event.payload, receivedAt: Date.now() }),
    );
    await listen<boolean>("muted", (event) => set({ muted: event.payload }));
    // Durations and quota staleness are time-relative; tick independently of
    // snapshots so the pill stays truthful when the source goes quiet.
    setInterval(() => set({ now: Date.now() }), 1000);
  },

  toggleTheme: () => {
    const theme: Theme = get().theme === "dark" ? "light" : "dark";
    remember("hm.theme", theme);
    set({ theme });
  },

  setView: (view) => {
    remember("hm.view", view);
    set({ view });
  },

  setQuery: (query) => set({ query }),

  setDateFilter: (dateFilter) => {
    remember("hm.dateFilter", dateFilter);
    set({ dateFilter });
  },

  cycleShape: async () => {
    await get().setShape(nextShape(get().shape));
  },

  setShape: async (shape) => {
    if (shape === get().shape) return;
    remember("hm.shape", shape);
    set({ shape });
    const failure = await applyShape(shape, get().expanded);
    if (failure) set({ error: failure });
  },

  cycleUsage: (delta: number, available: HarnessId[]) => {
    if (available.length === 0) return;
    const current = available.indexOf(get().usageHarness);
    // A harness that has since disappeared leaves index -1; stepping from
    // there lands on the first entry, which is what you want.
    const next = available[(current + delta + available.length) % available.length];
    remember("hm.usageHarness", next);
    set({ usageHarness: next });
  },

  focusSession: async (target: string) => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    try {
      await invoke("focus_session", { target });
      set({ error: null });
    } catch (err) {
      // Surfacing this matters: a silent no-op looks like a dead button.
      set({ error: `Could not jump: ${err}` });
      setTimeout(() => set({ error: null }), 5000);
    }
  },

  rerunSession: async (harness, sessionId, cwd) => {
    // No Tauri bridge (design review, preview harness): the button stays
    // clickable so its states can be looked at, and the click does nothing.
    if (!("__TAURI_INTERNALS__" in window)) return;
    try {
      await invoke("rerun_session", { harness, sessionId, cwd });
      set({ error: null });
    } catch (err) {
      // The backend names what went wrong - no herdr, a directory that is gone,
      // an agent that never came up - and all of it is worth reading, so the
      // whole message is passed through rather than summarised.
      set({ error: `Could not reopen: ${err}` });
      setTimeout(() => set({ error: null }), 8000);
    }
  },

  toggleExpanded: async () => {
    const expanded = !get().expanded;
    // Paint the panel first, then resize: the state change is what the click
    // asked for, and a refused resize only leaves the frame mis-sized.
    set({ expanded });
    const failure = await applyShape(get().shape, expanded);
    if (failure) set({ error: failure });
  },

  toggleMuted: async () => {
    // Through updateSettings rather than straight to `set_muted`, so there is
    // one place that writes the file and one place that falls back. The plain
    // command still exists for the tray menu, which has no settings view.
    await get().updateSettings({ muted: !get().muted });
  },

  updateSettings: async (patch) => {
    // Annotated, because spreading a `Partial` over a full type otherwise
    // widens every field to `T | undefined` and the backend wants the real
    // thing - a missing key there means "reset to default", not "leave alone".
    const merged: Settings = { ...get().settings, ...patch };
    // Painted before the write so a control never lags a click, and so the
    // restart note can appear immediately rather than after a round trip.
    set({
      settings: merged,
      muted: merged.muted,
      needsRestart: needsRestart(get().appliedSettings ?? merged, merged),
    });
    if (!(await persist(merged)) && "muted" in patch) {
      // Mute has to work even when the file cannot be written. Someone
      // silencing a widget that keeps lighting up their desktop is not in a
      // position to be told their settings directory is read-only, and the
      // in-memory atomic costs nothing to set.
      await invoke("set_muted", { muted: merged.muted }).catch(() => undefined);
    }
  },
}));

/**
 * Write the settings and adopt whatever the backend says it actually stored.
 *
 * Two reasons this is not fire-and-forget. The backend clamps, so a value that
 * came from a hand-edited file can come back different and the controls must
 * show what is in force. And `needs_restart` is the backend's own answer to
 * "is this visible yet" - recomputing it here would be a second implementation
 * of the same rule, free to disagree.
 */
async function persist(settings: Settings): Promise<boolean> {
  if (!("__TAURI_INTERNALS__" in window)) return false;
  try {
    const answer = await invoke<SettingsView>("set_settings", { settings });
    useMonitor.setState({
      settings: answer.settings,
      needsRestart: answer.needs_restart,
      ...(answer.error ? { error: `Could not save settings: ${answer.error}` } : {}),
    });
    return !answer.error;
  } catch (err) {
    useMonitor.setState({ error: `Could not save settings: ${err}` });
    return false;
  }
}
