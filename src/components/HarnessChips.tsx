import { HARNESS_CODE, HARNESS_DOT, HARNESS_LABEL, type AgentSession, type HarnessId } from "../types";

/**
 * Per-harness session counts.
 *
 * Chips are translucent glass with a coloured dot rather than solid colour
 * blocks: the pill's own surface is already carrying state, and five filled
 * chips on top of it turned into noise.
 *
 * Detected-but-empty harnesses stay visible but dim - "codex is installed and
 * quiet" and "codex is not here" are different facts.
 */
export function HarnessChips({
  sessions,
  detected,
  vertical,
}: {
  sessions: AgentSession[];
  detected: HarnessId[];
  vertical?: boolean;
}) {
  const counts = new Map<HarnessId, { total: number; attention: number }>();
  for (const harness of detected) counts.set(harness, { total: 0, attention: 0 });
  for (const session of sessions) {
    const entry = counts.get(session.key.harness) ?? { total: 0, attention: 0 };
    entry.total += 1;
    if (session.state === "awaiting-input" || session.state === "awaiting-permission") {
      entry.attention += 1;
    }
    counts.set(session.key.harness, entry);
  }

  return (
    <div
      data-tauri-drag-region
      // Vertical strip: two columns, so five harnesses do not turn the widget
      // into a ladder.
      className={vertical ? "grid grid-cols-2 gap-1" : "flex items-center gap-1"}
    >
      {[...counts.entries()].map(([harness, { total, attention }]) => (
        <span
          key={harness}
          data-tauri-drag-region
          title={`${HARNESS_LABEL[harness]}: ${total} session(s)${attention ? `, ${attention} waiting` : ""}`}
          className={`flex items-center gap-1 rounded-md bg-indigo-950/[0.06] px-1 py-px text-[9px] font-semibold tabular-nums text-indigo-950/70 ring-1 ring-inset ring-indigo-950/10 dark:bg-white/[0.07] dark:text-white/80 dark:ring-white/10 ${
            total === 0 ? "opacity-40" : ""
          } ${vertical ? "justify-between" : ""}`}
        >
          <span className="flex items-center gap-1">
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${HARNESS_DOT[harness]} ${
                attention > 0 ? "hm-alert" : ""
              }`}
            />
            {HARNESS_CODE[harness]}
          </span>
          {total}
        </span>
      ))}
    </div>
  );
}
