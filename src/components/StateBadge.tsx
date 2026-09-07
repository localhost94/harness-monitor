import type { SessionState } from "../types";
import { STATE_LABEL, STATE_STYLE } from "../lib/format";

/**
 * State is never signalled by colour alone: each one gets its own glyph and
 * its own motion. Running breathes, waiting blinks, idle sits still.
 */
function Glyph({ state }: { state: SessionState }) {
  switch (state) {
    case "running":
      return (
        <svg viewBox="0 0 10 10" className="h-2.5 w-2.5 fill-current" aria-hidden="true">
          <path d="M2 1.2 8.4 5 2 8.8Z" />
        </svg>
      );
    case "awaiting-input":
      return (
        <svg viewBox="0 0 10 10" className="h-2.5 w-2.5 fill-current" aria-hidden="true">
          <rect x="4.1" y="1" width="1.8" height="5" rx="0.9" />
          <circle cx="5" cy="8.2" r="1.05" />
        </svg>
      );
    case "awaiting-permission":
      return (
        <svg viewBox="0 0 10 10" className="h-2.5 w-2.5 fill-current" aria-hidden="true">
          <path d="M3 4.4V3.3a2 2 0 0 1 4 0v1.1h-1.1V3.3a.9.9 0 0 0-1.8 0v1.1Z" />
          <rect x="2.2" y="4.4" width="5.6" height="4.4" rx="1" />
        </svg>
      );
    case "shell":
      return (
        <svg viewBox="0 0 10 10" className="h-2.5 w-2.5 fill-current" aria-hidden="true">
          <path d="M1.6 2.6 4 5 1.6 7.4 2.5 8.3 5.8 5 2.5 1.7Z" />
          <rect x="5.6" y="7.3" width="3" height="1" rx="0.5" />
        </svg>
      );
    case "idle":
      return (
        <svg viewBox="0 0 10 10" className="h-2.5 w-2.5 fill-current" aria-hidden="true">
          <rect x="2.4" y="2.4" width="1.7" height="5.2" rx="0.8" />
          <rect x="5.9" y="2.4" width="1.7" height="5.2" rx="0.8" />
        </svg>
      );
    default:
      return (
        <svg viewBox="0 0 10 10" className="h-2.5 w-2.5 fill-current" aria-hidden="true">
          <circle cx="5" cy="5" r="2.2" />
        </svg>
      );
  }
}

function motionFor(state: SessionState): string {
  if (state === "running" || state === "active-unknown") return "hm-breathe";
  if (state === "awaiting-input" || state === "awaiting-permission") return "hm-alert";
  return "";
}

export function StateChip({ state, reason }: { state: SessionState; reason?: string | null }) {
  return (
    <span
      className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-[9px] font-medium ring-1 ring-inset ${STATE_STYLE[state]}`}
      title={reason ?? STATE_LABEL[state]}
    >
      <span className={motionFor(state)}>
        <Glyph state={state} />
      </span>
      {reason ?? STATE_LABEL[state]}
    </span>
  );
}

export { motionFor, Glyph };
