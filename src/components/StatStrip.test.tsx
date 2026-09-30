import { describe, expect, it } from "vitest";
import { screen, within } from "@testing-library/react";
import { render } from "@testing-library/react";
import { StatStrip } from "./StatStrip";
import { session } from "../test/fixtures";
import type { SessionState } from "../types";

function stat(title: RegExp) {
  return screen.getByTitle(title);
}

/** The visible figure on a chip - the label lives in the tooltip unless vertical. */
function figure(title: RegExp) {
  return stat(title).textContent?.trim();
}

describe("StatStrip", () => {
  it("always shows all three counts, in the same order", () => {
    // Positional reading: the numbers must be readable without parsing labels,
    // so a chip that disappears shifts the other two out from under the eye.
    render(<StatStrip sessions={[session({ state: "running" })]} />);
    const chips = screen.getAllByTitle(/running|idle|waiting/);
    expect(chips.map((c) => c.getAttribute("title"))).toEqual([
      "1 running",
      "0 idle",
      "0 waiting for you",
    ]);
  });

  it("keeps a zero visible but dimmed", () => {
    // "nothing is waiting" is information, and a hidden chip would move the
    // other two sideways.
    const { container } = render(<StatStrip sessions={[]} />);
    const zeros = screen.getAllByTitle(/^0 /);
    expect(zeros).toHaveLength(3);
    for (const zero of zeros) expect(zero.className).toContain("opacity-40");
    expect(container).toBeTruthy();
  });

  it("counts only live sessions, never history", () => {
    // An ended row is display-only; counting it as running would be a lie.
    render(
      <StatStrip
        sessions={[
          session({ state: "running" }),
          session({ state: "ended" }),
          session({ state: "ended" }),
        ]}
      />,
    );
    expect(figure(/running$/)).toBe("1");
    expect(figure(/idle$/)).toBe("0");
    expect(figure(/waiting for you$/)).toBe("0");
  });

  it("folds activity it cannot classify into running", () => {
    render(
      <StatStrip
        sessions={[
          session({ state: "running" }),
          session({ state: "active-unknown" }),
          session({ state: "idle" }),
          session({ state: "shell" }),
          session({ state: "awaiting-input" }),
        ]}
      />,
    );
    expect(figure(/running$/)).toBe("2");
    expect(figure(/idle$/)).toBe("2");
    expect(figure(/waiting for you$/)).toBe("1");
  });

  it("counts both waiting states as waiting", () => {
    render(
      <StatStrip
        sessions={[
          session({ state: "awaiting-input" }),
          session({ state: "awaiting-permission" }),
        ]}
      />,
    );
    expect(figure(/waiting for you$/)).toBe("2");
  });

  it("spends solid ink on a waiting count and nothing else", () => {
    // The one chip on the pill allowed to be a stamp of ink: if running used it
    // too, a working session would shout as loudly as a blocked one.
    const { container } = render(
      <StatStrip sessions={[session({ state: "awaiting-permission" })]} />,
    );
    const loud = stat(/waiting for you$/);
    expect(loud.className).toContain("bg-black text-white");

    const running = stat(/running$/);
    expect(running.className).not.toContain("text-white ring-black");
    expect(container).toBeTruthy();
  });

  it("goes quiet when nothing is waiting", () => {
    render(<StatStrip sessions={[session({ state: "running" })]} />);
    const waiting = stat(/waiting for you$/);
    expect(waiting.className).not.toContain("bg-black text-white");
    expect(waiting.className).toContain("opacity-40");
  });

  it("drops the labels in the horizontal pill and keeps them in the strip", () => {
    // 440px has room for three numbers and not for three words; the tooltip
    // still names each one.
    const { container: flat } = render(<StatStrip sessions={[session({ state: "running" })]} />);
    expect(flat.textContent).not.toContain("waiting");
    expect(flat.textContent).not.toContain("idle");

    const { container: tall } = render(
      <StatStrip sessions={[session({ state: "running" })]} vertical />,
    );
    expect(tall.textContent).toContain("running");
    expect(tall.textContent).toContain("idle");
    expect(tall.textContent).toContain("waiting");
  });

  it("numbers the vertical strip as a column of figures", () => {
    const { container } = render(
      <StatStrip sessions={[session({ state: "idle" })]} vertical />,
    );
    const rows = container.firstElementChild?.children ?? [];
    expect(rows).toHaveLength(3);
    for (const row of Array.from(rows)) {
      expect(within(row as HTMLElement).getByText(/\d$/)).toBeInTheDocument();
    }
  });
});

describe("StatStrip over every state", () => {
  it("assigns each state to exactly one bucket", () => {
    // The three buckets must partition the states, or a count is silently lost.
    const buckets: Record<SessionState, "running" | "idle" | "waiting" | "none"> = {
      running: "running",
      "active-unknown": "running",
      idle: "idle",
      shell: "idle",
      "awaiting-input": "waiting",
      "awaiting-permission": "waiting",
      ended: "none",
    };
    for (const [state, bucket] of Object.entries(buckets)) {
      const { unmount } = render(<StatStrip sessions={[session({ state: state as SessionState })]} />);
      const expected = { running: 0, idle: 0, waiting: 0, none: 0 };
      if (bucket !== "none") expected[bucket] = 1;
      expect(figure(/running$/), state).toBe(String(expected.running));
      expect(figure(/idle$/), state).toBe(String(expected.idle));
      expect(figure(/waiting for you$/), state).toBe(String(expected.waiting));
      unmount();
    }
  });
});
