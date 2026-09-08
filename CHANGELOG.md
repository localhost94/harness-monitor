# Changelog

Notable changes to HarnessMonitor. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Per-session usage for every harness that reports it**, on each row and
  totalled per harness: tokens everywhere, plus cost where the harness knows it.
  Claude Code's token figures are new - its state file has none, so the adapter
  now tails the session transcript with a byte cursor and folds in only what was
  appended, skipping sidechain lines so subagent turns are not double-counted.
- A line in the expanded list stating that the 5h/7d rings are Claude's plan
  window and that other harnesses bill per token, since nothing else exposes an
  equivalent and a blended figure would be fiction.

- The **7-day plan window** alongside the 5-hour one, both as rings, and each
  labelled with the **wall-clock time it resets** rather than only a countdown.
  The data was already being collected; only the 5-hour figure was shown.
- Harnesses that are detected but have no live sessions collapse into a `+N`
  chip in the horizontal pill, which frees the width the second quota window
  needed. Hovering names them.

## [0.1.1] - 2026-09-07

### Added

- **Windows x64 binary**, built in CI and attached to the release. Until now the
  only published build was ARM64, so most Windows users had to compile Tauri
  themselves.
- Release workflow that builds the Windows x64 app and both Linux agents
  (x86_64 and aarch64) on tag, so a working pair ships for either architecture.
  The agent runs inside WSL, so it has to match the machine: an x86_64 agent
  cannot run on an ARM64 machine's WSL.

### Documentation

- `docs/ARCHITECTURE.md`, `SECURITY.md`, `CHANGELOG.md`, Contributor Covenant,
  issue and PR templates, dependabot.

### Fixed

- CI: pinned bun to the version that wrote `bun.lock`; a newer bun rewrites the
  lockfile format, which made `--frozen-lockfile` fail on a lockfile that was
  in fact in sync.

## [0.1.0] - 2026-09-07

First public build.

### Added

- Tracks Claude Code, opencode, codex, gemini-cli and antigravity at once,
  reading only what those tools already write to disk. No backend, no network
  calls, no API keys.
- Reports each session as running, idle, waiting for input, or waiting for a
  permission decision, and notifies on exactly two transitions: a turn
  finishing, and a session starting to wait.
- Per-harness fidelity is labelled in the UI rather than averaged, so an
  inferred state is never presented as a reported one.
- Claude plan window (5-hour) shown as a ring, mirrored from the harness and
  never derived from token counts. Two sources are reconciled by recency: the
  statusline shim in `installer/`, and a `Stop` hook's analytics JSONL.
- Jump to the terminal pane running a session, via herdr when it is present.
- Floating pill (440x80) or vertical strip (118x300), light or dark, always on
  top, position remembered across restarts.
- Windows builds run the adapters inside WSL through the same binary in
  `--agent` mode; Linux/WSLg runs them in-process.
- Desktop notifications through the OS on Windows, and through the xdg desktop
  portal on Linux, where a stock WSL has no notification daemon.

### Known limitations

- The published `.exe` is Windows ARM64. x64 users must build from source until
  the release workflow's x64 artifact lands.
- Plan-quota figures come from a live Claude Code process, so they freeze - and
  grey out with their age - when no session is running.
- opencode permission prompts are deliberately not detected: the only trace is
  a log line with no session id and no matching resolution event, so anything
  latched from it would never unlatch.
- antigravity is presence-only. Its conversations are binary protobuf with no
  published schema, so no state is claimed for it.

[Unreleased]: https://github.com/localhost94/harness-monitor/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/localhost94/harness-monitor/releases/tag/v0.1.1
[0.1.0]: https://github.com/localhost94/harness-monitor/releases/tag/v0.1.0
