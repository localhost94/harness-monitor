import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { MOCK_SNAPSHOT } from "../lib/mock";
import type { Snapshot } from "../types";

type Theme = "light" | "dark";
type Orientation = "horizontal" | "vertical";

interface MonitorState {
  snapshot: Snapshot | null;
  theme: Theme;
  toggleTheme: () => void;
  orientation: Orientation;
  toggleOrientation: () => Promise<void>;
  /** Last action error, shown briefly in the panel footer. */
  error: string | null;
  focusSession: (target: string) => Promise<void>;
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
/** The window is sized by the backend: the webview cannot resize its own frame. */
async function applyShape(orientation: Orientation, expanded: boolean) {
  if (!("__TAURI_INTERNALS__" in window)) return;
  await invoke("set_shape", { vertical: orientation === "vertical", expanded });
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

function readOrientation(): Orientation {
  const override = new URLSearchParams(location.search).get("orientation");
  if (override === "vertical" || override === "horizontal") return override;
  try {
    return localStorage.getItem("hm.orientation") === "vertical" ? "vertical" : "horizontal";
  } catch {
    return "horizontal";
  }
}

export const useMonitor = create<MonitorState>((set, get) => ({
  snapshot: null,
  theme: readTheme(),
  orientation: readOrientation(),
  error: null,
  receivedAt: null,
  startedAt: Date.now(),
  expanded: false,
  muted: false,
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
        snapshot: { ...MOCK_SNAPSHOT, sessions },
        receivedAt: Date.now(),
        expanded: !params.has("collapsed"),
      });
      setInterval(() => set({ now: Date.now() }), 1000);
      return;
    }

    await applyShape(get().orientation, get().expanded);

    const [snapshot, muted] = await Promise.all([
      invoke<Snapshot | null>("get_snapshot"),
      invoke<boolean>("is_muted"),
    ]);
    set({ snapshot, muted, receivedAt: snapshot ? Date.now() : null });

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
    try {
      localStorage.setItem("hm.theme", theme);
    } catch {
      // Private mode or blocked storage: the choice just won't survive a restart.
    }
    set({ theme });
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

  toggleExpanded: async () => {
    const expanded = !get().expanded;
    await applyShape(get().orientation, expanded);
    set({ expanded });
  },

  toggleOrientation: async () => {
    const orientation: Orientation =
      get().orientation === "vertical" ? "horizontal" : "vertical";
    try {
      localStorage.setItem("hm.orientation", orientation);
    } catch {
      // Private mode: the choice just won't survive a restart.
    }
    await applyShape(orientation, get().expanded);
    set({ orientation });
  },

  toggleMuted: async () => {
    const muted = !get().muted;
    await invoke("set_muted", { muted });
    set({ muted });
  },
}));
