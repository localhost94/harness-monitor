import type { AgentSession } from "../types";

/**
 * Running / idle / waiting counts, always all three, always in the same order
 * and the same place - so the numbers can be read positionally without
 * parsing labels. A zero stays visible but dims: "nothing is waiting" is
 * information, and a disappearing chip would shift the other two.
 */
export function StatStrip({
  sessions,
  vertical,
}: {
  sessions: AgentSession[];
  vertical?: boolean;
}) {
  const waiting = sessions.filter(
    (s) => s.state === "awaiting-input" || s.state === "awaiting-permission",
  ).length;
  const running = sessions.filter(
    (s) => s.state === "running" || s.state === "active-unknown",
  ).length;
  const idle = sessions.filter((s) => s.state === "idle" || s.state === "shell").length;

  return (
    <div
      data-tauri-drag-region
      className={`flex gap-1 ${vertical ? "flex-col items-stretch" : "items-center"}`}
    >
      <Stat
        value={running}
        label="running"
        short="running"
        wide={vertical}
        tone="text-sky-700 dark:text-sky-300"
      >
        <path d="M2 1.2 8.4 5 2 8.8Z" />
      </Stat>
      <Stat
        value={idle}
        label="idle"
        short="idle"
        wide={vertical}
        tone="text-indigo-900/55 dark:text-indigo-200/55"
      >
        <rect x="2.4" y="2.4" width="1.7" height="5.2" rx="0.8" />
        <rect x="5.9" y="2.4" width="1.7" height="5.2" rx="0.8" />
      </Stat>
      <Stat
        value={waiting}
        label="waiting for you"
        short="waiting"
        wide={vertical}
        tone="text-amber-700 dark:text-amber-300"
        pulse={waiting > 0}
      >
        <rect x="4.1" y="1" width="1.8" height="5" rx="0.9" />
        <circle cx="5" cy="8.2" r="1.05" />
      </Stat>
    </div>
  );
}

function Stat({
  value,
  label,
  short,
  tone,
  pulse,
  wide,
  children,
}: {
  value: number;
  label: string;
  /** Word shown next to the glyph in the vertical strip. */
  short: string;
  tone: string;
  pulse?: boolean;
  /** Vertical strip: stretch the chip and push the number to the right edge. */
  wide?: boolean;
  children: React.ReactNode;
}) {
  return (
    <span
      data-tauri-drag-region
      title={`${value} ${label}`}
      // Glass, like every other chip on the pill: the surface underneath is
      // already carrying colour, so a second filled colour reads as noise.
      className={`flex items-center gap-1 rounded-md bg-indigo-950/[0.06] px-1.5 py-px text-[10px] font-semibold tabular-nums ring-1 ring-inset ring-indigo-950/10 dark:bg-white/[0.07] dark:ring-white/10 ${tone} ${value === 0 ? "opacity-40" : ""} ${
        wide ? "justify-between" : ""
      }`}
    >
      {/* Glyph and label travel together on the left; the number stays hard
          right so the three rows read as a column of figures. A bare icon plus
          a number does not say what the number counts. */}
      <span className="flex min-w-0 items-center gap-1">
        <svg
          viewBox="0 0 10 10"
          className={`h-2 w-2 shrink-0 fill-current ${pulse ? "hm-alert" : ""}`}
          aria-hidden="true"
        >
          {children}
        </svg>
        {wide && <span className="truncate font-medium">{short}</span>}
      </span>
      {value}
    </span>
  );
}
