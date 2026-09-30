import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsPopover } from "./SettingsPopover";
import { DEFAULT_SETTINGS, type Settings } from "../lib/settings";
import { snapshot } from "../test/fixtures";
import { onCommand, calls, rejects } from "../test/tauri";
import { useMonitor } from "../store/useMonitor";

/**
 * The real store, with only the IPC boundary faked.
 *
 * Stubbing `updateSettings` would have made the interesting half untestable:
 * whether a control sends a patch or the whole object is the store's job, and it
 * is the thing most likely to be wrong. `setup.ts` deletes the Tauri bridge
 * before every test, so it has to be put back here or every write quietly turns
 * into a no-op.
 */
function setup(options: { settings?: Partial<Settings>; detected?: Settings["enabled"] } = {}) {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  const current: Settings = { ...DEFAULT_SETTINGS, ...options.settings };
  // Echo back what was sent, with `needs_restart` decided by the value that was
  // in force - which is what the real command does.
  onCommand("set_settings", (args) => {
    const sent = args.settings as Settings;
    return {
      settings: sent,
      loaded_from_file: true,
      needs_restart: sent.interval_ms !== DEFAULT_SETTINGS.interval_ms,
      error: null,
    };
  });
  useMonitor.setState({
    query: "",
    dateFilter: "all",
    now: 1_700_000_000_000,
    view: "live",
    theme: "light",
    shape: "pill",
    needsRestart: false,
    error: null,
    settings: current,
    appliedSettings: DEFAULT_SETTINGS,
    snapshot: snapshot({ detected: options.detected ?? DEFAULT_SETTINGS.enabled }),
  });
  return current;
}

function open(options?: Parameters<typeof setup>[0]) {
  setup(options);
  const onClose = () => {};
  render(<SettingsPopover onClose={onClose} />);
  return { user: userEvent.setup(), onClose };
}

/** The last `set_settings` payload, or undefined if the boundary was never hit. */
function lastWrite(): Record<string, unknown> | undefined {
  const hits = calls.filter(([command]) => command === "set_settings");
  const last = hits[hits.length - 1];
  return last?.[1]?.settings as Record<string, unknown> | undefined;
}

describe("SettingsPopover", () => {
  it("groups settings by when they take effect, and marks the one that waits", () => {
    open();
    for (const title of ["Run again", "Notifications", "Data & refresh", "Appearance"]) {
      expect(screen.getByRole("heading", { name: new RegExp(title) })).toBeInTheDocument();
    }
    // The difference has to be visible before anything is changed, or the note
    // at the bottom arrives as a surprise.
    expect(screen.getByRole("heading", { name: /Data & refresh/ }).textContent).toMatch(
      /on restart/,
    );
  });

  it("closes on Escape", async () => {
    let closed = 0;
    setup();
    render(<SettingsPopover onClose={() => (closed += 1)} />);
    await userEvent.setup().keyboard("{Escape}");
    expect(closed).toBe(1);
  });

  it("closes on a click outside it", async () => {
    let closed = 0;
    setup();
    render(<SettingsPopover onClose={() => (closed += 1)} />);
    await userEvent.setup().click(screen.getByRole("button", { name: "Close settings" }));
    expect(closed).toBe(1);
  });

  it("stays open when a control inside is used", async () => {
    // Otherwise toggling a setting would close the sheet you were adjusting,
    // which is the most annoying possible version of this interaction.
    let closed = 0;
    setup();
    render(<SettingsPopover onClose={() => (closed += 1)} />);
    await userEvent.setup().click(screen.getByRole("checkbox", { name: /Mute notifications/ }));
    expect(closed).toBe(0);
  });

  it("sends the whole settings object, not just the field that changed", async () => {
    // The backend has no partial-update command and reads a missing key as
    // "reset to default", so a bare patch would silently reset everything else.
    const { user } = open({ settings: { max_ended: 250 } });
    await user.click(screen.getByRole("button", { name: "1m" }));
    expect(lastWrite()).toEqual({ ...DEFAULT_SETTINGS, max_ended: 250, rerun_timeout_ms: 60_000 });
  });

  it("adopts what the backend stored rather than what was asked for", async () => {
    // The backend clamps; a control that kept showing the requested value would
    // disagree with the file.
    setup();
    onCommand("set_settings", () => ({
      settings: { ...DEFAULT_SETTINGS, interval_ms: 500 },
      loaded_from_file: true,
      needs_restart: true,
      error: null,
    }));
    render(<SettingsPopover onClose={() => {}} />);
    await userEvent.setup().click(screen.getByRole("button", { name: "3s" }));
    // 500ms is not on offer, so nothing claims to be current rather than a
    // button lying about being the value in force.
    expect(screen.queryByRole("button", { pressed: true, name: "3s" })).not.toBeInTheDocument();
  });

  it("reports a settings file that could not be written", async () => {
    // The controls keep showing what was asked for - the user did ask for it -
    // but the failure is not silent.
    setup();
    onCommand("set_settings", () => ({
      settings: DEFAULT_SETTINGS,
      loaded_from_file: true,
      needs_restart: false,
      error: "read-only file system",
    }));
    render(<SettingsPopover onClose={() => {}} />);
    await userEvent.setup().click(screen.getByRole("button", { name: "3s" }));
    expect(useMonitor.getState().error).toMatch(/read-only file system/);
  });

  it("names the harnesses that cannot reopen a conversation, and why", () => {
    // Two of the five have disabled buttons on their rows. This is where a user
    // finds out that it is the harness's limit and not a broken app.
    open();
    const group = screen.getByRole("heading", { name: /Run again/ }).parentElement as HTMLElement;
    expect(group.textContent).toMatch(/gemini-cli only resumes/);
    expect(group.textContent).toMatch(/antigravity has no resume flag/);
  });

  it("explains the launch target in words, not in a tooltip", () => {
    // "terminal" opens a window even when herdr is installed, which surprises
    // people. A title attribute is only read by someone who already suspects.
    open();
    expect(screen.getByText(/a herdr pane if herdr is installed/)).toBeInTheDocument();
  });

  it("disables a harness toggle for a harness that is not installed", async () => {
    // A switch that cannot do anything is worse than one that is not offered.
    const { user } = open({ detected: ["claude-code", "open-code"] });
    const codex = screen.getByRole("button", { name: "CX" });
    expect(codex).toBeDisabled();
    await user.click(codex);
    expect(lastWrite()).toBeUndefined();
  });

  it("switches an installed harness on and off again", async () => {
    const { user } = open({ settings: { enabled: ["claude-code"] } });
    const codex = screen.getByRole("button", { name: "CX" });
    expect(codex).toHaveAttribute("aria-pressed", "false");
    await user.click(codex);
    expect(lastWrite()?.enabled).toEqual(["claude-code", "codex"]);
  });

  it("shows the restart note only when a scan setting changed", async () => {
    // Mute applies to the running pipeline, so promising a restart for it would
    // send people restarting for nothing.
    const { user } = open();
    expect(screen.queryByText(/Restart HarnessMonitor/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("checkbox", { name: /Mute notifications/ }));
    expect(screen.queryByText(/Restart HarnessMonitor/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "3s" }));
    expect(await screen.findByText(/Restart HarnessMonitor/)).toBeInTheDocument();
  });

  it("marks the current value of each segmented control as pressed", () => {
    open({ settings: { interval_ms: 3_000, max_ended: 50, rerun_target: "herdr" } });
    const pressed = screen.getAllByRole("button", { pressed: true }).map((b) => b.textContent);
    expect(pressed).toContain("3s");
    expect(pressed).toContain("50");
    expect(pressed).toContain("herdr");
  });

  it("uses real checkboxes, so the switches are reachable by keyboard", () => {
    open();
    // Three, not two: the reopen-focus switch is a switch too.
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes.map((b) => b.getAttribute("type"))).toEqual(["checkbox", "checkbox", "checkbox"]);
  });

  it("scrolls rather than overflowing the panel", () => {
    // 580px of panel and a dozen controls: on a short window the sheet has to
    // give way, and clipping the last group would hide a setting outright.
    open();
    const dialog = screen.getByRole("dialog", { name: "Settings" });
    expect(dialog.className).toContain("overflow-y-auto");
    expect(dialog.className).toContain("max-h-");
  });

  it("sets the shape it is given, rather than cycling to the next one", async () => {
    // A row of three buttons that each moved you somewhere else would be a lie
    // about what "vertical" does.
    const { user } = open();
    await user.click(screen.getByRole("button", { name: "vertical" }));
    expect(useMonitor.getState().shape).toBe("vertical");
  });

  it("re-reads the target's explanation when the target changes", async () => {
    const { user } = open();
    await user.click(screen.getByRole("button", { name: "herdr" }));
    expect(await screen.findByText(/fail rather than open/)).toBeInTheDocument();
  });

  it("leaves the store's error alone when the write succeeds", async () => {
    const { user } = open();
    useMonitor.setState({ error: null });
    await user.click(screen.getByRole("checkbox", { name: /Mute notifications/ }));
    expect(useMonitor.getState().error).toBeNull();
  });

  it("surfaces a rejected write rather than losing the click", async () => {
    setup();
    rejects("set_settings", "disk on fire");
    render(<SettingsPopover onClose={() => {}} />);
    await userEvent.setup().click(screen.getByRole("button", { name: "3s" }));
    expect(useMonitor.getState().error).toMatch(/disk on fire/);
  });

  it("keeps its groups inside one dialog, so a screen reader can find them", () => {
    open();
    const dialog = screen.getByRole("dialog", { name: "Settings" });
    expect(within(dialog).getByRole("heading", { name: /Appearance/ })).toBeInTheDocument();
  });
});
