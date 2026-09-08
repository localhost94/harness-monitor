import { HARNESS_CODE, HARNESS_LABEL, type HarnessId, type Snapshot } from "../types";
import { formatCost, formatTokens, tokenBreakdown, tokenHeadline } from "../lib/format";
import { useMonitor } from "../store/useMonitor";
import { Quota } from "./Quota";

/**
 * The pill has room for one harness's usage at a time, so it pages.
 *
 * Claude Code is the default and the only one with a plan window - the rings.
 * Every other harness bills per token, so its page shows what it actually
 * reports: tokens, and cost where it knows one. Paging rather than blending
 * keeps those two kinds of number from being read as comparable.
 */
export function UsagePager({
  snapshot,
  now,
  tone,
  stack,
}: {
  snapshot: Snapshot | null;
  now: number;
  tone: { title: string; sub: string };
  stack?: boolean;
}) {
  const { usageHarness, cycleUsage } = useMonitor();
  const available = snapshot?.detected ?? [];
  const harness: HarnessId = available.includes(usageHarness)
    ? usageHarness
    : (available[0] ?? "claude-code");

  return (
    <div className="flex flex-col gap-0.5">
      <div className="flex items-center gap-0.5">
        <Chevron
          direction="prev"
          disabled={available.length < 2}
          tone={tone.sub}
          onClick={() => cycleUsage(-1, available)}
        />
        <span
          data-tauri-drag-region
          title={`Usage for ${HARNESS_LABEL[harness]}${
            available.length > 1 ? " — arrows page through the others" : ""
          }`}
          className={`min-w-[1.6rem] text-center text-[9px] font-bold tracking-wide ${tone.title}`}
        >
          {HARNESS_CODE[harness]}
        </span>
        <Chevron
          direction="next"
          disabled={available.length < 2}
          tone={tone.sub}
          onClick={() => cycleUsage(1, available)}
        />
      </div>

      {harness === "claude-code" ? (
        <Quota
          quota={snapshot?.quota ?? null}
          now={now}
          tone={tone}
          stack={stack}
          pending={!snapshot}
        />
      ) : (
        <HarnessUsage snapshot={snapshot} harness={harness} tone={tone} />
      )}
    </div>
  );
}

function HarnessUsage({
  snapshot,
  harness,
  tone,
}: {
  snapshot: Snapshot | null;
  harness: HarnessId;
  tone: { title: string; sub: string };
}) {
  const sessions = (snapshot?.sessions ?? []).filter((s) => s.key.harness === harness);
  let tokens = 0;
  let cost = 0;
  for (const session of sessions) {
    if (session.tokens) tokens += tokenHeadline(session.tokens);
    if (session.cost) cost += session.cost;
  }

  if (sessions.length === 0) {
    return (
      <span className={`whitespace-nowrap text-[9px] ${tone.sub}`}>no live sessions</span>
    );
  }

  const breakdown = sessions
    .filter((s) => s.tokens)
    .map((s) => `${s.name ?? s.session_id.slice(0, 8)}: ${tokenBreakdown(s.tokens!)}`)
    .join("\n");

  return (
    <div
      data-tauri-drag-region
      title={breakdown || `${sessions.length} session(s)`}
      className="flex flex-col leading-tight"
    >
      {/* Three cases, and never the same fact twice: tokens over cost, tokens
          over a session count, or - for a harness that reports neither - the
          count over an explicit "nothing reported". */}
      <span className={`whitespace-nowrap text-[9px] font-semibold tabular-nums ${tone.title}`}>
        {tokens > 0 ? `${formatTokens(tokens)} tok` : `${sessions.length} live`}
      </span>
      <span className={`whitespace-nowrap text-[9px] tabular-nums ${tone.sub}`}>
        {cost > 0
          ? formatCost(cost)
          : tokens > 0
            ? `${sessions.length} session${sessions.length > 1 ? "s" : ""}`
            : "no usage reported"}
      </span>
    </div>
  );
}

function Chevron({
  direction,
  disabled,
  tone,
  onClick,
}: {
  direction: "prev" | "next";
  disabled: boolean;
  tone: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={direction === "prev" ? "Previous harness" : "Next harness"}
      className={`rounded p-px transition hover:bg-indigo-950/10 disabled:opacity-30 disabled:hover:bg-transparent dark:hover:bg-white/10 ${tone}`}
    >
      <svg
        viewBox="0 0 10 10"
        className="h-2.5 w-2.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {direction === "prev" ? <path d="M6.2 2 3.2 5l3 3" /> : <path d="M3.8 2l3 3-3 3" />}
      </svg>
    </button>
  );
}
