import { useRef, useState } from "react";
import { HARNESS_CHIP, HARNESS_CODE, HARNESS_LABEL, type AgentSession } from "../types";
import {
  ago,
  duration,
  formatCost,
  formatTokens,
  ROW_TINT,
  shortName,
  STATE_STYLE,
  stamp,
  tokenBreakdown,
  tokenHeadline,
} from "../lib/format";
import { filterSessions } from "../lib/search";
import { rerunBlockedReason, rerunTooltip } from "../lib/rerun";
import { useMonitor } from "../store/useMonitor";

/**
 * The history view: everything this machine can see that is no longer running.
 *
 * Flat and reverse-chronological, unlike the live list's harness groups. A date
 * range and a search box are both linear - they slice time and text - and
 * interleaving five harness headers into that only makes the result harder to
 * scan. Which harness a session was is on the row, where a single glance reads
 * it anyway.
 *
 * A finished row cannot be *jumped to*: the process is gone and the pane that
 * hosted it usually went with it, so there is nowhere to focus. It can be
 * *reopened*, because a session id and a working directory are all it takes to
 * relaunch the same harness on the same conversation. That is not a replay -
 * nothing anywhere records the command or the prompt a session started with -
 * and for two of the five harnesses it is not possible at all, so those rows
 * carry a button that is disabled and says why.
 */
export function FinishedList({ ended, now }: { ended: AgentSession[]; now: number }) {
  const { query, dateFilter, rerunSession } = useMonitor();
  const shown = filterSessions(ended, query, dateFilter, now);
  const sorted = [...shown].sort((a, b) => b.state_changed_at - a.state_changed_at);
  // A set rather than one flag: reopening two sessions is a legitimate thing to
  // want, and each launch is independent - the backend blocks on one agent
  // becoming interactive without holding up anything else.
  const [launching, setLaunching] = useState<ReadonlySet<string>>(new Set());
  // The same set, but readable synchronously. Disabling the button on the next
  // render is what stops a *second* click a moment later; this is what stops a
  // second click in the same tick, which a re-render cannot catch because the
  // re-render has not happened yet. Two terminals for one conversation is not a
  // cosmetic bug, so the guard does not depend on React's scheduling.
  const inFlight = useRef<Set<string>>(new Set());

  const launch = async (session: AgentSession) => {
    const key = rowKey(session);
    if (inFlight.current.has(key)) return;
    inFlight.current.add(key);
    setLaunching(new Set(inFlight.current));
    try {
      await rerunSession(session.key.harness, session.session_id, session.cwd);
    } finally {
      inFlight.current.delete(key);
      setLaunching(new Set(inFlight.current));
    }
  };

  if (ended.length === 0) {
    return (
      <p className="px-3 py-4 text-center text-[11px] text-black/45 dark:text-white/45">
        nothing finished yet that HarnessMonitor can see
      </p>
    );
  }

  if (sorted.length === 0) {
    return (
      <p className="px-3 py-4 text-center text-[11px] text-black/45 dark:text-white/45">
        {query.trim() === ""
          ? "nothing in this date range"
          : `nothing matching “${query.trim()}” in this date range`}
      </p>
    );
  }

  return (
    <div className="hm-scroll flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto px-3 pb-3">
      {shown.length < ended.length && (
        <p className="px-1 pt-0.5 text-[9px] tabular-nums text-black/35 dark:text-white/35">
          {shown.length} of {ended.length} sessions
        </p>
      )}
      {sorted.map((session) => (
        <EndedRow
          key={rowKey(session)}
          session={session}
          now={now}
          launching={launching.has(rowKey(session))}
          onRerun={() => void launch(session)}
        />
      ))}
    </div>
  );
}
/**
 * How long the session was open, or null when that is not knowable: a
 * harness that only stamps one timestamp, or a span too short to mean
 * anything.
 */
function ranFor(session: AgentSession): string | null {
  const span = session.state_changed_at - session.started_at;
  if (session.started_at <= 0 || span < 60_000) return null;
  return duration(session.started_at, session.state_changed_at);
}

/** Identity is the process: sessionId repeats across pids when resumed. */
function rowKey(s: AgentSession) {
  return `${s.key.harness}:${s.key.pid_domain}:${s.key.pid}:${s.key.proc_start}`;
}

/**
 * A session that has finished.
 *
 * No state chip, and no "idle" standing in for one: the file this row came from
 * still says whatever the harness last wrote - often `busy`, frozen mid-turn
 * months ago - and rendering that would be a lie about a process that does not
 * exist. What it can honestly say is when it was last touched and why it is
 * over.
 */
function EndedRow({
  session,
  now,
  launching,
  onRerun,
}: {
  session: AgentSession;
  now: number;
  launching: boolean;
  onRerun: () => void;
}) {
  const why =
    session.liveness === "unknown"
      ? "no verifiable process id on this host - filed as finished, not shown as live"
      : session.tier === "presence-only"
        ? "no longer recent; nothing is known about this conversation beyond its id"
        : "the process is gone; the harness never updated this file again";
  const when = stamp(session.state_changed_at, now);
  const blocked = rerunBlockedReason(session);

  return (
    <div
      className={`grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 overflow-hidden rounded-xl px-2 py-1.5 ring-1 ring-inset ring-black/[0.08] dark:ring-white/10 ${ROW_TINT.ended}`}
      title={`${why}\nLast activity: ${when}`}
    >
      <span
        className={`shrink-0 rounded px-1 py-px text-[9px] font-medium ${HARNESS_CHIP}`}
        title={HARNESS_LABEL[session.key.harness]}
      >
        {HARNESS_CODE[session.key.harness]}
      </span>
      <div className="min-w-0">
        <div className="flex items-baseline gap-1.5">
          <span className="truncate text-xs font-medium text-black/70 dark:text-white/70">
            {shortName(session)}
          </span>
          {session.model && (
            <span className="truncate text-[9px] text-black/35 dark:text-white/35">
              {session.model}
            </span>
          )}
        </div>
        <div className="truncate text-[10px] text-black/40 dark:text-white/40" title={session.cwd}>
          {session.cwd}
          {session.tokens && tokenHeadline(session.tokens) > 0 && (
            <span
              className="ml-1 tabular-nums text-black/55 dark:text-white/55"
              title={tokenBreakdown(session.tokens)}
            >
              · {formatTokens(tokenHeadline(session.tokens))} tok
            </span>
          )}
          {session.cost !== null && session.cost > 0 && (
            <span className="ml-1 font-semibold tabular-nums text-black/60 dark:text-white/60">
              · {formatCost(session.cost)}
            </span>
          )}
          {/* A span is only worth printing when there is one. Several harnesses
              set started_at to the same instant as the last write, and "ran 0s"
              on every row is noise that crowds out the path. */}
          {ranFor(session) !== null && (
            <span
              className="ml-1 tabular-nums text-black/35 dark:text-white/35"
              title={`Last activity: ${stamp(session.state_changed_at, now)}`}
            >
              · ran {ranFor(session)}
            </span>
          )}
        </div>
      </div>
      {/* Chip and timestamp stacked, button beside them: the same shape the
          live row uses for its jump arrow, so the two lists agree on where an
          action lives without a finished row gaining a jump it cannot have. */}
      <div className="flex items-center gap-1.5">
        <div className="flex flex-col items-end gap-0.5">
          <span
            className={`rounded px-1.5 py-0.5 text-[9px] font-medium ring-1 ring-inset ring-black/10 dark:text-white/40 dark:ring-white/10 ${STATE_STYLE.ended}`}
            title={why}
          >
            ended
          </span>
          <span
            className="text-[9px] tabular-nums text-black/35 dark:text-white/35"
            title={`Last activity: ${when}`}
          >
            {ago(session.state_changed_at, now)}
          </span>
        </div>
        <RerunButton session={session} blocked={blocked} launching={launching} onRerun={onRerun} />
      </div>
    </div>
  );
}

/**
 * Reopen the conversation this row describes.
 *
 * Disabled with its reason in the tooltip rather than hidden, for two reasons.
 * A missing control on some rows and not others reads as a rendering bug, and
 * the reason a row *cannot* be reopened is a fact about the user's own tools
 * that they are the only one who can act on.
 */
function RerunButton({
  session,
  blocked,
  launching,
  onRerun,
}: {
  session: AgentSession;
  blocked: string | null;
  launching: boolean;
  onRerun: () => void;
}) {
  const label = blocked ?? "Reopen this conversation";
  return (
    <button
      onClick={onRerun}
      // A launch in flight is already spoken for; a second click would open a
      // second terminal for the same conversation.
      disabled={blocked !== null || launching}
      // The pending state is named, not just animated: with reduced motion the
      // spinner does not turn, and the accessible name is the cue that survives.
      aria-label={launching ? "Opening this conversation" : label}
      aria-busy={launching}
      title={launching ? "Opening this conversation…" : (blocked ?? rerunTooltip(session))}
      className={`shrink-0 rounded-lg p-1 transition ${
        blocked === null && !launching
          ? "text-black/80 hover:bg-black/10 dark:text-white/80 dark:hover:bg-white/10"
          : "cursor-default text-black/25 dark:text-white/25"
      }`}
    >
      {launching ? (
        <svg
          viewBox="0 0 14 14"
          className="hm-spin h-3.5 w-3.5"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
          strokeLinecap="round"
          aria-hidden="true"
        >
          <path d="M7 1.6a5.4 5.4 0 1 1-5.1 3.6" />
        </svg>
      ) : (
        <svg
          viewBox="0 0 14 14"
          className="h-3.5 w-3.5"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          {/* A replay arrow, and deliberately not the live row's outward arrow:
              this one starts something, that one goes somewhere. */}
          <path d="M11.6 7a4.6 4.6 0 1 1-1.5-3.4" />
          <path d="M10.6 1.3v2.6H8" />
        </svg>
      )}
    </button>
  );
}