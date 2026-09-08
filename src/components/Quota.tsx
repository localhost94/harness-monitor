import { QUOTA_STALE_MS, type QuotaSnapshot } from "../types";
import { clockTime, dayAndTime, duration, untilReset } from "../lib/format";

/**
 * Both plan windows, each as a ring plus the wall-clock time it resets.
 *
 * The percentages are mirrored from the harness and never derived from token
 * counts. When the reading goes stale the whole block greys out and says how
 * old it is, rather than being extrapolated forward.
 */
export function Quota({
  quota,
  now,
  tone,
  stack,
  pending,
}: {
  quota: QuotaSnapshot | null;
  now: number;
  tone: { title: string; sub: string };
  /** Vertical strip: stack the two windows instead of sitting them side by side. */
  stack?: boolean;
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
}: {
  label: string;
  pct: number;
  resetsAt: string | null;
  left: string | null;
  tone: { title: string; sub: string };
  stale: boolean;
  staleAge: string | null;
  source: string;
}) {
  const pct = Math.min(100, Math.max(0, raw));
  const stroke = stale
    ? "stroke-zinc-400/70"
    : pct > 85
      ? "stroke-rose-400"
      : pct > 60
        ? "stroke-amber-300"
        : "stroke-emerald-300";
  const r = 11;
  const circumference = 2 * Math.PI * r;

  return (
    <div
      className="flex shrink-0 items-center gap-1"
      title={
        staleAge
          ? `${label} window: ${pct.toFixed(0)}% used as of ${staleAge} ago — the source went quiet`
          : `${label} window: ${pct.toFixed(0)}% used${left ? `, ${left} left` : ""} (from ${source})`
      }
    >
      <div className="relative flex h-[30px] w-[30px] shrink-0 items-center justify-center">
        <svg viewBox="0 0 30 30" className="absolute inset-0 -rotate-90">
          <circle
            cx="15"
            cy="15"
            r={r}
            className="fill-none stroke-current opacity-20"
            strokeWidth="3.5"
          />
          <circle
            cx="15"
            cy="15"
            r={r}
            className={`fill-none ${stroke}`}
            strokeWidth="3.5"
            strokeLinecap="round"
            strokeDasharray={`${(circumference * pct) / 100} ${circumference}`}
          />
        </svg>
        <span className={`text-[9px] font-bold leading-none tabular-nums ${tone.title}`}>
          {pct.toFixed(0)}
        </span>
      </div>
      <div className="flex flex-col leading-tight">
        <span className={`text-[9px] font-semibold ${tone.title}`}>{label}</span>
        {resetsAt && (
          <span className={`whitespace-nowrap text-[9px] tabular-nums ${tone.sub}`}>
            ↻ {resetsAt}
          </span>
        )}
      </div>
    </div>
  );
}
