import { useEffect, useRef } from "react";
import { HARNESS_CODE, HARNESS_LABEL } from "../types";
import { PANEL_STICKY } from "../lib/theme";
import {
  ALL_HARNESSES,
  ENDED_CHOICES,
  INTERVAL_CHOICES,
  TARGET_CHOICES,
  TIMEOUT_CHOICES,
  toggleHarness,
} from "../lib/settings";
import { isResumable } from "../lib/rerun";
import { useMonitor, type Shape } from "../store/useMonitor";

/**
 * Settings, as a popover over the list.
 *
 * A popover rather than a third tab because the two tabs are a question about
 * sessions and this is not one - folding it in would mean the live and finished
 * lists could both be navigated away from, and the counts on the tab strip would
 * stop meaning "what is happening now". It is also only reachable while the
 * panel is open: the pill's four buttons sit in a 2x2 that 80px of height
 * cannot grow, which is the reason the gear is not on the pill at all.
 *
 * The controls are grouped by when the value takes effect rather than by what it
 * is about, because that is the thing the user cannot otherwise see. Anything
 * read by the running pipeline applies immediately; anything the scanner was
 * started with says so.
 */
export function SettingsPopover({ onClose }: { onClose: () => void }) {
  const settings = useMonitor((s) => s.settings);
  const update = useMonitor((s) => s.updateSettings);
  const needsRestart = useMonitor((s) => s.needsRestart);
  const theme = useMonitor((s) => s.theme);
  const toggleTheme = useMonitor((s) => s.toggleTheme);
  const shape = useMonitor((s) => s.shape);
  const setShape = useMonitor((s) => s.setShape);
  const view = useMonitor((s) => s.view);
  const setView = useMonitor((s) => s.setView);
  const detected = useMonitor((s) => s.snapshot?.detected ?? []);
  const panel = useRef<HTMLDivElement>(null);

  // Escape closes, from anywhere - including from a control the user has just
  // tabbed to, which a click-outside handler would not catch.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      {/* A backdrop rather than a document click handler: the list scrolls under
          the popover, and a click that lands on a row the popover is covering
          must not also select that row. */}
      <button
        aria-label="Close settings"
        onClick={onClose}
        className="absolute inset-0 z-10 cursor-default"
        tabIndex={-1}
      />
      <div
        ref={panel}
        data-no-drag
        role="dialog"
        aria-label="Settings"
        className={`absolute inset-x-2 bottom-2 z-20 max-h-[calc(100%-2.5rem)] overflow-y-auto rounded-xl px-2.5 py-2 shadow-lg ring-1 ring-black/10 backdrop-blur-xl dark:ring-white/15 ${PANEL_STICKY}`}
      >
        <Group title="Run again">
          <Choice
            label="Open in"
            note={TARGET_CHOICES.find((c) => c.value === settings.rerun_target)?.note}
          >
            {TARGET_CHOICES.map((choice) => (
              <Pill
                key={choice.value}
                active={settings.rerun_target === choice.value}
                onClick={() => void update({ rerun_target: choice.value })}
              >
                {choice.label}
              </Pill>
            ))}
          </Choice>
          <Toggle
            label="Focus the new pane"
            on={settings.rerun_focus}
            onChange={(rerun_focus) => void update({ rerun_focus })}
          />
          <Choice label="Wait up to">
            {TIMEOUT_CHOICES.map((choice) => (
              <Pill
                key={choice.value}
                active={settings.rerun_timeout_ms === choice.value}
                onClick={() => void update({ rerun_timeout_ms: choice.value })}
              >
                {choice.label}
              </Pill>
            ))}
          </Choice>
          {/* Not a control, and not decoration either: two of the five harnesses
              genuinely cannot be reopened, and the buttons on those rows are
              disabled for exactly this reason. */}
          <p className="pt-0.5 text-[9px] leading-snug text-black/40 dark:text-white/40">
            Only {ALL_HARNESSES.filter(isResumable).map((h) => HARNESS_LABEL[h]).join(", ")} can
            reopen a conversation by its id. gemini-cli only resumes “latest”, and antigravity
            has no resume flag.
          </p>
        </Group>

        <Group title="Notifications">
          <Toggle
            label="Mute notifications"
            on={settings.muted}
            onChange={(muted) => void update({ muted })}
          />
          <Toggle
            label="Stay quiet while this panel is focused"
            on={settings.suppress_when_focused}
            onChange={(suppress_when_focused) => void update({ suppress_when_focused })}
          />
        </Group>

        <Group title="Data & refresh" restart>
          <Choice label="Refresh every">
            {INTERVAL_CHOICES.map((choice) => (
              <Pill
                key={choice.value}
                active={settings.interval_ms === choice.value}
                onClick={() => void update({ interval_ms: choice.value })}
              >
                {choice.label}
              </Pill>
            ))}
          </Choice>
          <Choice label="Keep this many finished rows">
            {ENDED_CHOICES.map((choice) => (
              <Pill
                key={choice.value}
                active={settings.max_ended === choice.value}
                onClick={() => void update({ max_ended: choice.value })}
              >
                {choice.label}
              </Pill>
            ))}
          </Choice>
          <div className="flex items-center gap-1 pt-0.5">
            <span className="shrink-0 text-[10px] text-black/50 dark:text-white/50">Harnesses</span>
            <div className="flex flex-wrap gap-1">
              {ALL_HARNESSES.map((harness) => {
                // A harness that is not installed cannot be switched on, and
                // offering the toggle anyway would be a switch that silently
                // does nothing.
                const installed = detected.includes(harness);
                const on = settings.enabled.includes(harness);
                return (
                  <button
                    key={harness}
                    onClick={() => void update({ enabled: toggleHarness(settings.enabled, harness) })}
                    disabled={!installed}
                    aria-pressed={on}
                    title={
                      installed
                        ? `${on ? "Stop" : "Start"} scanning ${HARNESS_LABEL[harness]}`
                        : `${HARNESS_LABEL[harness]} is not installed on this machine`
                    }
                    className={`rounded px-1 py-px text-[9px] font-medium ring-1 ring-inset transition ${
                      !installed
                        ? "cursor-default text-black/25 ring-black/10 dark:text-white/25 dark:ring-white/10"
                        : on
                          ? "bg-black text-white ring-black dark:bg-white dark:text-black dark:ring-white"
                          : "text-black/45 ring-black/15 hover:bg-black/[0.04] dark:text-white/45 dark:ring-white/20 dark:hover:bg-white/[0.06]"
                    }`}
                  >
                    {HARNESS_CODE[harness]}
                  </button>
                );
              })}
            </div>
          </div>
        </Group>

        <Group title="Appearance">
          <Choice label="Theme">
            <Pill active={theme === "light"} onClick={toggleTheme}>
              light
            </Pill>
            <Pill active={theme === "dark"} onClick={toggleTheme}>
              dark
            </Pill>
          </Choice>
          <Choice label="Shape">
            {(["pill", "line", "vertical"] as Shape[]).map((option) => (
              <Pill
                key={option}
                active={shape === option}
                // One button per shape rather than a cycle, because a cycle
                // cannot show you the state you are in - and clicking "vertical"
                // has to land on vertical, not on whatever comes next.
                onClick={() => void setShape(option)}
              >
                {option === "line" ? "one line" : option}
              </Pill>
            ))}
          </Choice>
          <Choice label="Open on">
            <Pill active={view === "live"} onClick={() => setView("live")}>
              live
            </Pill>
            <Pill active={view === "finished"} onClick={() => setView("finished")}>
              finished
            </Pill>
          </Choice>
        </Group>

        {needsRestart && (
          <p className="mt-1.5 rounded-lg bg-black/[0.05] px-2 py-1 text-[9px] leading-snug text-black/60 ring-1 ring-inset ring-black/10 dark:bg-white/[0.06] dark:text-white/65 dark:ring-white/15">
            The scanner reads these when it starts. Restart HarnessMonitor to apply them.
          </p>
        )}
      </div>
    </>
  );
}

function Group({
  title,
  restart,
  children,
}: {
  title: string;
  restart?: boolean;
  children: React.ReactNode;
}) {
  return (
    <section className="border-b border-black/[0.07] py-1.5 first:pt-0 last:border-b-0 dark:border-white/10">
      <h3 className="mb-1 flex items-baseline gap-1.5 text-[9px] uppercase tracking-wide text-black/40 dark:text-white/40">
        {title}
        {restart && (
          <span className="normal-case tracking-normal text-black/30 dark:text-white/30">
            on restart
          </span>
        )}
      </h3>
      <div className="flex flex-col gap-1">{children}</div>
    </section>
  );
}

function Choice({
  label,
  note,
  children,
}: {
  label: string;
  note?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className="flex items-center gap-1.5">
        <span className="shrink-0 text-[10px] text-black/50 dark:text-white/50">{label}</span>
        <div className="flex flex-wrap items-center gap-1">{children}</div>
      </div>
      {/* Visible rather than a tooltip. The consequence of a launch target is
          the thing most likely to surprise someone - "terminal" opens a window
          even with herdr installed - and a title attribute is invisible until
          you happen to hover it. */}
      {note && (
        <p className="pt-0.5 pl-0.5 text-[9px] leading-snug text-black/40 dark:text-white/40">
          {note}
        </p>
      )}
    </div>
  );
}

function Pill({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      className={`rounded-md px-1.5 py-0.5 text-[9px] font-medium ring-1 ring-inset transition ${
        active
          ? "bg-black text-white ring-black dark:bg-white dark:text-black dark:ring-white"
          : "text-black/45 ring-black/10 hover:bg-black/[0.04] dark:text-white/45 dark:ring-white/10 dark:hover:bg-white/[0.06]"
      }`}
    >
      {children}
    </button>
  );
}

/**
 * A switch, drawn as a checkbox the size of the rest of the panel.
 *
 * A real `<input type="checkbox">` rather than a styled div, so it is reachable
 * by keyboard and announced correctly for free.
 */
function Toggle({
  label,
  on,
  onChange,
}: {
  label: string;
  on: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="flex cursor-pointer items-center gap-1.5">
      <input
        type="checkbox"
        checked={on}
        onChange={(event) => onChange(event.target.checked)}
        className="h-3 w-3 shrink-0 accent-black dark:accent-white"
      />
      <span className="text-[10px] text-black/60 dark:text-white/60">{label}</span>
    </label>
  );
}
