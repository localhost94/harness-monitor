import { describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import { UsagePager } from "./UsagePager";
import { NOW, quotaSnapshot, session, snapshot } from "../test/fixtures";
import { renderWithStore, TONE, userEvent } from "../test/render";
import { useMonitor } from "../store/useMonitor";
import { HARNESS_CODE } from "../types";

const THREE = ["claude-code", "open-code", "codex"] as const;

/** The usage block's own tooltip, as opposed to the arrows' or the code's. */
function usageTitle(container: HTMLElement) {
  const titled = Array.from(
    container.querySelectorAll<HTMLElement>("[data-tauri-drag-region][title]"),
  );
  const usage = titled.find((el) => !/^Usage for/.test(el.getAttribute("title") ?? ""));
  return usage?.getAttribute("title") ?? "";
}

function pager(snap = snapshot({ detected: [...THREE] }), state = {}) {
  return renderWithStore(<UsagePager snapshot={snap} now={NOW} tone={TONE} />, {
    now: NOW,
    snapshot: snap,
    ...state,
  });
}

describe("UsagePager", () => {
  it("shows the plan window for Claude Code", () => {
    pager(snapshot({ detected: [...THREE], quota: quotaSnapshot({ at: NOW }) }));
    expect(screen.getByText(/^5h /)).toBeInTheDocument();
    expect(screen.getByText(/^7d /)).toBeInTheDocument();
  });

  it("shows tokens for a harness that bills per token instead", () => {
    // Paging rather than blending keeps a plan percentage and a token count
    // from ever being read as comparable.
    pager(
      snapshot({
        detected: [...THREE],
        sessions: [
          session({
            key: { harness: "open-code" },
            tokens: { input: 1_000, output: 2_000, reasoning: 0, cache_read: 0, cache_write: 0 },
          }),
        ],
      }),
      { usageHarness: "open-code" },
    );
    expect(screen.getByText("3k tok")).toBeInTheDocument();
    expect(screen.queryByText(/^5h /)).not.toBeInTheDocument();
  });

  it("pages forward and back through the detected harnesses", async () => {
    const user = userEvent.setup();
    pager();
    expect(screen.getByText(HARNESS_CODE["claude-code"])).toBeInTheDocument();

    await user.click(screen.getByTitle("Next harness"));
    expect(useMonitor.getState().usageHarness).toBe("open-code");
    expect(screen.getByText(HARNESS_CODE["open-code"])).toBeInTheDocument();

    await user.click(screen.getByTitle("Previous harness"));
    expect(useMonitor.getState().usageHarness).toBe("claude-code");
  });

  it("disables both arrows when there is nothing to page through", () => {
    // An arrow that does nothing is worse than no arrow.
    pager(snapshot({ detected: ["claude-code"] }));
    expect(screen.getByTitle("Previous harness")).toBeDisabled();
    expect(screen.getByTitle("Next harness")).toBeDisabled();
  });

  it("enables them as soon as there are two", () => {
    pager(snapshot({ detected: ["claude-code", "codex"] }));
    expect(screen.getByTitle("Previous harness")).toBeEnabled();
    expect(screen.getByTitle("Next harness")).toBeEnabled();
  });

  it("falls back to the first detected harness when the remembered one is gone", async () => {
    // A harness can be uninstalled between runs; the pager must not sit on an
    // empty page.
    pager(snapshot({ detected: ["codex", "gemini"] }), { usageHarness: "antigravity" });
    expect(screen.getByText(HARNESS_CODE.codex)).toBeInTheDocument();
  });

  it("names the page in its tooltip only when there is more than one", () => {
    const { unmount } = pager(snapshot({ detected: ["claude-code"] }));
    expect(screen.getByTitle("Usage for Claude Code").getAttribute("title")).toBe(
      "Usage for Claude Code",
    );
    unmount();

    pager(snapshot({ detected: THREE.map(String) as never }));
    expect(
      screen.getAllByTitle(/arrows page through the others/)[0]?.getAttribute("title"),
    ).toContain("Usage for Claude Code");
  });

  it("waits rather than claiming there is no plan window before the first snapshot", () => {
    renderWithStore(<UsagePager snapshot={null} now={NOW} tone={TONE} />, { now: NOW });
    expect(screen.getByText("--")).toBeInTheDocument();
    expect(screen.queryByText("no quota yet")).not.toBeInTheDocument();
  });

  it("says so plainly for a harness with no live sessions", () => {
    pager(snapshot({ detected: THREE.map(String) as never }), { usageHarness: "codex" });
    expect(screen.getByText("no live sessions")).toBeInTheDocument();
  });

  it("prefers cost over a session count when the harness reports one", () => {
    pager(
      snapshot({
        detected: THREE.map(String) as never,
        sessions: [
          session({
            key: { harness: "codex" },
            tokens: { input: 500, output: 500, reasoning: 0, cache_read: 0, cache_write: 0 },
            cost: 4.5,
          }),
        ],
      }),
      { usageHarness: "codex" },
    );
    expect(screen.getByText("1k tok")).toBeInTheDocument();
    expect(screen.getByText("$4.50")).toBeInTheDocument();
  });

  it("shows a session count when there are tokens but no cost", () => {
    // Claude Code reports no per-session cost; a "$0.00" there would read as
    // "free", which is the opposite of the truth.
    pager(
      snapshot({
        detected: THREE.map(String) as never,
        sessions: [
          session({
            key: { harness: "codex" },
            tokens: { input: 500, output: 500, reasoning: 0, cache_read: 0, cache_write: 0 },
          }),
          session({ key: { harness: "codex" } }),
        ],
      }),
      { usageHarness: "codex" },
    );
    expect(screen.getByText("1k tok")).toBeInTheDocument();
    expect(screen.getByText("2 sessions")).toBeInTheDocument();
  });

  it("says nothing reported when the harness gives neither", () => {
    // gemini-cli records no status and no cost, and a bare "2 live" would
    // imply a measurement that was never made.
    pager(
      snapshot({
        detected: THREE.map(String) as never,
        sessions: [session({ key: { harness: "codex" }, tokens: null })],
      }),
      { usageHarness: "codex" },
    );
    expect(screen.getByText("1 live")).toBeInTheDocument();
    expect(screen.getByText("no usage reported")).toBeInTheDocument();
  });

  it("breaks the tokens down per session in the tooltip", () => {
    const { container } = pager(
      snapshot({
        detected: THREE.map(String) as never,
        sessions: [
          session({
            key: { harness: "codex" },
            name: "migrations",
            tokens: { input: 1_386, output: 951_739, reasoning: 0, cache_read: 0, cache_write: 0 },
          }),
        ],
      }),
      { usageHarness: "codex" },
    );
    // The chevrons and the harness code carry titles too, so the usage block
    // has to be picked out by what its title says rather than by position.
    expect(usageTitle(container)).toContain("migrations");
    expect(usageTitle(container)).toMatch(/in [\d,]+/);
  });

  it("falls back to a session count in the tooltip when nothing is reported", () => {
    const { container } = pager(
      snapshot({
        detected: THREE.map(String) as never,
        sessions: [session({ key: { harness: "codex" }, tokens: null })],
      }),
      { usageHarness: "codex" },
    );
    expect(usageTitle(container)).toBe("1 session(s)");
  });

  it("keeps the harness code in the one-line bar, since that is what the arrows page", () => {
    // Everything but the word is dropped at 44px; the two letters are what
    // make the arrows mean anything.
    renderWithStore(
      <UsagePager
        snapshot={snapshot({ detected: [...THREE], quota: quotaSnapshot({ at: NOW }) })}
        now={NOW}
        tone={TONE}
        inline
      />,
      { now: NOW },
    );
    expect(screen.getByText(HARNESS_CODE["claude-code"])).toBeInTheDocument();
  });

  it("stays a single row in the one-line bar and a column in the strip", () => {
    const { container: flat } = renderWithStore(
      <UsagePager snapshot={snapshot({ detected: [...THREE] })} now={NOW} tone={TONE} />,
      { now: NOW },
    );
    expect((flat.firstElementChild as HTMLElement).className).toContain("flex-col");

    const { container: thin } = renderWithStore(
      <UsagePager snapshot={snapshot({ detected: [...THREE] })} now={NOW} tone={TONE} inline />,
      { now: NOW },
    );
    expect((thin.firstElementChild as HTMLElement).className).toContain("items-center");
  });
});
