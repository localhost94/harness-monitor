import type { AgentSession, HarnessId } from "../types";

/**
 * Whether a finished row can be reopened, and if not, why not.
 *
 * The reasons are not decoration. A button that is greyed out with no
 * explanation is a button people report as broken, and one that is *enabled*
 * but quietly opens some other conversation is worse than either - so the two
 * harnesses that cannot resume by id are named here rather than papered over
 * with a "latest" guess.
 *
 * This mirrors `resume_args` in `src-tauri/src/rerun.rs`. The two have to
 * agree, and the reason they are separate files rather than one is that they
 * are separate processes: the button must be disabled before anything is
 * invoked, so the frontend cannot ask the backend what it would do.
 */
export type RerunVerdict = { ok: true } | { ok: false; reason: string };

/** Harnesses that can reopen a conversation by its own session id. */
const RESUMABLE: Partial<Record<HarnessId, string>> = {
  "claude-code": "claude --resume",
  "open-code": "opencode --session",
  codex: "codex resume",
};

const UNRESUMABLE: Partial<Record<HarnessId, string>> = {
  // Verified against the installed CLI: `--resume` takes "latest" or an index
  // from a picker, never a session id. Offering "latest" would resume whichever
  // conversation happens to be newest in that directory, which is not the row
  // the user clicked.
  gemini: "gemini-cli can only resume “latest” or an index, not this session",
  // The conversation lives under ~/.gemini/antigravity rather than a project
  // directory, and the CLI has no resume flag at all.
  antigravity: "antigravity has no way to reopen a conversation",
};

export function isResumable(harness: HarnessId): boolean {
  return harness in RESUMABLE;
}

/** The command a row would run, for the tooltip. */
export function resumeCommand(harness: HarnessId): string | null {
  return RESUMABLE[harness] ?? null;
}

/**
 * Everything the button needs, decided from the row alone.
 *
 * No capability probe: herdr may or may not be installed, and there may or may
 * not be a terminal to fall back to, but neither of those changes whether a
 * click is *meaningful*. Those failures happen at launch time and are reported
 * there.
 */
export function canRerun(session: AgentSession): RerunVerdict {
  const blocked = UNRESUMABLE[session.key.harness];
  if (blocked) return { ok: false, reason: blocked };
  if (!isResumable(session.key.harness)) {
    return { ok: false, reason: "this harness cannot be reopened" };
  }
  if (session.session_id.trim() === "") {
    return { ok: false, reason: "this row has no session id" };
  }
  if (session.cwd.trim() === "") {
    return { ok: false, reason: "this row has no working directory" };
  }
  return { ok: true };
}

/** Why the button is unavailable, for its tooltip. `null` when it is available. */
export function rerunBlockedReason(session: AgentSession): string | null {
  const verdict = canRerun(session);
  return verdict.ok ? null : verdict.reason;
}

/** What the tooltip says when the button does work. */
export function rerunTooltip(session: AgentSession): string {
  const command = resumeCommand(session.key.harness);
  return `Reopen this conversation in a new terminal (${command} …)`;
}
