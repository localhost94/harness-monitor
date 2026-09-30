import { HARNESS_CODE, HARNESS_LABEL, type AgentSession } from "../types";
import type { DateFilter } from "../store/useMonitor";

/**
 * Filtering, kept out of the components so the rules are readable in one place
 * and can be reasoned about without a DOM.
 *
 * Two filters, deliberately not merged: `matchesQuery` is about *which*
 * session, `withinWindow` is about *when*. Combining them into one opaque
 * predicate is how a filter ends up hiding rows for a reason the user cannot
 * see.
 */

/**
 * Everything a session can be searched by, lowercased.
 *
 * The session id matters more than it looks: antigravity has nothing but an
 * id, and it is the only handle a user has on that row.
 */
export function haystack(session: AgentSession): string {
  return [
    session.name,
    session.cwd,
    session.session_id,
    session.model,
    session.terminal_title,
    HARNESS_LABEL[session.key.harness],
    HARNESS_CODE[session.key.harness],
  ]
    .filter((field): field is string => Boolean(field))
    .join(" ")
    .toLowerCase();
}

/**
 * Space-separated terms, all of which must match - so `deploy claude` narrows
 * rather than widens. AND is what makes a second word useful at all; a
 * substring search that returns *more* rows when you type more is worse than
 * no search.
 */
export function matchesQuery(session: AgentSession, query: string): boolean {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return true;
  const hay = haystack(session);
  return terms.every((term) => hay.includes(term));
}

/**
 * The cut-off a date bucket means, as an epoch-ms floor.
 *
 * `today` is local midnight rather than "the last 24 hours": asking what you
 * ran today and being shown yesterday evening is the kind of off-by-one that
 * makes a date filter untrustworthy. Everything is bucketed on
 * `state_changed_at`, the last time the harness wrote to the session, which is
 * the only timestamp every adapter fills in - antigravity's file has nothing
 * else.
 */
export function windowStart(filter: DateFilter, now: number): number {
  switch (filter) {
    case "today": {
      const midnight = new Date(now);
      midnight.setHours(0, 0, 0, 0);
      return midnight.getTime();
    }
    case "week":
      return now - 7 * 86_400_000;
    case "month":
      return now - 30 * 86_400_000;
    case "all":
      return 0;
  }
}

export function withinWindow(session: AgentSession, filter: DateFilter, now: number): boolean {
  return session.state_changed_at >= windowStart(filter, now);
}

export function filterSessions(
  sessions: AgentSession[],
  query: string,
  filter: DateFilter,
  now: number,
): AgentSession[] {
  const cutoff = windowStart(filter, now);
  const terms = query.trim();
  return sessions.filter(
    (s) => s.state_changed_at >= cutoff && (terms === "" || matchesQuery(s, terms)),
  );
}