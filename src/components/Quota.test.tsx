import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { Quota } from "./Quota";
import { NOW, quotaSnapshot } from "../test/fixtures";
import { QUOTA_STALE_MS } from "../types";
import { TONE } from "../test/render";

function ring(container: HTMLElement, which: "track" | "arc") {
  const circles = container.querySelectorAll("circle");
  return which === "track" ? circles[0] : circles[1];
}

function renderQuota(props: Partial<Parameters<typeof Quota>[0]> = {}, quota = quotaSnapshot({ at: NOW })) {
  const utils = render(
    <Quota quota={quota} now={NOW} tone={TONE} {...props} />,
  );
  return { ...utils, container: utils.container };
}

/** The window div that carries the full sentence, ring geometry aside. */
function windowTitle(container: HTMLElement) {
  return container.querySelector("[title]")?.getAttribute("title") ?? "";
}

describe("Quota", () => {
  it("waits for the first snapshot rather than claiming there is no plan window", () => {
    // Three distinct states; conflating the first two made a working install
    // look broken.
    render(<Quota quota={null} now={NOW} tone={TONE} pending />);
    expect(screen.getByTitle("Waiting for the first snapshot")).toHaveTextContent("--");
    expect(screen.queryByText("no quota yet")).not.toBeInTheDocument();
  });

  it("explains an absent reading instead of drawing an empty ring", () => {
    render(<Quota quota={null} now={NOW} tone={TONE} />);
    const label = screen.getByText("no quota yet");
    expect(label).toBeInTheDocument();
    expect(label.getAttribute("title")).toContain("statusline shim");
  });

  it("treats a missing five-hour figure as no reading at all", () => {
    // A ring around nothing is decoration, not information.
    render(
      <Quota quota={quotaSnapshot({ five_hour_pct: null })} now={NOW} tone={TONE} />,
    );
    expect(screen.getByText("no quota yet")).toBeInTheDocument();
  });

  it("shows both windows as percentages, not bare numbers", () => {
    // "52" alone reads as a count; the ring carries the proportion.
    const { container } = renderQuota();
    expect(screen.getByText("5h 64%")).toBeInTheDocument();
    expect(screen.getByText("7d 34%")).toBeInTheDocument();
    expect(ring(container, "arc")).toBeInTheDocument();
  });

  it("omits the seven-day ring when there is no seven-day figure", () => {
    render(
      <Quota quota={quotaSnapshot({ seven_day_pct: null })} now={NOW} tone={TONE} />,
    );
    expect(screen.getByText("5h 64%")).toBeInTheDocument();
    expect(screen.queryByText(/^7d/)).not.toBeInTheDocument();
  });

  it("weights the arc by the percentage, clamped to 100", () => {
    const { container } = renderQuota({}, quotaSnapshot({ five_hour_pct: 50, seven_day_pct: null }));
    const arc = ring(container, "arc");
    const dash = arc.getAttribute("stroke-dasharray") ?? "";
    // "half the circumference, then the whole circumference" - the second
    // number is the track behind it.
    const [filled, full] = dash.split(" ");
    expect(Number(filled)).toBeGreaterThan(0);
    expect(Number(full)).toBeGreaterThan(Number(filled));

    const { container: over } = renderQuota(
      {},
      quotaSnapshot({ five_hour_pct: 140, seven_day_pct: null }),
    );
    const clamped = ring(over, "arc").getAttribute("stroke-dasharray") ?? "";
    const [filledOver, fullOver] = clamped.split(" ");
    expect(Number(fullOver)).toBeCloseTo(Number(full), 5);
    expect(Number(filledOver)).toBeLessThanOrEqual(Number(fullOver));
  });

  it("steps the stroke up as the window empties, and never past full ink", () => {
    const strokeAt = (pct: number) => {
      const { container } = renderQuota(
        {},
        quotaSnapshot({ at: NOW, five_hour_pct: pct, seven_day_pct: null }),
      );
      return ring(container, "arc").getAttribute("class") ?? "";
    };
    const plenty = strokeAt(20);
    const middling = strokeAt(70);
    const nearly = strokeAt(90);
    expect(plenty).toContain("stroke-black/50");
    expect(middling).toContain("stroke-black/80");
    expect(nearly).toContain("stroke-black");
  });

  it("keeps a stale reading on the faint stroke whatever it says", () => {
    // A reading that froze is uncertain, so it is printed as uncertain even
    // when the last number it gave was high.
    const { container } = renderQuota(
      {},
      quotaSnapshot({
        at: NOW - QUOTA_STALE_MS - 1,
        five_hour_pct: 90,
        seven_day_pct: null,
      }),
    );
    const arc = ring(container, "arc").getAttribute("class") ?? "";
    expect(arc).toContain("stroke-black/30");
    expect(arc).not.toContain("stroke-black dark");
  });

  it("puts the reset clock and the countdown in the tooltip", () => {
    // A countdown answers "how long", a clock answers "when I can start again",
    // and the second one is what you plan around.
    const { container } = renderQuota(
      {},
      quotaSnapshot({
        at: NOW,
        five_hour_resets_at: new Date(NOW + 97 * 60_000).toISOString(),
        five_hour_pct: 64,
        seven_day_pct: null,
      }),
    );
    expect(windowTitle(container)).toContain("resets");
    expect(windowTitle(container)).toContain("1h 37m left");
  });

  it("says the reading came from where it came from", () => {
    const { container } = renderQuota();
    expect(windowTitle(container)).toContain("statusline");
  });

  it("reminds the reader that only Claude has a plan window", () => {
    // The rings look comparable across harnesses, and they are not.
    const { container } = renderQuota();
    expect(windowTitle(container)).toContain("Other harnesses have no plan window");
  });

  it("greys out and reports the age rather than extrapolating forward", () => {
    // A confidently wrong percentage is worse than an absent one.
    const stale = quotaSnapshot({ at: NOW - QUOTA_STALE_MS - 60_000, seven_day_pct: null });
    const { container } = renderQuota({}, stale);
    const shell = container.firstElementChild as HTMLElement;
    expect(shell.className).toContain("opacity-50");
    expect(windowTitle(container)).toContain("the source went quiet");
  });

  it("does not grey a reading that is exactly at the staleness edge", () => {
    const fresh = quotaSnapshot({ at: NOW - QUOTA_STALE_MS });
    const { container } = renderQuota({}, fresh);
    expect((container.firstElementChild as HTMLElement).className).not.toContain("opacity-50");
  });

  it("still shows the figure on a stale reading, just quietly", () => {
    const stale = quotaSnapshot({ at: NOW - QUOTA_STALE_MS - 60_000, seven_day_pct: null });
    renderQuota({}, stale);
    expect(screen.getByText("5h 64%")).toBeInTheDocument();
  });

  it("stacks the two windows in the vertical strip and sits them side by side otherwise", () => {
    const { container: flat } = renderQuota();
    const { container: tall } = renderQuota({ stack: true });
    expect((flat.firstElementChild as HTMLElement).className).toContain("items-center");
    expect((tall.firstElementChild as HTMLElement).className).toContain("flex-col");
  });

  it("drops the reset clock in the one-line bar, where there is no second line", () => {
    // It is the same fact the tooltip carries, and 44px has no room for it.
    renderQuota({ inline: true });
    expect(screen.queryByText(/↻/)).not.toBeInTheDocument();
  });

  it("shrinks the ring in the one-line bar rather than wrapping the bar taller", () => {
    const { container: normal } = renderQuota();
    const { container: thin } = renderQuota({ inline: true });
    expect(ring(normal, "arc").getAttribute("r")).toBe("11");
    expect(ring(thin, "arc").getAttribute("r")).toBe("9");
  });

  it("leaves the widget draggable, since the pill is one drag region", () => {
    const { container } = renderQuota();
    expect(container.querySelector("[data-tauri-drag-region]")).toBeInTheDocument();
  });
});
