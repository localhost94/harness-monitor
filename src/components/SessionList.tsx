import {
  HARNESS_CHIP,
  HARNESS_CODE,
  HARNESS_LABEL,
  HARNESS_TEXT,
  type AgentSession,
  type HarnessId,
} from "../types";
import { duration, ROW_TINT, shortName } from "../lib/format";
import { StateChip } from "./StateBadge";
import { useMonitor } from "../store/useMonitor";

const TIER_NOTE: Record<AgentSession["tier"], string | null> = {
  full: null,
  "usage-only": "state inferred from recency",
  "presence-only": "activity only",
};

/** How long to wait before a missing snapshot is treated as a problem. */
const SNAPSHOT_TIMEOUT_MS = 12_000;

export function SessionList() {
  const { snapshot, now, startedAt, error } = useMonitor();
  if (!snapshot) {
    // The Windows build reads snapshots from an agent it launches inside WSL.
    // If that never starts there are no logs to look at by default, so say
    // what to check right here.
    if (now - startedAt > SNAPSHOT_TIMEOUT_MS) {
      return (
        <Empty>
          no snapshots yet.
          <br />
          check the WSL agent: <code className="text-zinc-700 dark:text-zinc-300">HM_AGENT_PATH</code>,{" "}
          <code className="text-zinc-700 dark:text-zinc-300">HM_WSL_DISTRO</code>
          <br />
          run with <code className="text-zinc-700 dark:text-zinc-300">HM_LOG_FILE</code> set to see why
        </Empty>
      );
    }
    return <Empty>waiting for first snapshot…</Empty>;
  }

  const byHarness = new Map<HarnessId, AgentSession[]>();
  for (const harness of snapshot.detected) byHarness.set(harness, []);
  for (const session of snapshot.sessions) {
    byHarness.set(session.key.harness, [...(byHarness.get(session.key.harness) ?? []), session]);
  }

  return (
    <div className="flex flex-col gap-3 overflow-y-auto px-3 pb-3">
      {[...byHarness.entries()].map(([harness, sessions]) => (
        <section key={harness} className="flex flex-col gap-1">
          <h2 className="flex items-center gap-1.5 px-1 text-[10px] font-medium uppercase tracking-wide">
            <span
              className={`rounded px-1 py-px text-[9px] ring-1 ring-inset ${HARNESS_CHIP[harness]}`}
            >
              {HARNESS_CODE[harness]}
            </span>
            <span className={HARNESS_TEXT[harness]}>{HARNESS_LABEL[harness]}</span>
            <span className="text-zinc-400 dark:text-zinc-600">{sessions.length}</span>
          </h2>
          {sessions.length === 0 ? (
            <p className="px-1 text-[11px] text-zinc-400 dark:text-zinc-600">no live sessions</p>
          ) : (
            sessions
              .sort((a, b) => b.state_changed_at - a.state_changed_at)
              .map((session) => <Row key={rowKey(session)} session={session} now={now} />)
          )}
        </section>
      ))}
      {snapshot.detected.length === 0 && <Empty>no harness data found on this host</Empty>}
      {error && (
        <p className="rounded-lg bg-rose-500/15 px-2 py-1 text-[10px] text-rose-700 ring-1 ring-inset ring-rose-500/30 dark:text-rose-300">
          {error}
        </p>
      )}
    </div>
  );
}

/** Identity is the process: sessionId repeats across pids when resumed. */
function rowKey(s: AgentSession) {
  return `${s.key.harness}:${s.key.pid_domain}:${s.key.pid}:${s.key.proc_start}`;
}

function Row({ session, now }: { session: AgentSession; now: number }) {
  const note = TIER_NOTE[session.tier];
  const focusSession = useMonitor((s) => s.focusSession);
  const jumpable = session.jump_target !== null;
  return (
    <div
      className={`grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 overflow-hidden rounded-xl px-2 py-1.5 ring-1 ring-inset ring-black/5 transition dark:ring-white/5 ${ROW_TINT[session.state]}`}
    >
      <span
        className={`shrink-0 rounded px-1 py-px text-[9px] font-medium ring-1 ring-inset ${
          HARNESS_CHIP[session.key.harness]
        }`}
        title={HARNESS_LABEL[session.key.harness]}
      >
        {HARNESS_CODE[session.key.harness]}
      </span>
      <div className="min-w-0">
        <div className="flex items-baseline gap-1.5">
          <span className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">
            {shortName(session)}
          </span>
          {session.terminal_title && (
            <span
              className="truncate text-[9px] text-indigo-700/70 dark:text-indigo-300/60"
              title={`Terminal tab: ${session.terminal_title}`}
            >
              {session.terminal_title}
            </span>
          )}
          {session.is_background && (
            <span className="text-[9px] uppercase text-zinc-500 dark:text-zinc-400">bg</span>
          )}
        </div>
        <div
          className="truncate text-[10px] text-zinc-500 dark:text-zinc-400"
          title={session.cwd}
        >
          {session.cwd}
          {note && <span className="ml-1 text-zinc-400 dark:text-zinc-500">· {note}</span>}
        </div>
      </div>
      <div className="flex items-center gap-1.5">
        <div className="flex flex-col items-end gap-0.5">
          <StateChip state={session.state} reason={session.waiting_for} />
          <span className="text-[9px] tabular-nums text-zinc-400 dark:text-zinc-500">
            {duration(session.state_changed_at, now)}
          </span>
        </div>
        {/* "Where is it?" is the question this answers - the whole reason the
            row carries a herdr pane id at all. */}
        <button
          onClick={() => session.jump_target && void focusSession(session.jump_target)}
          disabled={!jumpable}
          title={
            jumpable
              ? `Jump to this session (pane ${session.jump_target})`
              : "No jump target: this session is not in a herdr pane"
          }
          className={`rounded-lg p-1 transition ${
            jumpable
              ? "text-indigo-700 hover:bg-indigo-500/15 dark:text-indigo-300"
              : "cursor-default text-zinc-300 dark:text-zinc-600"
          }`}
        >
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
            <path d="M5.5 2.5h6v6M11.5 2.5 6 8M8.5 11.5h-6v-6" />
          </svg>
        </button>
      </div>
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <p className="px-3 py-4 text-center text-[11px] text-zinc-500 dark:text-zinc-400">{children}</p>
  );
}
