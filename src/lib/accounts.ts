import type { AccountSnapshot, AgentSession, HarnessId, QuotaSnapshot, Snapshot } from "../types";

/**
 * Reading a snapshot as accounts rather than as one machine.
 *
 * Every helper here derives from `snapshot.accounts` + `snapshot.sessions`
 * rather than from a second denormalised field, so there is exactly one place
 * the backend can be wrong about who owns what.
 */

/** Union across accounts - what the pill's chips count. */
export function detectedHarnesses(snapshot: Snapshot | null): HarnessId[] {
  const seen = new Set<HarnessId>();
  for (const account of snapshot?.accounts ?? []) {
    for (const harness of account.detected) seen.add(harness);
  }
  return [...seen];
}

export function quotaFor(snapshot: Snapshot | null, account: string): QuotaSnapshot | null {
  return snapshot?.quotas.find((q) => q.account === account) ?? null;
}

/** One page of the pill's usage pager: a single account's single harness. */
export interface UsagePage {
  account: string;
  harness: HarnessId;
}

export function samePage(a: UsagePage | null, b: UsagePage | null): boolean {
  return !!a && !!b && a.account === b.account && a.harness === b.harness;
}

/**
 * Every (account, harness) the pill can show, in the order it pages through
 * them: accounts in configured order, harnesses in detection order. One
 * account with three harnesses pages the same way it always did - the account
 * dimension simply has length 1.
 */
export function usagePages(snapshot: Snapshot | null): UsagePage[] {
  const pages: UsagePage[] = [];
  for (const account of snapshot?.accounts ?? []) {
    for (const harness of account.detected) pages.push({ account: account.label, harness });
  }
  return pages;
}

export interface AccountGroup {
  account: AccountSnapshot;
  quota: QuotaSnapshot | null;
  /** Detected harnesses, each with its sessions - empty list included, since
   *  "installed and quiet" and "not installed" are different facts. */
  harnesses: { harness: HarnessId; sessions: AgentSession[] }[];
  attention: number;
}

export interface DivisionGroup {
  division: string;
  accounts: AccountGroup[];
  attention: number;
}

function needsAttention(s: AgentSession): boolean {
  return s.state === "awaiting-input" || s.state === "awaiting-permission";
}

/**
 * Sessions bucketed division -> account -> harness, sorted so whatever is
 * blocked on a human floats to the top at every level. A manager scrolling a
 * dozen accounts should never have to hunt for the one that stopped.
 */
export function byDivision(snapshot: Snapshot): DivisionGroup[] {
  const divisions = new Map<string, AccountGroup[]>();

  for (const account of snapshot.accounts) {
    const mine = snapshot.sessions.filter((s) => s.key.account === account.label);
    const harnesses = account.detected.map((harness) => ({
      harness,
      sessions: mine
        .filter((s) => s.key.harness === harness)
        .sort(
          (a, b) =>
            Number(needsAttention(b)) - Number(needsAttention(a)) ||
            b.state_changed_at - a.state_changed_at,
        ),
    }));
    const waiting = (h: { sessions: AgentSession[] }) => h.sessions.filter(needsAttention).length;
    harnesses.sort((a, b) => waiting(b) - waiting(a) || b.sessions.length - a.sessions.length);

    const group: AccountGroup = {
      account,
      quota: quotaFor(snapshot, account.label),
      harnesses,
      attention: mine.filter(needsAttention).length,
    };
    divisions.set(account.division, [...(divisions.get(account.division) ?? []), group]);
  }

  const groups = [...divisions.entries()].map(([division, accounts]) => ({
    division,
    accounts: accounts.sort((a, b) => b.attention - a.attention),
    attention: accounts.reduce((sum, a) => sum + a.attention, 0),
  }));
  return groups.sort((a, b) => b.attention - a.attention || a.division.localeCompare(b.division));
}
