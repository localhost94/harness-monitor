import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { StateChip, motionFor } from "./StateBadge";
import { ALL_STATES } from "../test/fixtures";
import { STATE_LABEL, STATE_STYLE } from "../lib/format";

describe("StateChip", () => {
  it("names the state, so colour is never the only signal", () => {
    for (const state of ALL_STATES) {
      const { unmount } = render(<StateChip state={state} />);
      expect(screen.getByText(STATE_LABEL[state]), `label for ${state}`).toBeInTheDocument();
      unmount();
    }
  });

  it("prefers the harness-reported reason over the generic label", () => {
    // "permission prompt" tells you what to click; "needs approval" does not.
    render(<StateChip state="awaiting-permission" reason="permission prompt" />);
    const chip = screen.getByText("permission prompt");
    expect(chip).toBeInTheDocument();
    expect(screen.queryByText(STATE_LABEL["awaiting-permission"])).not.toBeInTheDocument();
  });

  it("falls back to the label when there is no reason", () => {
    render(<StateChip state="awaiting-input" reason={null} />);
    expect(screen.getByText(STATE_LABEL["awaiting-input"])).toBeInTheDocument();
  });

  it("carries the reason in the tooltip, whether or not it is the visible text", () => {
    const { unmount } = render(<StateChip state="running" />);
    expect(screen.getByTitle(STATE_LABEL.running)).toBeInTheDocument();
    unmount();

    render(<StateChip state="running" reason="streaming" />);
    expect(screen.getByTitle("streaming")).toBeInTheDocument();
  });

  it("applies the state's own chip skin", () => {
    for (const state of ALL_STATES) {
      const { container, unmount } = render(<StateChip state={state} />);
      const chip = container.querySelector("span");
      expect(chip?.className, `style for ${state}`).toContain(STATE_STYLE[state]);
      unmount();
    }
  });

  it("gives every state a distinct glyph, so two states are never confused", () => {
    // Rendered shape, not class: the glyph is the non-colour channel.
    const shapes = new Set<string>();
    for (const state of ALL_STATES) {
      const { container, unmount } = render(<StateChip state={state} />);
      shapes.add(container.querySelector("svg")?.innerHTML ?? "");
      unmount();
    }
    expect(shapes.size).toBe(ALL_STATES.length);
  });
});

describe("motionFor", () => {
  it("animates only the states that are actually in motion", () => {
    // Running breathes, waiting blinks, and everything else sits still.
    expect(motionFor("running")).toBe("hm-breathe");
    expect(motionFor("active-unknown")).toBe("hm-breathe");
    expect(motionFor("awaiting-input")).toBe("hm-alert");
    expect(motionFor("awaiting-permission")).toBe("hm-alert");
    expect(motionFor("idle")).toBe("");
    expect(motionFor("shell")).toBe("");
    expect(motionFor("ended")).toBe("");
  });

  it("does not animate an ended row", () => {
    // A photograph of a conversation should not look like it is happening.
    expect(motionFor("ended")).toBe("");
  });
});
