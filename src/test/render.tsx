import { render, type RenderOptions, type RenderResult } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { useMonitor } from "../store/useMonitor";

/**
 * Render a component with the store already in a known state.
 *
 * The store is a module singleton, so state has to be written before render
 * rather than passed in. `useMonitor.setState` does exactly that, and leaving
 * the real actions in place means a test can still click a button and watch
 * the store change - which is usually the interesting assertion.
 */
export function renderWithStore(
  ui: ReactElement,
  state: Partial<ReturnType<typeof useMonitor.getState>> = {},
  options?: Omit<RenderOptions, "wrapper">,
): RenderResult {
  useMonitor.setState({
    snapshot: null,
    theme: "light",
    shape: "pill",
    usageHarness: "claude-code",
    view: "live",
    query: "",
    dateFilter: "all",
    error: null,
    receivedAt: null,
    startedAt: Date.now(),
    expanded: false,
    muted: false,
    now: Date.now(),
    ...state,
  });
  return render(ui, options);
}

export { userEvent };

/** The surface tones the pill hands its children. */
export const TONE = { title: "text-black", sub: "text-black/55" } as const;
