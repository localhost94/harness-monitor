/**
 * Mutable state behind the `@tauri-apps/api` mocks installed in `setup.ts`.
 *
 * The mocks have to be addressable from a test body - a test that wants
 * `set_shape` to fail needs to make it fail, and a test that wants no snapshot
 * needs the call to resolve `null` rather than hang. So the mock delegates here
 * instead of hardcoding a return value.
 */

/** Per-command handler. Receives the invoke args; may be async. */
export type InvokeHandler = (args: Record<string, unknown>) => unknown;

const handlers = new Map<string, InvokeHandler>();

/**
 * Calls seen so far, in order, as `[command, args]`. Lets a test assert that
 * init reached the backend at all, which is the difference between "the button
 * is wired" and "the button is painted".
 */
export const calls: Array<[string, Record<string, unknown>]> = [];

/** Event listeners registered by `listen`, keyed by event name. */
export const listeners = new Map<string, Set<(payload: unknown) => void>>();

/** Resolve/reject handlers for a command, replacing any previous one. */
export function onCommand(command: string, handler: InvokeHandler): void {
  handlers.set(command, handler);
}

/** Resolve a command to a fixed value. */
export function resolves(command: string, value: unknown): void {
  handlers.set(command, () => value);
}

/** Reject a command, the way a refused IPC call would. */
export function rejects(command: string, reason: unknown): void {
  handlers.set(command, () => {
    throw reason;
  });
}

/** Make a command hang, like an agent that never starts. */
export function pending(command: string): void {
  handlers.set(command, () => new Promise(() => {}));
}

export async function dispatch(command: string, args: Record<string, unknown> = {}) {
  calls.push([command, args]);
  const handler = handlers.get(command);
  if (!handler) return null;
  return handler(args);
}

/** Push a payload to whatever the app subscribed to with `listen`. */
export function emit(event: string, payload: unknown): void {
  for (const listener of listeners.get(event) ?? []) listener(payload);
}

/**
 * A fresh set of per-test state. Called from `setup.ts` in `beforeEach`, so no
 * test can see a handler or a call another test registered.
 */
export function reset(): void {
  handlers.clear();
  calls.length = 0;
  listeners.clear();
}
