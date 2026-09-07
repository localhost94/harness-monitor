import type { AgentSession } from "../types";

/**
 * Two themes, light and dark - the surface no longer changes with state.
 *
 * Both are deliberately violet-tinted rather than neutral: #F7F8FF is not the
 * white of a Windows dialog, and #252A4D is not the near-black of a terminal
 * or the neutral grey of editor chrome, so the pill never dissolves into
 * whatever sits behind it.
 *
 * State is still carried, just by accents instead of the whole surface: the
 * left rail, the glyph and its motion, the count chips, the row tints, and a
 * coloured ring when something is waiting on you.
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
  "border-indigo-400/45 bg-gradient-to-b from-[#F2F4FF] to-[#DFE5FA] shadow-[0_10px_30px_-12px_rgba(49,46,129,0.45)] dark:border-indigo-300/20 dark:from-[#252A4D] dark:to-[#1B1F3B] dark:shadow-[0_12px_34px_-12px_rgba(0,0,0,0.65)]";
const TITLE = "text-[#1A1D3A] dark:text-indigo-50";
const SUB = "text-[#1A1D3A]/60 dark:text-indigo-200/60";
const GLOSS = "from-white/80 dark:from-white/12";

interface Surface {
  shell: string;
  /** Vertical rail down the left edge - this is where state shows. */
  edge: string;
  title: string;
  sub: string;
  gloss: string;
  /** Ring around the pill, only when something is waiting on the user. */
  ring: string;
  /** Accent for the headline glyph. */
  accent: string;
}

const EDGE: Record<Mode, string> = {
  permission: "bg-gradient-to-b from-orange-400 to-rose-500",
  input: "bg-gradient-to-b from-amber-300 to-amber-500",
  running: "bg-gradient-to-b from-cyan-300 to-sky-500",
  idle: "bg-gradient-to-b from-indigo-300 to-violet-400 dark:from-indigo-400/70 dark:to-violet-500/70",
  empty: "bg-gradient-to-b from-slate-300 to-slate-400 dark:from-slate-600 dark:to-slate-700",
};

const RING: Record<Mode, string> = {
  permission: "ring-2 ring-orange-500/70 dark:ring-orange-400/60",
  input: "ring-2 ring-amber-500/70 dark:ring-amber-400/60",
  running: "",
  idle: "",
  empty: "",
};

const ACCENT: Record<Mode, string> = {
  permission: "text-orange-600 dark:text-orange-400",
  input: "text-amber-600 dark:text-amber-400",
  running: "text-sky-600 dark:text-sky-400",
  idle: "text-indigo-500 dark:text-indigo-300/70",
  empty: "text-slate-400 dark:text-slate-500",
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

/** Panel behind the session list: same family as the pill, calmer. */
export const PANEL =
  "border-indigo-400/40 bg-gradient-to-b from-[#EEF1FE]/95 to-[#E2E7FB]/95 shadow-[0_12px_34px_-14px_rgba(49,46,129,0.45)] dark:border-indigo-300/15 dark:from-[#202443]/95 dark:to-[#181B33]/95";
