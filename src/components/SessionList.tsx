import { useState } from "react";
import {
  HARNESS_CHIP,
  HARNESS_CHIP_ALERT,
  HARNESS_CODE,
  HARNESS_LABEL,
  HARNESS_TEXT,
  type AgentSession,
  type HarnessId,
} from "../types";
import { PANEL_STICKY } from "../lib/theme";
import {
  duration,
  formatCost,
  formatTokens,
  ROW_TINT,
  shortName,
  tokenBreakdown,
  tokenHeadline,
} from "../lib/format";
import { filterSessions } from "../lib/search";
import { DATE_FILTER_LABEL, useMonitor, type DateFilter } from "../store/useMonitor";
import { FinishedList } from "./FinishedList";
import { SettingsPopover } from "./SettingsPopover";
import { StateChip } from "./StateBadge";

const TIER_NOTE: Record<AgentSession["tier"], string | null> = {
  full: null,
  "usage-only": "state inferred from recency",
  "presence-only": "activity only",
};

/** How long to wait before a missing snapshot is treated as a problem. */
const SNAPSHOT_TIMEOUT_MS = 12_000;

export function SessionList() {
  const { snapshot, now, startedAt, error } = useMonitor();
  const view = useMonitor((s) => s.view);
  // Local rather than in the store: the popover is a child of this component,
  // and nothing else in the app has any business knowing whether it is open.
  // `?settings=1` opens it for the headless preview harness, the same way
  // `?view=finished` and `?only=idle` reach states that otherwise need a click.
  const [settingsOpen, setSettingsOpen] = useState(
    () => new URLSearchParams(location.search).get("settings") === "1",
  );
  if (!snapshot) {
    // The Windows build reads snapshots from an agent it launches inside WSL.
    // If that never starts there are no logs to look at by default, so say
    // what to check right here.
    if (now - startedAt > SNAPSHOT_TIMEOUT_MS) {
      return (
        <Empty>
          no snapshots yet.
          <br />
          check the WSL agent:{" "}
          <code className="font-mono text-black/75 dark:text-white/75">HM_AGENT_PATH</code>,{" "}
          <code className="font-mono text-black/75 dark:text-white/75">HM_WSL_DISTRO</code>
          <br />
          run with{" "}
          <code className="font-mono text-black/75 dark:text-white/75">HM_LOG_FILE</code> set to
          see why
        </Empty>
      );
    }
    return <Empty>waiting for first snapshot…</Empty>;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PanelHeader
        live={snapshot.sessions.length}
        finished={snapshot.ended.length}
        settingsOpen={settingsOpen}
        onToggleSettings={() => setSettingsOpen((open) => !open)}
      />
      {view === "live" ? (
        <LiveList snapshot={snapshot} now={now} />
      ) : (
        <FinishedList ended={snapshot.ended} now={now} />
      )}
      {settingsOpen && <SettingsPopover onClose={() => setSettingsOpen(false)} />}
      {/* Above the popover, not beside it. A settings write that fails is
          reported here and nowhere else, so it has to stay readable even with
          the sheet that caused it open. The error is transient and
          high-contrast; the sheet is neither. */}
      {error && (
        <p className="relative z-30 mx-3 mb-2 shrink-0 rounded-lg bg-black px-2 py-1 text-[10px] font-medium text-white ring-2 ring-black dark:bg-white dark:text-black dark:ring-white">
          {error}
        </p>
      )}
    </div>
  );
}

/**
 * Two tabs, a search box, and a date range - the second two only matter on the
 * history tab.
 *
 * `data-no-drag` because the whole panel sits inside a drag region: a click
 * that lands on the gap between the search box and the tab strip would
 * otherwise drag the window instead of pressing nothing.
 */
function PanelHeader({
  live,
  finished,
  settingsOpen,
  onToggleSettings,
}: {
  live: number;
  finished: number;
  settingsOpen: boolean;
  onToggleSettings: () => void;
}) {
  const { view, setView, query, setQuery, dateFilter, setDateFilter } = useMonitor();
  return (
    <div data-no-drag className="flex shrink-0 flex-col gap-1.5 px-3 pb-1.5 pt-2">
      <div className="flex items-center gap-1">
        <Tab label="live" count={live} active={view === "live"} onClick={() => setView("live")} />
        <Tab
          label="finished"
          count={finished}
          active={view === "finished"}
          onClick={() => setView("finished")}
        />
        {/* Only in the panel, never on the pill: the pill's four buttons sit in
            a 2x2 that 80px of height cannot grow, and a fifth icon is a worse
            answer than one more click to reach. */}
        <button
          onClick={onToggleSettings}
          aria-expanded={settingsOpen}
          aria-label="Settings"
          title="Settings"
          className={`ml-auto rounded-lg p-0.5 transition hover:bg-black/10 dark:hover:bg-white/10 ${
            settingsOpen ? "text-black dark:text-white" : "text-black/45 dark:text-white/45"
          }`}
        >
          <svg
            viewBox="0 0 14 14"
            className="h-3.5 w-3.5"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.3"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            {/* Six short teeth rather than eight long ones: at 14px the longer
                version reads as a sun or an asterisk, which is neither a gear
                nor anything else a user is looking for. */}
            <circle cx="7" cy="7" r="2.1" />
            <path d="M7 1.7v1.5M7 10.8v1.5M12.3 7h-1.5M3.2 7H1.7M10.73 3.27l-1.06 1.06M4.33 9.67l-1.06 1.06M10.73 10.73 9.67 9.67M4.33 4.33 3.27 3.27" />
          </svg>
        </button>
      </div>

      <div className="relative">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          spellCheck={false}
          autoComplete="off"
          placeholder={view === "live" ? "filter live sessions" : "search name, path, id, model"}
          aria-label={view === "live" ? "Filter live sessions" : "Search finished sessions"}
          className="w-full rounded-lg bg-black/[0.04] py-1 pl-2 pr-6 text-[11px] text-black/80 ring-1 ring-inset ring-black/10 outline-none placeholder:text-black/30 focus:ring-black/35 dark:bg-white/[0.05] dark:text-white/80 dark:ring-white/10 dark:placeholder:text-white/30 dark:focus:ring-white/40"
        />
        {query !== "" && (
          <button
            onClick={() => setQuery("")}
            title="Clear search"
            aria-label="Clear search"
            className="absolute right-0.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-[13px] leading-none text-black/40 hover:bg-black/10 hover:text-black/70 dark:text-white/40 dark:hover:bg-white/10 dark:hover:text-white/80"
          >
            ×
          </button>
        )}
      </div>

      {/* The range only means something for history, and a control that changes
          nothing is worse than no control. */}
      {view === "finished" && (
        <div className="flex items-center gap-1">
          <span className="text-[9px] uppercase tracking-wide text-black/35 dark:text-white/35">
            last active
          </span>
          {(["all", "today", "week", "month"] as DateFilter[]).map((filter) => (
            <button
              key={filter}
              onClick={() => setDateFilter(filter)}
              title={`Sessions last active ${DATE_FILTER_LABEL[filter]}`}
              className={`rounded-md px-1.5 py-0.5 text-[9px] font-medium ring-1 ring-inset transition ${
                dateFilter === filter
                  ? "bg-black/[0.07] text-black/75 ring-black/20 dark:bg-white/[0.10] dark:text-white/85 dark:ring-white/20"
                  : "text-black/45 ring-black/10 hover:bg-black/[0.04] dark:text-white/45 dark:ring-white/10 dark:hover:bg-white/[0.06]"
              }`}
            >
              {DATE_FILTER_LABEL[filter]}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function Tab({
  label,
  count,
  active,
  onClick,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      className={`flex items-center gap-1 rounded-lg px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide ring-1 ring-inset transition ${
        active
          ? "bg-black text-white ring-black dark:bg-white dark:text-black dark:ring-white"
          : "text-black/45 ring-black/10 hover:bg-black/[0.04] dark:text-white/45 dark:ring-white/10 dark:hover:bg-white/[0.06]"
      }`}
    >
      {label}
      <span className={active ? "tabular-nums opacity-70" : "tabular-nums opacity-60"}>{count}</span>
    </button>
  );
}

function LiveList({
  snapshot,
  now,
}: {
  snapshot: NonNullable<ReturnType<typeof useMonitor.getState>["snapshot"]>;
  now: number;
}) {
  const query = useMonitor((s) => s.query);
  const sessions = filterSessions(snapshot.sessions, query, "all", now);
  const byHarness = new Map<HarnessId, AgentSession[]>();
  for (const harness of snapshot.detected) byHarness.set(harness, []);
  for (const session of sessions) {
    byHarness.set(session.key.harness, [...(byHarness.get(session.key.harness) ?? []), session]);
  }

  // With a dozen sessions the list scrolls, so what needs you has to be near
  // the top rather than wherever its harness happens to sort.
  const filtering = query.trim() !== "";
  // A search that leaves four harnesses each printing "nothing matching here"
  // has answered with noise. While filtering, only the groups that matched
  // stay; the count line says how much was hidden.
  const groups = [...byHarness.entries()]
    .filter(([, group]) => !filtering || group.length > 0)
    .sort(([, a], [, b]) => rank(b) - rank(a) || b.length - a.length);

  return (
    <div className="hm-scroll flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-3 pb-3">
      {filtering && (
        <p className="px-1 text-[9px] tabular-nums text-black/35 dark:text-white/35">
          {sessions.length} of {snapshot.sessions.length} live sessions
        </p>
      )}
      {groups.length === 0 && (
        <p className="px-1 py-3 text-center text-[11px] text-black/45 dark:text-white/45">
          no live session matches “{query.trim()}”
        </p>
      )}
      {groups.map(([harness, group]) => (
        <section key={harness} className="flex flex-col gap-1">
          <h2
            className={`sticky top-0 z-10 -mx-1 flex items-center gap-1.5 px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide backdrop-blur ${PANEL_STICKY}`}
          >
            <span className={`rounded px-1 py-px text-[9px] ${HARNESS_CHIP}`}>
              {HARNESS_CODE[harness]}
            </span>
            <span className={HARNESS_TEXT}>{HARNESS_LABEL[harness]}</span>
            <span className="text-black/40 dark:text-white/40">{group.length}</span>
            <Aggregate sessions={group} />
          </h2>
          {group.length === 0 ? (
            <p className="px-1 text-[11px] text-black/40 dark:text-white/40">no live sessions</p>
          ) : (
            group
              .sort(
                (a, b) =>
                  Number(b.state.startsWith("awaiting")) -
                    Number(a.state.startsWith("awaiting")) ||
                  b.state_changed_at - a.state_changed_at,
              )
              .map((session) => <Row key={rowKey(session)} session={session} now={now} />)
          )}
        </section>
      ))}
      {snapshot.detected.length === 0 && <Empty>no harness data found on this host</Empty>}
      {/* The rings in the pill are a Claude subscription window; nothing else
          here has an equivalent. Saying so once beats implying the numbers are
          comparable. */}
      {snapshot.detected.some((h) => h !== "claude-code") && (
        <p className="px-1 pt-1 text-[9px] leading-snug text-black/40 dark:text-white/45">
          The 5h / 7d rings are Claude&apos;s plan window — only Claude Code reports one. The other
          harnesses bill per token, so they show tokens and cost per session instead.
        </p>
      )}
    </div>
  );
}

/** Groups with sessions waiting on the user sort first, then busier groups. */
function rank(sessions: AgentSession[]): number {
  let score = 0;
  for (const s of sessions) {
    if (s.state === "awaiting-permission") score += 100;
    else if (s.state === "awaiting-input") score += 90;
    else if (s.state === "running" || s.state === "active-unknown") score += 10;
  }
  return score;
}

/** Identity is the process: sessionId repeats across pids when resumed. */
function rowKey(s: AgentSession) {
  return `${s.key.harness}:${s.key.pid_domain}:${s.key.pid}:${s.key.proc_start}`;
}

function Row({ session, now }: { session: AgentSession; now: number }) {
  const note = TIER_NOTE[session.tier];
  const focusSession = useMonitor((s) => s.focusSession);
  const jumpable = session.jump_target !== null;
  const alert = session.state.startsWith("awaiting");
  return (
    <div
      className={`grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 overflow-hidden rounded-xl px-2 py-1.5 ring-1 ring-inset ring-black/[0.08] transition dark:ring-white/10 ${ROW_TINT[session.state]}`}
    >
      <span
        className={`shrink-0 rounded px-1 py-px text-[9px] font-medium ${
          alert ? HARNESS_CHIP_ALERT : HARNESS_CHIP
        }`}
        title={HARNESS_LABEL[session.key.harness]}
      >
        {HARNESS_CODE[session.key.harness]}
      </span>
      <div className="min-w-0">
        <div className="flex items-baseline gap-1.5">
          <span className="truncate text-xs font-medium text-black dark:text-white">
            {shortName(session)}
          </span>
          {session.terminal_title && (
            <span
              className="truncate text-[9px] text-black/45 dark:text-white/45"
              title={`Terminal tab: ${session.terminal_title}`}
            >
              {session.terminal_title}
            </span>
          )}
          {session.is_background && (
            <span className="text-[9px] uppercase text-black/45 dark:text-white/45">bg</span>
          )}
        </div>
        <div
          className="truncate text-[10px] text-black/50 dark:text-white/50"
          title={session.cwd}
        >
          {session.cwd}
          {session.tokens && tokenHeadline(session.tokens) > 0 && (
            <span
              className="ml-1 tabular-nums text-black/65 dark:text-white/65"
              title={tokenBreakdown(session.tokens)}
            >
              · {formatTokens(tokenHeadline(session.tokens))} tok
            </span>
          )}
          {session.cost !== null && session.cost > 0 && (
            <span className="ml-1 font-semibold tabular-nums text-black dark:text-white">
              · {formatCost(session.cost)}
            </span>
          )}
          {note && <span className="ml-1 text-black/40 dark:text-white/40">· {note}</span>}
        </div>
      </div>
      <div className="flex items-center gap-1.5">
        <div className="flex flex-col items-end gap-0.5">
          <StateChip state={session.state} reason={session.waiting_for} />
          <span className="text-[9px] tabular-nums text-black/40 dark:text-white/45">
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
              ? "text-black/80 hover:bg-black/10 dark:text-white/80 dark:hover:bg-white/10"
              : "cursor-default text-black/25 dark:text-white/25"
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

/**
 * Per-harness totals. Claude Code reports no per-session cost (a subscription
 * has none to report), while opencode does - so this shows whichever the
 * harness actually gives, rather than a blank or a zero that would read as
 * "free".
 */
function Aggregate({ sessions }: { sessions: AgentSession[] }) {
  let tokens = 0;
  let cost = 0;
  for (const s of sessions) {
    if (s.tokens) tokens += tokenHeadline(s.tokens);
    if (s.cost) cost += s.cost;
  }
  if (tokens === 0 && cost === 0) return null;

  return (
    <span className="ml-auto flex items-center gap-1.5 font-normal normal-case tracking-normal">
      {tokens > 0 && (
        <span className="tabular-nums text-black/50 dark:text-white/50">
          {formatTokens(tokens)} tok
        </span>
      )}
      {cost > 0 && (
        <span className="tabular-nums text-black/75 dark:text-white/75">{formatCost(cost)}</span>
      )}
    </span>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <p className="px-3 py-4 text-center text-[11px] text-black/50 dark:text-white/50">{children}</p>
  );
}