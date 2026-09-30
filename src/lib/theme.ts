import type { AgentSession } from "../types";

/**
 * One palette: paper and ink, in both directions. No hue anywhere.
 *
 * Monochrome is the risk in this design, not the answer. A grey widget on a
 * grey desktop is exactly what an editor looks like, and the widget is
 * designed to float over editors. So the surfaces sit at the extremes instead
 * of the middle: true white on true black, a hard 1px keyline all the way
 * round, a deep drop shadow and a hairline along the top edge. Not one of
 * those is a property a terminal or a code editor has. The pill reads as a
 * printed card that happens to be on screen, not as another pane.
 *
 * State is carried by value, never by hue: the rail down the left edge is ink
 * at a different density for each state, the headline glyph and its motion,
 * and a keyline ring around the whole pill when something is waiting on you.
 */
export type Mode = "permission" | "input" | "running" | "idle" | "empty";

export function modeOf(sessions: AgentSession[]): Mode {
  if (sessions.some((s) => s.state === "awaiting-permission")) return "permission";
  if (sessions.some((s) => s.state === "awaiting-input")) return "input";
  if (sessions.some((s) => s.state === "running" || s.state === "active-unknown"))
    return "running";
  return sessions.length > 0 ? "idle" : "empty";
}

const SHELL =
  "border-black/80 bg-gradient-to-b from-white to-[#ECECEA] shadow-[0_1px_0_0_rgba(0,0,0,0.07),0_12px_30px_-12px_rgba(0,0,0,0.45)] dark:border-white/25 dark:from-[#1B1B1B] dark:to-[#0B0B0B] dark:shadow-[0_1px_0_0_rgba(255,255,255,0.08),0_14px_34px_-12px_rgba(0,0,0,0.9)]";
const TITLE = "text-black dark:text-white";
const SUB = "text-black/55 dark:text-white/55";
const GLOSS = "from-black/[0.08] dark:from-white/20";

interface Surface {
  shell: string;
  /** Vertical rail down the left edge - this is where state shows. */
  edge: string;
  title: string;
  sub: string;
  gloss: string;
  /** Keyline around the pill, only when something is waiting on the user. */
  ring: string;
  /** Accent for the headline glyph. */
  accent: string;
}

/** Full ink: someone is blocked on an answer. Both waiting states get it. */
const ATTENTION_EDGE = "bg-black dark:bg-white";

const EDGE: Record<Mode, string> = {
  permission: ATTENTION_EDGE,
  input: ATTENTION_EDGE,
  running: "bg-gradient-to-b from-black/70 to-black/35 dark:from-white/70 dark:to-white/35",
  idle: "bg-gradient-to-b from-black/30 to-black/15 dark:from-white/30 dark:to-white/15",
  empty: "bg-gradient-to-b from-black/12 to-black/[0.06] dark:from-white/12 dark:to-white/[0.06]",
};

const RING: Record<Mode, string> = {
  permission: "ring-2 ring-black dark:ring-white",
  input: "ring-2 ring-black/50 dark:ring-white/50",
  running: "",
  idle: "",
  empty: "",
};

const ACCENT: Record<Mode, string> = {
  permission: "text-black dark:text-white",
  input: "text-black dark:text-white",
  running: "text-black/80 dark:text-white/80",
  idle: "text-black/60 dark:text-white/60",
  empty: "text-black/35 dark:text-white/35",
};

export function surfaceFor(mode: Mode): Surface {
  return {
    shell: SHELL,
    edge: EDGE[mode],
    title: TITLE,
    sub: SUB,
    gloss: GLOSS,
    ring: RING[mode],
    accent: ACCENT[mode],
  };
}

/**
 * Panel behind the session list: the same paper, printed a shade deeper, so
 * the list reads as a second leaf under the pill rather than a second window.
 */
export const PANEL =
  "border-black/15 bg-gradient-to-b from-[#F6F6F4]/95 to-[#E7E7E4]/95 shadow-[0_1px_0_0_rgba(0,0,0,0.05),0_14px_34px_-14px_rgba(0,0,0,0.5)] dark:border-white/15 dark:from-[#171717]/95 dark:to-[#0C0C0C]/95 dark:shadow-[0_16px_38px_-14px_rgba(0,0,0,0.9)]";

/** Header colour for a sticky row: the panel's top stop, so nothing shows through. */
export const PANEL_STICKY = "bg-[#F6F6F4]/95 dark:bg-[#171717]/95";
