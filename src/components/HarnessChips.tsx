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

  const entries = [...counts.entries()];
  // Horizontally there is no room for five chips beside the counts and the
  // quota, and a harness with no sessions is the least useful of them - so the
  // empty ones collapse into a single "+N" that still names them on hover.
  const shown = vertical ? entries : entries.filter(([, c]) => c.total > 0);
  const hidden = vertical ? [] : entries.filter(([, c]) => c.total === 0);

  return (
    <div
      data-tauri-drag-region
      // Vertical strip: two columns, so five harnesses do not turn the widget
      // into a ladder.
      className={vertical ? "grid grid-cols-2 gap-1" : "flex min-w-0 items-center gap-1"}
    >
      {shown.map(([harness, { total, attention }]) => (
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
      {hidden.length > 0 && (
        <span
          data-tauri-drag-region
          title={`Detected but idle: ${hidden.map(([h]) => HARNESS_LABEL[h]).join(", ")}`}
          className="shrink-0 rounded-md bg-indigo-950/[0.06] px-1 py-px text-[9px] font-semibold tabular-nums text-indigo-950/45 ring-1 ring-inset ring-indigo-950/10 dark:bg-white/[0.07] dark:text-white/45 dark:ring-white/10"
        >
          +{hidden.length}
        </span>
      )}
    </div>
  );
}
