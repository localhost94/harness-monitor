import { HARNESS_CODE, HARNESS_LABEL, type Snapshot } from "../types";
import { formatCost, formatTokens, tokenBreakdown, tokenHeadline } from "../lib/format";
import { quotaFor, samePage, usagePages, type UsagePage } from "../lib/accounts";
import { useMonitor } from "../store/useMonitor";
import { Quota } from "./Quota";

/**
 * The pill has room for one page of usage at a time, so it pages - over
 * (account, harness) pairs, not harnesses. Two Claude subscriptions have two
 * separate 5-hour windows, and one ring for both would describe neither.
 *
 * Claude Code is the only harness with a plan window - the rings. Every other
 * harness bills per token, so its page shows what it actually reports: tokens,
 * and cost where it knows one. Paging rather than blending keeps those two
 * kinds of number from being read as comparable.
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
  const { usagePage, cycleUsage } = useMonitor();
  const pages = usagePages(snapshot);
  const fallback: UsagePage = { account: "", harness: "claude-code" };
  const page = pages.find((p) => samePage(p, usagePage)) ?? pages[0] ?? fallback;
  // One account is the common case: naming it on every page would be chrome
  // that never changes. It appears only once there is something to tell apart.
  const multiAccount = (snapshot?.accounts.length ?? 0) > 1;

  return (
    <div className="flex flex-col gap-0.5">
      <div className="flex items-center gap-0.5">
        <Chevron
          direction="prev"
          disabled={pages.length < 2}
          tone={tone.sub}
          onClick={() => cycleUsage(-1, pages)}
        />
        <span
          data-tauri-drag-region
          title={`Usage for ${HARNESS_LABEL[page.harness]}${
            page.account ? ` on ${page.account}` : ""
          }${pages.length > 1 ? " — arrows page through the others" : ""}`}
          className={`flex min-w-[1.6rem] max-w-[4.5rem] flex-col items-center leading-none ${tone.title}`}
        >
          <span className="text-[9px] font-bold tracking-wide">{HARNESS_CODE[page.harness]}</span>
          {multiAccount && (
            <span className={`w-full truncate text-center text-[8px] font-medium ${tone.sub}`}>
              {page.account}
            </span>
          )}
        </span>
        <Chevron
          direction="next"
          disabled={pages.length < 2}
          tone={tone.sub}
          onClick={() => cycleUsage(1, pages)}
        />
      </div>

      {page.harness === "claude-code" ? (
        <Quota
          quota={quotaFor(snapshot, page.account)}
          now={now}
          tone={tone}
          stack={stack}
          pending={!snapshot}
        />
      ) : (
        <HarnessUsage snapshot={snapshot} page={page} tone={tone} />
      )}
    </div>
  );
}

function HarnessUsage({
  snapshot,
  page,
  tone,
}: {
  snapshot: Snapshot | null;
  page: UsagePage;
  tone: { title: string; sub: string };
}) {
  const sessions = (snapshot?.sessions ?? []).filter(
    (s) => s.key.harness === page.harness && s.key.account === page.account,
  );
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
      title={direction === "prev" ? "Previous account / harness" : "Next account / harness"}
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
