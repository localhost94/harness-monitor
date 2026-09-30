import { describe, expect, it, vi } from "vitest";
import { fireEvent, screen } from "@testing-library/react";
import App from "./App";
import { NOW, session, snapshot } from "./test/fixtures";
import { renderWithStore } from "./test/render";
import * as tauri from "./test/tauri";
import { drags } from "./test/setup";
import { useMonitor } from "./store/useMonitor";

/** Captured before any test stubs it, since the store outlives the test. */
const realInit = useMonitor.getState().init;

/**
 * `App` calls `init()` on mount, and the preview branch of `init` replaces the
 * snapshot with the fixture and forces the panel open. That is the right
 * behaviour for a browser preview and the wrong starting point for a layout
 * test, so the data path is stubbed here and exercised on its own below.
 */
function mount(store: Record<string, unknown> = {}) {
  return renderWithStore(<App />, {
    now: NOW,
    snapshot: null,
    init: async () => {},
    ...store,
  });
}

describe("App", () => {
  it("keeps the panel out of the way until it is asked for", () => {
    mount();
    expect(screen.queryByRole("button", { name: /finished/ })).not.toBeInTheDocument();
  });

  it("carries the list once expanded", () => {
    mount({ expanded: true, snapshot: snapshot({ sessions: [session({ name: "a-session" })] }) });
    expect(screen.getByText("a-session")).toBeInTheDocument();
  });

  it("applies the dark class to the whole widget, not to a subtree", () => {
    // The pill and the panel have to flip together or the widget reads as two
    // objects instead of one.
    const { container } = mount({ theme: "dark", expanded: true });
    expect((container.firstElementChild as HTMLElement).className).toContain("dark");
  });

  it("leaves the dark class off in light mode, since the class is what drives it", () => {
    const { container } = mount({ theme: "light" });
    expect((container.firstElementChild as HTMLElement).className).not.toContain("dark");
  });

  it("lets the vertical strip fill the window while the pill stays fixed height", () => {
    // Two different height strategies: the strip is the window, the pill is 80px.
    const { container: vertical } = mount({ shape: "vertical" });
    expect((vertical.firstElementChild?.firstElementChild as HTMLElement).className).toContain(
      "flex-1",
    );

    const { container: flat } = mount({ shape: "pill" });
    expect((flat.firstElementChild?.firstElementChild as HTMLElement).className).toContain(
      "flex-none",
    );
  });

  it("bounds the panel so a long list can actually scroll", () => {
    // Without a bounded height, overflow-y-auto never engages and long lists
    // were simply clipped.
    const { container } = mount({ expanded: true, snapshot: snapshot() });
    // The pill is also rounded-[20px], so the panel is picked out by the gap
    // that separates it from the pill.
    const panel = container.querySelector('[class*="mt-1"]') as HTMLElement;
    expect(panel).not.toBeNull();
    expect(panel.className).toContain("min-h-0");
    expect(panel.className).toContain("overflow-hidden");
  });

  it("shares the same rounded corner between pill and panel, so they read as one object", () => {
    const { container } = mount({ expanded: true, snapshot: snapshot() });
    const panel = container.querySelector('[class*="mt-1"]') as HTMLElement;
    expect(panel.className).toContain("rounded-[20px]");
    expect((container.firstElementChild as HTMLElement).className).toContain("rounded-2xl");
  });

  it("starts the data path on mount", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    tauri.resolves("get_snapshot", snapshot({ sessions: [] }));
    tauri.resolves("is_muted", false);

    // The real `init`, not the stub the layout tests above left in the store.
    renderWithStore(<App />, { now: NOW, snapshot: null, init: realInit });
    await vi.waitFor(() =>
      expect(tauri.calls.map(([command]) => command)).toContain("get_snapshot"),
    );
  });

  describe("the drag region", () => {
    it("starts a drag from anywhere in the pill that is not a control", () => {
      // Tauri's data-tauri-drag-region only matches the exact mousedown target,
      // so clicking the headline - the biggest part of the pill - would not
      // move the window at all.
      const { container } = mount({ snapshot: snapshot({ sessions: [session()] }) });
      fireEvent.mouseDown(screen.getByText("1 running"));
      expect(drags).toBe(1);
      // The drag only happened because the click landed inside a drag zone.
      expect(container.querySelector("[data-drag-zone]")).not.toBeNull();
    });

    it("does not drag when the click lands on a button", () => {
      // Otherwise pressing a control would also move the window, and the button
      // would feel like it missed.
      mount({ snapshot: snapshot({ sessions: [session()] }) });
      fireEvent.mouseDown(screen.getByTitle("Show sessions"));
      expect(drags).toBe(0);
    });

    it("does not drag from the search box or the tab strip", () => {
      // The panel sits inside a drag region; without data-no-drag, typing in it
      // would drag the window instead of putting a character on screen.
      mount({ expanded: true, snapshot: snapshot() });
      fireEvent.mouseDown(screen.getByLabelText("Filter live sessions"));
      expect(drags).toBe(0);
    });

    it("does not drag from outside the widget", () => {
      mount({ snapshot: snapshot({ sessions: [session()] }) });
      fireEvent.mouseDown(document.body);
      expect(drags).toBe(0);
    });

    it("ignores a non-primary button, so a right-click menu still works", () => {
      mount({ snapshot: snapshot({ sessions: [session()] }) });
      fireEvent.mouseDown(screen.getByText("1 running"), { button: 2 });
      expect(drags).toBe(0);
    });

    it("stops listening once unmounted", () => {
      // A leaked listener would keep dragging a window that is no longer there.
      const { unmount } = mount({ snapshot: snapshot({ sessions: [session()] }) });
      unmount();
      fireEvent.mouseDown(document.body, { button: 0 });
      expect(drags).toBe(0);
    });
  });
});
