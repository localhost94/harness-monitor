import { describe, expect, it } from "vitest";
import type { HarnessId } from "../types";
import { canRerun, isResumable, rerunBlockedReason, resumeCommand } from "./rerun";
import { session } from "../test/fixtures";

const ALL: HarnessId[] = ["claude-code", "open-code", "codex", "gemini", "antigravity"];

describe("canRerun", () => {
  it("allows the three harnesses that resume by session id", () => {
    for (const harness of ["claude-code", "open-code", "codex"] as HarnessId[]) {
      expect(canRerun(session({ key: { harness } })), harness).toEqual({ ok: true });
    }
  });

  it("refuses gemini, and says why", () => {
    // The whole point of disabling it rather than offering "latest": "latest"
    // is whichever conversation is newest in that directory, which is a
    // different session than the row the user clicked.
    const verdict = canRerun(session({ key: { harness: "gemini" } }));
    expect(verdict.ok).toBe(false);
    expect(verdict.ok === false && verdict.reason).toMatch(/latest/);
  });

  it("refuses antigravity, and says why", () => {
    const verdict = canRerun(session({ key: { harness: "antigravity" } }));
    expect(verdict.ok).toBe(false);
    expect(verdict.ok === false && verdict.reason).toMatch(/antigravity/);
  });

  it("covers every harness the app knows about", () => {
    // A new adapter added without a decision here would ship with a button
    // that either does nothing or does the wrong thing.
    for (const harness of ALL) {
      const verdict = canRerun(session({ key: { harness } }));
      expect(typeof verdict.ok, harness).toBe("boolean");
    }
    expect(ALL.filter(isResumable)).toEqual(["claude-code", "open-code", "codex"]);
  });

  it("needs a session id and a directory to mean anything", () => {
    expect(canRerun(session({ session_id: "" })).ok).toBe(false);
    // Whitespace is not an id; a path of spaces is not a directory.
    expect(canRerun(session({ session_id: "   " })).ok).toBe(false);
    expect(canRerun(session({ cwd: "" })).ok).toBe(false);
    expect(canRerun(session({ cwd: "  " })).ok).toBe(false);
  });

  it("names the unresumable harness even when the directory is also gone", () => {
    // The more useful of the two facts: "you can never reopen this" beats
    // "that folder moved" as the thing to tell someone.
    const verdict = canRerun(
      session({ key: { harness: "gemini" }, cwd: "", session_id: "" }),
    );
    expect(verdict.ok === false && verdict.reason).toMatch(/gemini-cli/);
  });
});

describe("rerunBlockedReason", () => {
  it("is null exactly when the row can be reopened", () => {
    // The two functions must not disagree, or the tooltip contradicts the
    // button's own disabled state.
    for (const harness of ALL) {
      const row = session({ key: { harness } });
      const allowed = canRerun(row).ok;
      expect(rerunBlockedReason(row) === null, harness).toBe(allowed);
    }
  });
});

describe("resumeCommand", () => {
  it("is the real command line, and null where there is none", () => {
    // Checked against each CLI's actual flag rather than written from memory:
    // the whole feature rests on these three strings being right.
    expect(resumeCommand("claude-code")).toBe("claude --resume");
    expect(resumeCommand("open-code")).toBe("opencode --session");
    expect(resumeCommand("codex")).toBe("codex resume");
    expect(resumeCommand("gemini")).toBeNull();
    expect(resumeCommand("antigravity")).toBeNull();
  });
});
