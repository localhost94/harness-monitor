import { modeOf, surfaceFor } from "../lib/theme";
import { useMonitor } from "../store/useMonitor";
import { DragGrip } from "./DragGrip";
import { HarnessChips } from "./HarnessChips";
import { UsagePager } from "./UsagePager";
import { StatStrip } from "./StatStrip";
import { Glyph } from "./StateBadge";

export function Pill() {
  const {
    snapshot,
    expanded,
    muted,
    now,
    theme,
    orientation,
    toggleExpanded,
    toggleMuted,
    toggleTheme,
    toggleOrientation,
  } = useMonitor();
  const vertical = orientation === "vertical" && !expanded;
  const sessions = snapshot?.sessions ?? [];
  const attention = sessions.filter(
    (s) => s.state === "awaiting-input" || s.state === "awaiting-permission",
  );
  const running = sessions.filter(
    (s) => s.state === "running" || s.state === "active-unknown",
  ).length;
  const mode = modeOf(sessions);
  const surface = surfaceFor(mode);

  const headlineState =
    mode === "permission"
      ? ("awaiting-permission" as const)
      : mode === "input"
        ? ("awaiting-input" as const)
        : mode === "running"
          ? ("running" as const)
          : ("idle" as const);

  // The headline answers the only question the pill exists to answer: does
  // anything want me right now?
  const headline =
    attention.length > 0
      ? `${attention.length} need${attention.length === 1 ? "s" : ""} you`
      : running > 0
        ? `${running} running`
        : sessions.length > 0
          ? "all idle"
          : "no sessions";

  const controls = (
    <>
      <IconButton
        onClick={toggleExpanded}
        title={expanded ? "Collapse" : "Show sessions"}
        tone={surface.title}
      >
        {expanded ? <path d="M3 8.5 7 4.5l4 4" /> : <path d="M3 5h8M3 8h8M3 11h5" />}
      </IconButton>
      <IconButton
        onClick={toggleMuted}
        title={muted ? "Notifications muted - click to unmute" : "Mute notifications"}
        tone={muted ? "text-rose-600 dark:text-rose-400" : surface.sub}
      >
        {muted ? (
          <path d="M4 6h2l3-2.5v9L6 10H4Zm7.5-1 -3 6" />
        ) : (
          <path d="M4 6h2l3-2.5v9L6 10H4Zm7 -1a4 4 0 0 1 0 6" />
        )}
      </IconButton>
      <IconButton
        onClick={toggleOrientation}
        title={orientation === "vertical" ? "Switch to horizontal" : "Switch to vertical"}
        tone={surface.sub}
      >
        {orientation === "vertical" ? (
          <path d="M2.5 4.5h9M2.5 9.5h9" />
        ) : (
          <path d="M4.5 2.5v9M9.5 2.5v9" />
        )}
      </IconButton>
      <IconButton onClick={toggleTheme} title="Switch light / dark" tone={surface.sub}>
        {theme === "dark" ? (
          <path d="M7 3v1.5M7 9.5V11M3 7h1.5M9.5 7H11M7 5.2a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6Z" />
        ) : (
          <path d="M9.2 8.6A3.6 3.6 0 0 1 5.4 4.8 3.9 3.9 0 1 0 9.2 8.6Z" />
        )}
      </IconButton>
    </>
  );

  const glyph = (
    <span
      className={`shrink-0 ${surface.accent} ${mode === "running" ? "hm-breathe" : ""} ${
        mode === "input" || mode === "permission" ? "hm-alert" : ""
      }`}
    >
      <Glyph state={headlineState} />
    </span>
  );

  const shell = `relative overflow-hidden border backdrop-blur-xl ${surface.shell} ${surface.ring}`;

  // Vertical strip: for parking along a screen edge. Same information, stacked,
  // with the counts as full-width rows so the numbers line up.
  if (vertical) {
    return (
      <div
        data-drag-zone
        data-tauri-drag-region
        className={`grid h-full grid-rows-[5px_auto_auto_auto_auto] gap-y-2 rounded-[20px] px-2 pb-2 pt-0 ${shell}`}
      >
        <div data-tauri-drag-region className={`-mx-2 h-full w-auto ${surface.edge}`} />

        <div data-tauri-drag-region className="flex flex-col gap-1.5 pt-1">
          {/* Same headline as the horizontal pill: the counts below are
              detail, and detail without a headline reads as trivia. */}
          <div data-tauri-drag-region className="flex items-center gap-1">
            {glyph}
            <span className={`truncate text-[11px] font-semibold ${surface.title}`}>
              {headline}
            </span>
          </div>
          <UsagePager snapshot={snapshot} now={now} tone={surface} stack />
        </div>

        <StatStrip sessions={sessions} vertical />
        <HarnessChips sessions={sessions} detected={snapshot?.detected ?? []} vertical />

        <div className="flex items-end justify-between gap-px self-end">{controls}</div>
      </div>
    );
  }

  return (
    <div
      data-drag-zone
      data-tauri-drag-region
      // Grid, not flex: fixed rails for rail/grip/quota/buttons and one
      // minmax(0,1fr) column for text, so nothing can push the controls off
      // the pill however long a session name gets.
      className={`grid h-[80px] grid-cols-[5px_10px_minmax(0,1fr)_auto_auto] items-center gap-x-2 rounded-[20px] pr-1.5 ${shell}`}
    >
      {/* Glass gloss: a hairline highlight along the top edge. */}
      <div
        className={`pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r to-transparent ${surface.gloss}`}
      />
      <div data-tauri-drag-region className={`h-full w-full ${surface.edge}`} />
      <DragGrip />

      <div data-tauri-drag-region className="flex min-w-0 flex-col gap-1">
        <div data-tauri-drag-region className="flex items-center gap-1.5">
          {glyph}
          <span className={`shrink-0 text-[13px] font-semibold tracking-tight ${surface.title}`}>
            {headline}
          </span>
          {attention.length > 0 && (
            <span className={`truncate text-[10px] ${surface.sub}`}>
              {attention[0].waiting_for ?? "waiting"}
            </span>
          )}
        </div>

        {/* One row: counts first (the answer), harnesses second (the where). */}
        <div data-tauri-drag-region className="flex min-w-0 items-center gap-1.5 overflow-hidden">
          <StatStrip sessions={sessions} />
          <span
            data-tauri-drag-region
            className="h-3 w-px bg-indigo-950/15 dark:bg-white/15"
            aria-hidden="true"
          />
          <HarnessChips sessions={sessions} detected={snapshot?.detected ?? []} />
        </div>
      </div>

      <UsagePager snapshot={snapshot} now={now} tone={surface} stack />

      {/* Four icons in 2x2: a single column would not fit 80px of height. */}
      <div className="grid grid-cols-2 gap-px">{controls}</div>
    </div>
  );
}

function IconButton({
  onClick,
  title,
  tone,
  children,
}: {
  onClick: () => void;
  title: string;
  tone: string;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className={`rounded-lg p-0.5 transition hover:bg-indigo-950/10 dark:hover:bg-white/10 ${tone}`}
    >
      <svg
        viewBox="0 0 14 14"
        className="h-3.5 w-3.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinecap="round"
        aria-hidden="true"
      >
        {children}
      </svg>
    </button>
  );
}
