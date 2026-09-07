import { QUOTA_STALE_MS, type QuotaSnapshot } from "../types";
import { duration, untilReset } from "../lib/format";

/**
 * Claude plan window as a ring: a dial reads as a fraction at a glance, where
 * a number alone has to be compared against a limit you have to remember.
 *
 * The percentage is mirrored from the harness, never derived - and when the
 * reading goes stale it greys out with its age rather than being extrapolated.
 */
export function Quota({
  quota,
  now,
  tone,
  withLabel,
  pending,
}: {
  quota: QuotaSnapshot | null;
  now: number;
  tone: { title: string; sub: string };
  /** Spell out what the ring measures - the strip has no headline to lean on. */
  withLabel?: boolean;
  /** No snapshot has arrived yet - not the same as "no quota exists". */
  pending?: boolean;
}) {
  // Three different states, and conflating them is what made a working
  // install look broken: no snapshot yet (starting up), a snapshot with no
  // quota reading at all (nothing has reported one), and a real number.
  if (pending) {
    return (
      <span className={`text-[9px] tabular-nums ${tone.sub}`} title="Waiting for first snapshot">
        --
      </span>
    );
  }
  if (!quota || quota.five_hour_pct === null) {
    return (
      <span
        className={`max-w-[52px] text-[9px] leading-tight ${tone.sub}`}
        title={
          "No plan-window reading yet. Both sources are pushed by a live Claude Code process: " +
          "the statusline shim (installer/install-statusline.sh --install) on every render, or a " +
          "Stop hook writing ~/.claude/llm-analytics-usage/*.jsonl at the end of each turn."
        }
      >
        no quota yet
      </span>
    );
  }

  const stale = now - quota.at > QUOTA_STALE_MS;
  const pct = Math.min(100, Math.max(0, quota.five_hour_pct));
  const resets = untilReset(quota.five_hour_resets_at, now);
  const stroke = stale
    ? "stroke-zinc-400/70"
    : pct > 85
      ? "stroke-rose-400"
      : pct > 60
        ? "stroke-amber-300"
        : "stroke-emerald-300";

  const r = 11;
  const circumference = 2 * Math.PI * r;

  const ring = (
    <div
      data-tauri-drag-region
      title={
        stale
          ? `5h window: ${pct.toFixed(0)}% used as of ${duration(quota.at, now)} ago (source went quiet)`
          : `5h window: ${pct.toFixed(0)}% used${resets ? `, ${resets} left` : ""} (${quota.source})`
      }
      className={`relative flex h-[30px] w-[30px] items-center justify-center ${stale ? "opacity-50" : ""}`}
    >
      <svg viewBox="0 0 30 30" className="absolute inset-0 -rotate-90">
        <circle cx="15" cy="15" r={r} className="fill-none stroke-current opacity-20" strokeWidth="3.5" />
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
      <span className={`text-[9px] font-bold tabular-nums leading-none ${tone.title}`}>
        {pct.toFixed(0)}
      </span>
    </div>
  );

  if (!withLabel) return ring;

  return (
    <div data-tauri-drag-region className="flex items-center gap-1">
      {ring}
      <span className={`text-[9px] leading-tight ${tone.sub}`}>
        5h
        <br />
        used
      </span>
    </div>
  );
}
