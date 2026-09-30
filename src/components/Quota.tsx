import { QUOTA_STALE_MS, type QuotaSnapshot } from "../types";
import { clockTime, dayAndTime, duration, untilReset } from "../lib/format";

/**
 * Both plan windows, each as a ring plus the wall-clock time it resets.
 *
 * The percentages are mirrored from the harness and never derived from token
 * counts. With the hue gone, pressure is shown by weight instead: a low
 * reading is a thin arc, a middling one a heavier one, and a window nearly
 * spent a third heavier still. When the reading goes stale the whole block
 * greys out and says how old it is, rather than being extrapolated forward.
 */
export function Quota({
  quota,
  now,
  tone,
  stack,
  inline,
  pending,
}: {
  quota: QuotaSnapshot | null;
  now: number;
  tone: { title: string; sub: string };
  /** Vertical strip: stack the two windows instead of sitting them side by side. */
  stack?: boolean;
  /** One-line bar: shrink the rings and drop the reset clock, which only the
      tooltip has room for at this height. */
  inline?: boolean;
  /** No snapshot has arrived yet - not the same as "no quota exists". */
  pending?: boolean;
}) {
  // Three distinct states; conflating the first two made a working install
  // look broken.
  if (pending) {
    return (
      <span
        className={`text-[9px] tabular-nums ${tone.sub}`}
        title="Waiting for the first snapshot"
      >
        --
      </span>
    );
  }
  if (!quota || quota.five_hour_pct === null) {
    return (
      <span
        className={`max-w-[52px] text-[9px] leading-tight ${tone.sub}`}
        title={
          "No plan-window reading yet. Both sources are pushed by a live Claude Code " +
          "process: the statusline shim (installer/install-statusline.sh --install) on " +
          "every render, or a Stop hook writing ~/.claude/llm-analytics-usage/*.jsonl at " +
          "the end of each turn."
        }
      >
        no quota yet
      </span>
    );
  }

  const stale = now - quota.at > QUOTA_STALE_MS;

  return (
    <div
      data-tauri-drag-region
      className={`flex gap-x-2.5 gap-y-1 ${stack ? "flex-col" : "items-center"} ${
        stale ? "opacity-50" : ""
      }`}
    >
      <Window
        label="5h"
        pct={quota.five_hour_pct}
        resetsAt={clockTime(quota.five_hour_resets_at)}
        left={untilReset(quota.five_hour_resets_at, now)}
        tone={tone}
        stale={stale}
        staleAge={stale ? duration(quota.at, now) : null}
        source={quota.source}
        inline={inline}
      />
      {quota.seven_day_pct !== null && (
        <Window
          label="7d"
          pct={quota.seven_day_pct}
          resetsAt={dayAndTime(quota.seven_day_resets_at)}
          left={untilReset(quota.seven_day_resets_at, now)}
          tone={tone}
          stale={stale}
          staleAge={stale ? duration(quota.at, now) : null}
          source={quota.source}
          inline={inline}
        />
      )}
    </div>
  );
}

function Window({
  label,
  pct: raw,
  resetsAt,
  left,
  tone,
  stale,
  staleAge,
  source,
  inline,
}: {
  label: string;
  pct: number;
  resetsAt: string | null;
  left: string | null;
  tone: { title: string; sub: string };
  stale: boolean;
  staleAge: string | null;
  source: string;
  inline?: boolean;
}) {
  const pct = Math.min(100, Math.max(0, raw));
  const spent = pct > 85;
  // Three weights, not three colours: thin arc for plenty left, medium for the
  // middle, full ink and a heavier stroke for a window that is nearly gone.
  const stroke = stale
    ? "stroke-black/30 dark:stroke-white/25"
    : spent
      ? "stroke-black dark:stroke-white"
      : pct > 60
        ? "stroke-black/80 dark:stroke-white/80"
        : "stroke-black/50 dark:stroke-white/50";
  // 30px leaves no room for a second line of text at one-line height, so the
  // ring shrinks rather than the label wrapping or the bar growing.
  const r = inline ? 9 : 11;
  const circumference = 2 * Math.PI * r;

  return (
    <div
      className="flex shrink-0 items-center gap-1"
      title={
        staleAge
          ? `Claude plan, ${label} window: ${pct.toFixed(0)}% used as of ${staleAge} ago — the source went quiet`
          : `Claude plan, ${label} window: ${pct.toFixed(0)}% used${
              resetsAt ? `, resets ${resetsAt}` : ""
            }${left ? `, ${left} left` : ""} (from ${source}). Other harnesses have no plan window; their usage shows per session.`
      }
    >
      <div
        className={`relative flex shrink-0 items-center justify-center ${
          inline ? "h-[24px] w-[24px]" : "h-[30px] w-[30px]"
        }`}
      >
        <svg viewBox="0 0 30 30" className="absolute inset-0 -rotate-90">
          <circle
            cx="15"
            cy="15"
            r={r}
            className="fill-none stroke-current opacity-[0.12]"
            strokeWidth={inline ? 4 : 3.5}
          />
          <circle
            cx="15"
            cy="15"
            r={r}
            className={`fill-none ${stroke}`}
            strokeWidth={inline ? 4 : spent && !stale ? 5 : 3.5}
            strokeLinecap="round"
            strokeDasharray={`${(circumference * pct) / 100} ${circumference}`}
          />
        </svg>
      </div>
      <div className="flex flex-col leading-tight">
        {/* The figure sits in the label, not inside the ring: "52%" does not
            fit in a 30px ring at a legible size, and a bare "52" reads as a
            count rather than a percentage. The ring carries the proportion. */}
        <span className={`whitespace-nowrap text-[9px] font-semibold tabular-nums ${tone.title}`}>
          {label} {pct.toFixed(0)}%
        </span>
        {/* The reset clock is the first thing to go in a 44px strip: it is the
            same fact the tooltip above carries, and the expanded list shows it
            at full size. */}
        {resetsAt && !inline && (
          <span className={`whitespace-nowrap text-[9px] tabular-nums ${tone.sub}`}>
            ↻ {resetsAt}
          </span>
        )}
      </div>
    </div>
  );
}
