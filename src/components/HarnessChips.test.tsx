import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { HarnessChips } from "./HarnessChips";
import { session } from "../test/fixtures";
import { HARNESS_CODE, HARNESS_LABEL, type HarnessId } from "../types";

const ALL: HarnessId[] = ["claude-code", "open-code", "codex", "gemini", "antigravity"];

describe("HarnessChips", () => {
  it("shows only the harnesses that have sessions, collapsing the rest", () => {
    // Horizontally there is no room for five chips, and a harness with no
    // sessions is the least useful of them.
    render(
      <HarnessChips
        detected={ALL}
        sessions={[
          session({ key: { harness: "claude-code" } }),
          session({ key: { harness: "codex" } }),
        ]}
      />,
    );
    expect(screen.getByText(HARNESS_CODE["claude-code"])).toBeInTheDocument();
    expect(screen.getByText(HARNESS_CODE.codex)).toBeInTheDocument();
    expect(screen.queryByText(HARNESS_CODE.gemini)).not.toBeInTheDocument();
    expect(screen.getByTitle(/Detected but idle/)).toHaveTextContent("+3");
  });

  it("names the collapsed harnesses on hover", () => {
    // A "+3" that does not say which three is a number, not an affordance.
    render(<HarnessChips detected={ALL} sessions={[]} />);
    const title = screen.getByTitle(/Detected but idle/).getAttribute("title") ?? "";
    expect(title).toContain(HARNESS_LABEL["open-code"]);
    expect(title).toContain(HARNESS_LABEL.codex);
  });

  it("keeps every detected harness visible in the vertical strip", () => {
    // "codex is installed and quiet" and "codex is not here" are different
    // facts, and the vertical strip has the room to say both.
    render(<HarnessChips detected={ALL} sessions={[]} vertical />);
    for (const harness of ALL) {
      expect(screen.getByText(HARNESS_CODE[harness]), harness).toBeInTheDocument();
    }
    expect(screen.queryByTitle(/Detected but idle/)).not.toBeInTheDocument();
  });

  it("counts sessions per harness", () => {
    render(
      <HarnessChips
        detected={["claude-code", "codex"]}
        sessions={[
          session({ key: { harness: "claude-code" } }),
          session({ key: { harness: "claude-code" } }),
          session({ key: { harness: "codex" } }),
        ]}
      />,
    );
    expect(screen.getByTitle("Claude Code: 2 session(s)")).toBeInTheDocument();
    expect(screen.getByTitle("codex: 1 session(s)")).toBeInTheDocument();
  });

  it("says how many of a harness's sessions are waiting on you", () => {
    render(
      <HarnessChips
        detected={["claude-code"]}
        sessions={[
          session({ key: { harness: "claude-code" }, state: "awaiting-permission" }),
          session({ key: { harness: "claude-code" }, state: "running" }),
        ]}
      />,
    );
    expect(screen.getByTitle("Claude Code: 2 session(s), 1 waiting")).toBeInTheDocument();
  });

  it("marks a quiet harness differently from a busy one", () => {
    // With no hue left, the dot is the mark: hollow for installed-and-quiet,
    // solid for one with live sessions.
    // Read the dot out of the chip rather than off the whole container, so a
    // second harness cannot make the assertion pass for the wrong reason.
    const dotClass = (title: RegExp) => {
      const dot = screen.getByTitle(title).querySelector(".h-1\\.5");
      return dot?.getAttribute("class") ?? "";
    };

    const { unmount } = render(
      <HarnessChips detected={["codex"]} sessions={[]} vertical />,
    );
    expect(dotClass(/^codex: 0/)).toContain("border-black/30");
    expect(dotClass(/^codex: 0/)).not.toContain("bg-black");
    unmount();

    render(
      <HarnessChips
        detected={["codex"]}
        sessions={[session({ key: { harness: "codex" } })]}
        vertical
      />,
    );
    expect(dotClass(/^codex: 1/)).toContain("bg-black");
    expect(dotClass(/^codex: 1/)).not.toContain("border-black/30");
  });

  it("blinks only the harness that is waiting", () => {
    const { container } = render(
      <HarnessChips
        detected={["claude-code", "codex"]}
        sessions={[
          session({ key: { harness: "claude-code" }, state: "awaiting-input" }),
          session({ key: { harness: "codex" }, state: "running" }),
        ]}
      />,
    );
    // One blinking dot, not two: the blink is the alarm.
    expect(container.innerHTML.match(/hm-alert/g)).toHaveLength(1);
  });

  it("dims a detected-but-quiet harness", () => {
    // Installed-and-quiet is a fact worth showing, just quietly.
    render(
      <HarnessChips
        detected={["codex"]}
        sessions={[session({ key: { harness: "codex" } })]}
        vertical
      />,
    );
    expect(screen.getByTitle(/^codex: 1/)).not.toHaveClass("opacity-50");

    const { unmount } = render(
      <HarnessChips detected={["codex"]} sessions={[]} vertical />,
    );
    expect(screen.getByTitle(/^codex: 0/)).toHaveClass("opacity-50");
    unmount();
  });

  it("renders nothing recognisable when no harness is detected", () => {
    // A blank strip must not be mistaken for "no sessions running".
    const { container } = render(<HarnessChips detected={[]} sessions={[]} />);
    expect(container.textContent).toBe("");
  });
});
