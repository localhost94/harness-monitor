import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import { SessionList } from "./SessionList";
import { NOW, session, snapshot } from "../test/fixtures";
import { renderWithStore, userEvent } from "../test/render";
import { useMonitor } from "../store/useMonitor";
import { HARNESS_CODE, HARNESS_LABEL, type Snapshot } from "../types";

function list(snap: Snapshot | null = snapshot(), store: Record<string, unknown> = {}) {
  return renderWithStore(<SessionList />, { now: NOW, startedAt: NOW, snapshot: snap, ...store });
}

describe("SessionList", () => {
  describe("before the first snapshot", () => {
    it("waits, without accusing anyone of a problem", () => {
      list(null, { receivedAt: null, now: NOW, startedAt: NOW });
      expect(screen.getByText(/waiting for first snapshot/)).toBeInTheDocument();
    });

    it("names the three things to check once waiting becomes a problem", () => {
      // The Windows build reads from an agent it launches inside WSL, and
      // there are no logs to look at by default.
      list(null, { now: NOW + 13_000, startedAt: NOW });
      expect(screen.getByText("HM_AGENT_PATH")).toBeInTheDocument();
      expect(screen.getByText("HM_WSL_DISTRO")).toBeInTheDocument();
      expect(screen.getByText("HM_LOG_FILE")).toBeInTheDocument();
    });

    it("stays quiet just before the threshold", () => {
      // Twelve seconds of silence is normal; calling it broken is not.
      list(null, { now: NOW + 11_000, startedAt: NOW });
      expect(screen.queryByText("HM_AGENT_PATH")).not.toBeInTheDocument();
    });
  });

  describe("tabs", () => {
    it("counts live and finished separately", () => {
      list(
        snapshot({
          sessions: [session(), session({ state: "idle" })],
          ended: [session({ state: "ended" })],
        }),
      );
      expect(screen.getByRole("button", { name: /live/ })).toHaveTextContent("live2");
      expect(screen.getByRole("button", { name: /finished/ })).toHaveTextContent("finished1");
    });

    it("shows the live list first", () => {
      list(snapshot({ sessions: [session({ name: "live-one" })] }));
      expect(screen.getByText("live-one")).toBeInTheDocument();
    });

    it("switches to history on the second tab", async () => {
      const user = userEvent.setup();
      list(
        snapshot({
          sessions: [session({ name: "live-one" })],
          ended: [session({ name: "old-one", state: "ended" })],
        }),
      );
      await user.click(screen.getByRole("button", { name: /finished/ }));
      expect(screen.getByText("old-one")).toBeInTheDocument();
      expect(screen.queryByText("live-one")).not.toBeInTheDocument();
    });

    it("marks the active tab for assistive tech, not by colour alone", () => {
      list();
      expect(screen.getByRole("button", { name: /live/ })).toHaveAttribute("aria-pressed", "true");
      expect(screen.getByRole("button", { name: /finished/ })).toHaveAttribute(
        "aria-pressed",
        "false",
      );
    });
  });

  describe("the live list", () => {
    it("groups by harness and names each group", () => {
      list(
        snapshot({
          detected: ["claude-code", "codex"],
          sessions: [
            session({ key: { harness: "claude-code" } }),
            session({ key: { harness: "codex" } }),
          ],
        }),
      );
      expect(screen.getByText(HARNESS_LABEL["claude-code"])).toBeInTheDocument();
      expect(screen.getByText(HARNESS_LABEL.codex)).toBeInTheDocument();
    });

    it("keeps a detected-but-quiet harness visible", () => {
      // "codex is installed and quiet" and "codex is not here" are different
      // facts, and the group is what distinguishes them.
      const { container } = list(snapshot({ detected: ["claude-code", "codex"], sessions: [] }));
      // One per quiet harness - the group header is what says which.
      expect(screen.getAllByText("no live sessions")).toHaveLength(2);
      expect(container.innerHTML).toContain("no live sessions");
      expect(screen.getByText("codex")).toBeInTheDocument();
    });

    it("puts the harness waiting on you at the top of the list", () => {
      // With a dozen sessions the list scrolls, so what needs you cannot be
      // left wherever its harness happens to sort.
      list(
        snapshot({
          detected: ["claude-code", "codex", "gemini"],
          sessions: [
            session({ key: { harness: "codex" }, state: "running" }),
            session({ key: { harness: "gemini" }, state: "idle" }),
            session({ key: { harness: "claude-code" }, state: "awaiting-permission" }),
          ],
        }),
      );
      const headings = screen.getAllByRole("heading", { level: 2 });
      expect(headings[0]?.textContent).toContain(HARNESS_LABEL["claude-code"]);
    });

    it("puts the row that needs you above the running one inside a group", () => {
      list(
        snapshot({
          detected: ["claude-code"],
          sessions: [
            session({ key: { harness: "claude-code" }, name: "busy-one", state: "running" }),
            session({
              key: { harness: "claude-code" },
              name: "waiting-one",
              state: "awaiting-permission",
            }),
          ],
        }),
      );
      const names = screen
        .getAllByText(/busy-one|waiting-one/)
        .map((n) => n.textContent);
      expect(names.indexOf("waiting-one")).toBeLessThan(names.indexOf("busy-one"));
    });

    it("filters as you type and says how much it hid", async () => {
      const user = userEvent.setup();
      list(
        snapshot({
          detected: ["claude-code", "codex"],
          sessions: [
            session({ key: { harness: "claude-code" }, name: "migrations" }),
            session({ key: { harness: "codex" }, name: "unrelated" }),
          ],
        }),
      );
      await user.type(screen.getByLabelText("Filter live sessions"), "migrat");
      expect(screen.getByText("1 of 2 live sessions")).toBeInTheDocument();
      expect(screen.queryByText("unrelated")).not.toBeInTheDocument();
    });

    it("drops the groups that did not match, rather than printing nothing under each", async () => {
      // Four harnesses each saying "no live sessions here" is an answer made of
      // noise.
      const user = userEvent.setup();
      list(
        snapshot({
          detected: ["claude-code", "codex", "gemini", "antigravity"],
          sessions: [
            session({ key: { harness: "claude-code" }, name: "migrations" }),
            session({ key: { harness: "codex" }, name: "unrelated" }),
            session({ key: { harness: "gemini" }, name: "unrelated" }),
            session({ key: { harness: "antigravity" }, name: "unrelated" }),
          ],
        }),
      );
      await user.type(screen.getByLabelText("Filter live sessions"), "migrat");
      expect(screen.queryByText(HARNESS_LABEL.codex)).not.toBeInTheDocument();
      expect(screen.queryByText("no live sessions")).not.toBeInTheDocument();
    });

    it("says when nothing matched, and quotes what it looked for", async () => {
      const user = userEvent.setup();
      list(snapshot({ sessions: [session({ name: "migrations" })] }));
      await user.type(screen.getByLabelText("Filter live sessions"), "zzz");
      expect(screen.getByText(/no live session matches “zzz”/)).toBeInTheDocument();
    });

    it("clears the search from the panel", async () => {
      const user = userEvent.setup();
      list(snapshot({ sessions: [session({ name: "migrations" })] }), { query: "migrat" });
      await user.click(screen.getByLabelText("Clear search"));
      expect(useMonitor.getState().query).toBe("");
    });

    it("hides the clear button when there is nothing to clear", () => {
      list(snapshot());
      expect(screen.queryByLabelText("Clear search")).not.toBeInTheDocument();
    });

    it("marks the search box out of the drag region", () => {
      // A click in the gap around it would otherwise drag the window instead
      // of doing nothing.
      const { container } = list(snapshot());
      expect(container.querySelector("[data-no-drag]")).toBeInTheDocument();
    });

    it("explains itself when no harness data was found at all", () => {
      list(snapshot({ detected: [] }));
      expect(screen.getByText(/no harness data found/)).toBeInTheDocument();
    });

    it("says the rings are Claude's alone when another harness is present", () => {
      // Nothing else has a plan window, and implying the numbers compare is
      // worse than saying they do not.
      const { container } = list(snapshot({ detected: ["claude-code", "codex"] }));
      expect(container.innerHTML).toContain("only Claude Code reports one");
    });

    it("says nothing of the sort when Claude Code is the only harness", () => {
      const { container } = list(snapshot({ detected: ["claude-code"] }));
      expect(container.innerHTML).not.toContain("only Claude Code reports one");
    });
  });

  describe("a live row", () => {
    it("prints the token headline and not the cache reads that would swamp it", () => {
      const { container } = list(
        snapshot({
          sessions: [
            session({
              tokens: {
                input: 1_386,
                output: 951_739,
                reasoning: 0,
                cache_read: 242_058_096,
                cache_write: 51_204,
              },
            }),
          ],
        }),
      );
      // On the row and again in the group total; neither may quote the cache.
      expect(screen.getAllByText(/953k tok/)).toHaveLength(2);
      expect(container.innerHTML).not.toContain("242M");
    });

    it("keeps the full breakdown in the tooltip", () => {
      list(
        snapshot({
          sessions: [
            session({
              tokens: {
                input: 1_386,
                output: 951_739,
                reasoning: 0,
                cache_read: 242_058_096,
                cache_write: 0,
              },
            }),
          ],
        }),
      );
      expect(
        screen.getByTitle(/cache read 242,058,096/),
      ).toBeInTheDocument();
    });

    it("labels the harness on the row, because that is what you act on", () => {
      list(snapshot({ detected: ["claude-code"], sessions: [session()] }));
      const chip = screen.getByTitle(HARNESS_LABEL["claude-code"]);
      expect(chip).toHaveTextContent(HARNESS_CODE["claude-code"]);
    });

    it("strikes the harness chip solid when it is waiting on you", () => {
      const { container } = list(
        snapshot({
          detected: ["claude-code"],
          sessions: [session({ state: "awaiting-permission", waiting_for: "permission prompt" })],
        }),
      );
      expect(container.innerHTML).toContain("bg-black text-white");
    });

    it("prints the reason it is waiting, which is what to click", () => {
      list(
        snapshot({
          detected: ["claude-code"],
          sessions: [session({ state: "awaiting-permission", waiting_for: "permission prompt" })],
        }),
      );
      expect(screen.getByText("permission prompt")).toBeInTheDocument();
    });

    it("labels a background session, which should not be mistaken for focus", () => {
      list(
        snapshot({
          detected: ["claude-code"],
          sessions: [session({ is_background: true })],
        }),
      );
      expect(screen.getByText("bg")).toBeInTheDocument();
    });

    it("shows the terminal tab title when the host knows it", () => {
      list(
        snapshot({
          detected: ["claude-code"],
          sessions: [session({ terminal_title: "◑ harness-monitor" })],
        }),
      );
      expect(screen.getByText("◑ harness-monitor")).toBeInTheDocument();
    });

    it("adds up a harness's tokens and cost in its group header", () => {
      const { container } = list(
        snapshot({
          detected: ["claude-code"],
          sessions: [
            session({
              tokens: { input: 1_000, output: 1_000, reasoning: 0, cache_read: 0, cache_write: 0 },
            }),
            session({
              tokens: { input: 1_000, output: 1_000, reasoning: 0, cache_read: 0, cache_write: 0 },
              cost: 3,
            }),
          ],
        }),
      );
      // Claude Code reports no per-session cost, but another harness may, and a
      // blank or a zero there would read as "free".
      expect(screen.getByText("4k tok")).toBeInTheDocument();
      expect(container.innerHTML).toContain("$3.00");
    });

    it("prints no totals for a harness that reports neither", () => {
      const { container } = list(
        snapshot({
          detected: ["antigravity"],
          sessions: [session({ key: { harness: "antigravity" }, tokens: null })],
        }),
      );
      // Matched on a figure, not the bare word: the footnote about Claude's
      // rings contains "tokens" and would satisfy a looser check.
      expect(container.innerHTML).not.toMatch(/[\d.]+k? tok/);
      expect(container.innerHTML).not.toMatch(/\$\d/);
    });

    it("says an inferred state was inferred", () => {
      // A state read from an mtime must never be mistaken for a reported one.
      list(
        snapshot({
          detected: ["open-code"],
          sessions: [session({ key: { harness: "open-code" }, tier: "usage-only" })],
        }),
      );
      expect(screen.getByText(/state inferred from recency/)).toBeInTheDocument();
    });

    it("says a presence-only harness knows nothing but that something happened", () => {
      list(
        snapshot({
          detected: ["antigravity"],
          sessions: [session({ key: { harness: "antigravity" }, tier: "presence-only" })],
        }),
      );
      expect(screen.getByText(/activity only/)).toBeInTheDocument();
    });

    it("prints no note for a harness that reports its own state", () => {
      list(
        snapshot({
          detected: ["claude-code"],
          sessions: [session({ tier: "full" })],
        }),
      );
      expect(screen.queryByText(/state inferred/)).not.toBeInTheDocument();
      expect(screen.queryByText(/activity only/)).not.toBeInTheDocument();
    });
  });

  describe("jumping to a session", () => {
    it("jumps to the pane herdr reported", async () => {
      const user = userEvent.setup();
      list(
        snapshot({
          detected: ["claude-code"],
          sessions: [session({ jump_target: "wA:p1" })],
        }),
      );
      await user.click(screen.getByTitle(/Jump to this session/));
      expect(useMonitor.getState().error).toBeNull();
    });

    it("disables the jump when nothing on this host knows where the session is", () => {
      list(snapshot({ detected: ["claude-code"], sessions: [session({ jump_target: null })] }));
      const button = screen.getByTitle(/No jump target/);
      expect(button).toBeDisabled();
    });
  });

  describe("settings", () => {
    it("is reached through a gear in the panel header, which is the only way in", () => {
      // The pill's four buttons sit in a 2x2 that 80px of height cannot grow,
      // so the gear is not on the pill at all.
      const user = userEvent.setup();
      list();
      const gear = screen.getByRole("button", { name: "Settings" });
      expect(gear).toHaveAttribute("aria-expanded", "false");
      expect(screen.queryByRole("dialog", { name: "Settings" })).not.toBeInTheDocument();

      return user.click(gear).then(() => {
        expect(screen.getByRole("dialog", { name: "Settings" })).toBeInTheDocument();
        expect(gear).toHaveAttribute("aria-expanded", "true");
      });
    });

    it("is not a one-way door", async () => {
      const user = userEvent.setup();
      list();
      await user.click(screen.getByRole("button", { name: "Settings" }));
      await user.click(screen.getByRole("button", { name: "Settings" }));
      expect(screen.queryByRole("dialog", { name: "Settings" })).not.toBeInTheDocument();
    });

    it("offers settings on the finished tab too, where the reopen button lives", () => {
      // A feature split across two views, reachable from only one of them, is a
      // feature half the people using it cannot find.
      list(snapshot({ ended: [session({ state: "ended" })] }), { view: "finished" });
      expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
    });
  });

  describe("the error footer", () => {
    it("shows a failed action, because a silent no-op looks like a dead button", () => {
      list(snapshot(), { error: "Could not jump: no such pane" });
      expect(screen.getByText("Could not jump: no such pane")).toBeInTheDocument();
    });

    it("shows nothing when there is no error", () => {
      const { container } = list(snapshot(), { error: null });
      expect(container.innerHTML).not.toContain("Could not");
    });

    it("stays readable with the settings sheet open over it", () => {
      // A settings write that fails reports here and nowhere else, so the sheet
      // that caused the failure must not be what hides the explanation.
      list(snapshot(), { error: "Could not save settings: read-only file system" });
      const footer = screen.getByText(/read-only file system/);
      expect(footer.className).toMatch(/z-30/);
    });
  });
});
