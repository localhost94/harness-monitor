import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import * as tauri from "./tauri";

/**
 * The app never runs outside a Tauri window except in the headless preview
 * harness, and `init()` branches on exactly that. Both branches are exercised
 * here: a test that wants the preview path deletes `__TAURI_INTERNALS__`, one
 * that wants the real path sets it.
 */
function clearTauriBridge() {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) =>
    tauri.dispatch(command, args ?? {}),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: (e: { payload: unknown }) => void) => {
    const set = tauri.listeners.get(event) ?? new Set();
    set.add((payload) => handler({ payload }));
    tauri.listeners.set(event, set);
    return Promise.resolve(() => set.delete((payload) => handler({ payload })));
  },
}));

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: () => Promise.resolve(true),
  requestPermission: () => Promise.resolve("granted"),
  sendNotification: () => undefined,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    startDragging: () => {
      drags += 1;
      return Promise.resolve();
    },
  }),
}));

/** How many times App asked the window layer to start a drag. */
export let drags = 0;

beforeEach(() => {
  tauri.reset();
  drags = 0;
  clearTauriBridge();
  localStorage.clear();
  window.history.replaceState({}, "", "/");
});

afterEach(() => {
  cleanup();
  // `init()` starts a 1s clock tick that outlives the test that started it.
  vi.clearAllTimers();
  vi.useRealTimers();
  localStorage.clear();
  clearTauriBridge();
  window.history.replaceState({}, "", "/");
});
