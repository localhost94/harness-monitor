import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import { Pill } from "./Pill";
import { NOW, quotaSnapshot, session, snapshot } from "../test/fixtures";
import { renderWithStore, userEvent } from "../test/render";
import { useMonitor } from "../store/useMonitor";
import { surfaceFor } from "../lib/theme";
import type { AgentSession } from "../types";

function pill(sessions: AgentSession[] = [], store: Record<string, unknown> = {}) {
  const snap = snapshot({
    detected: ["claude-code", "open-code", "codex"],
    sessions,
    quota: quotaSnapshot({ at: NOW }),
  });
  return renderWithStore(<Pill />, { now: NOW, snapshot: snap, ...store });
}

describe("Pill", () => {
  describe("the headline", () => {
    it("answers the only question the widget exists to answer", () => {
      // Does anything want me right now? Everything else on the pill is detail.
      pill([session({ state: "awaiting-permission" })]);
      expect(screen.getByText("1 needs you")).toBeInTheDocument();
    });

    it("agrees with itself about the plural", () => {
      pill([
        session({ state: "awaiting-input" }),
        session({ state: "awaiting-permission" }),
      ]);
      expect(screen.getByText("2 need you")).toBeInTheDocument();
    });

    it("counts both waiting states, not just the prompts", () => {
      pill([
        session({ state: "awaiting-input" }),
        session({ state: "awaiting-permission" }),
        session({ state: "running" }),
      ]);
      expect(screen.getByText("2 need you")).toBeInTheDocument();
    });

    it("reports running work when nothing is waiting", () => {
      pill([session({ state: "running" }), session({ state: "active-unknown" })]);
      expect(screen.getByText("2 running")).toBeInTheDocument();
    });

    it("says everything present is idle, rather than showing a bare zero", () => {
      pill([session({ state: "idle" }), session({ state: "shell" })]);
      expect(screen.getByText("all idle")).toBeInTheDocument();
    });

    it("says there is nothing at all, with no snapshot yet", () => {
      pill([]);
      expect(screen.getByText("no sessions")).toBeInTheDocument();
    });

    it("never counts history, so a month-old row cannot outshout live work", () => {
      renderWithStore(<Pill />, {
        now: NOW,
        snapshot: snapshot({ sessions: [session({ state: "running" })], ended: [session({ state: "ended" })] }),
      });
      expect(screen.getByText("1 running")).toBeInTheDocument();
    });

    it("survives a null snapshot rather than rendering nothing", () => {
      renderWithStore(<Pill />, { now: NOW, snapshot: null });
      expect(screen.getByText("no sessions")).toBeInTheDocument();
    });
  });

  describe("the surface", () => {
    it("rings the pill when something is waiting on the user", () => {
      const { container } = pill([session({ state: "awaiting-permission" })]);
      const shell = container.firstElementChild as HTMLElement;
      expect(shell.className).toContain(surfaceFor("permission").ring);
    });

    it("does not ring the pill for work in progress", () => {
      const { container } = pill([session({ state: "running" })]);
      expect((container.firstElementChild as HTMLElement).className).not.toContain("ring-2");
    });

    it("does not ring the pill when everything is idle", () => {
      const { container } = pill([session({ state: "idle" })]);
      expect((container.firstElementChild as HTMLElement).className).not.toContain("ring-2");
    });

    it("animates the headline glyph only while there is motion", () => {
      const { container: busy } = pill([session({ state: "running" })]);
      expect(busy.innerHTML).toContain("hm-breathe");

      const { container: waiting } = pill([session({ state: "awaiting-input" })]);
      expect(waiting.innerHTML).toContain("hm-alert");

      const { container: still } = pill([session({ state: "idle" })]);
      expect(still.innerHTML).not.toContain("hm-breathe");
      expect(still.innerHTML).not.toContain("hm-alert");
    });

    it("shows the reason the top session is waiting", () => {
      pill([session({ state: "awaiting-permission", waiting_for: "permission prompt" })]);
      expect(screen.getByText("permission prompt")).toBeInTheDocument();
    });

    it("falls back to a generic reason when the harness gave none", () => {
      pill([session({ state: "awaiting-input", waiting_for: null })]);
      expect(screen.getByText("waiting")).toBeInTheDocument();
    });
  });

  describe("the controls", () => {
    it("expands and collapses the panel", async () => {
      const user = userEvent.setup();
      pill();
      await user.click(screen.getByTitle("Show sessions"));
      expect(useMonitor.getState().expanded).toBe(true);

      await user.click(screen.getByTitle("Collapse"));
      expect(useMonitor.getState().expanded).toBe(false);
    });

    it("mutes and unmutes", async () => {
      const user = userEvent.setup();
      pill();
      await user.click(screen.getByTitle("Mute notifications"));
      expect(useMonitor.getState().muted).toBe(true);

      await user.click(screen.getByTitle("Notifications muted - click to unmute"));
      expect(useMonitor.getState().muted).toBe(false);
    });

    it("cycles through all three shapes", async () => {
      const user = userEvent.setup();
      pill();
      await user.click(screen.getByTitle(/^Shape: pill/));
      expect(useMonitor.getState().shape).toBe("line");
      await user.click(screen.getByTitle(/^Shape: one line/));
      expect(useMonitor.getState().shape).toBe("vertical");
      await user.click(screen.getByTitle(/^Shape: vertical/));
      expect(useMonitor.getState().shape).toBe("pill");
    });

    it("names where the shape button lands before it is pressed", () => {
      // The glyph shows the destination, not the current shape.
      pill();
      expect(screen.getByTitle("Shape: pill — click for one line")).toBeInTheDocument();
    });

    it("switches theme", async () => {
      const user = userEvent.setup();
      pill();
      await user.click(screen.getByTitle("Switch light / dark"));
      expect(useMonitor.getState().theme).toBe("dark");
    });
  });

  describe("the shapes", () => {
    it("puts the counts in the pill", () => {
      // The pill is 80px tall, which fits a 2x2 button grid and one text row.
      const { container } = pill([session({ state: "running" })]);
      expect((container.firstElementChild as HTMLElement).className).toContain("h-[80px]");
    });

    it("drops the count strip in the one-line bar", () => {
      // At 440px the pager and four buttons leave the middle column about
      // 120px, and three count chips need more than twice that - which is why
      // they overlapped instead of fitting.
      const { container } = pill([session({ state: "running" })], { shape: "line" });
      const shell = container.firstElementChild as HTMLElement;
      expect(shell.className).toContain("h-[44px]");
      // The chips are gone; the counts survive in the headline's tooltip, which
      // is the only place the one-line shape can still carry them.
      expect(screen.queryByTitle("0 waiting for you")).not.toBeInTheDocument();
      expect(screen.getByText("1 running")).toBeInTheDocument();
      expect(screen.getByTitle(/0 idle, 0 waiting for you/)).toBeInTheDocument();
    });

    it("keeps both quota rings in the one-line bar", () => {
      // The one thing a shorter widget could have done is hide data, and the
      // rings are the only place the plan window appears at all.
      const { container } = pill([], { shape: "line" });
      expect(container.innerHTML).toContain("5h ");
      expect(container.innerHTML).toContain("7d ");
    });

    it("stacks the counts in the vertical strip", () => {
      const { container } = pill([session({ state: "running" })], { shape: "vertical" });
      const shell = container.firstElementChild as HTMLElement;
      expect(shell.className).toContain("grid-rows-[5px_auto_auto_auto_auto]");
      expect(shell.innerHTML).toContain("waiting for you");
    });

    it("gives the vertical strip the full height of the window", () => {
      const { container } = pill([], { shape: "vertical" });
      expect((container.firstElementChild as HTMLElement).className).toContain("h-full");
    });

    it("prefers the panel layout once expanded, whatever the shape was", () => {
      // Expanding is the user asking for the list, not for a taller strip.
      const { container } = pill([session({ state: "running" })], {
        shape: "vertical",
        expanded: true,
      });
      expect((container.firstElementChild as HTMLElement).className).toContain("h-[80px]");
    });

    it("keeps the same headline in every shape", () => {
      // Counts below are detail, and detail without a headline reads as trivia.
      const sessions = [session({ state: "awaiting-input" })];
      for (const shape of ["pill", "line", "vertical"] as const) {
        const { unmount } = pill(sessions, { shape });
        expect(screen.getByText("1 needs you"), shape).toBeInTheDocument();
        unmount();
      }
    });
  });

  it("declares itself a drag zone, since the whole widget moves", () => {
    const { container } = pill();
    expect(container.firstElementChild).toHaveAttribute("data-drag-zone");
  });

  it("pages through harness usage from the pill", async () => {
    const user = userEvent.setup();
    pill([session({ state: "running" })]);
    await user.click(screen.getByTitle("Next harness"));
    expect(useMonitor.getState().usageHarness).toBe("open-code");
  });
});
