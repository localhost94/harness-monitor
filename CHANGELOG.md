# Changelog

Notable changes to HarnessMonitor. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **A finished session can be run again.** Every row in the *finished* list
  carries a button that reopens that conversation in a terminal: `pane split` at
  the session's directory, then `herdr agent start --kind <harness>`, which waits
  for the agent to become interactive. This is a *reopen*, not a *jump* - the
  live list's arrow focuses a pane that is already running, and a finished row
  has no pane - and the two use different glyphs so the difference is visible.

  It is also not a replay, and the app says so: no adapter records the command or
  the prompt a session started with, so what the button does is start the harness
  again on the same conversation (`claude --resume <id>`,
  `opencode --session <id>`, `codex resume <id>`). gemini-cli's `--resume` takes
  only `latest` or an index, and antigravity has no resume flag at all, so those
  two rows carry a **disabled** button that states the reason. An enabled button
  that quietly opened some other conversation would be worse than no button.

  Without herdr there is no addressable terminal, so the fallback opens a new
  one itself - `x-terminal-emulator`, `gnome-terminal`, `konsole`, `alacritty`,
  `kitty`, `wezterm`, `foot`, `xterm`, or AppleScript into Terminal on macOS -
  and if neither is found the failure names what was missing. Windows without
  herdr says so and stops, rather than shelling out to `wt.exe` from inside WSL
  on a code path that could not be tested where it was written.

- **A settings menu.** A gear beside the live and finished tabs - not on the
  pill, whose four buttons already fill a 2x2 that 80px of height cannot grow.
  The groups are arranged by *when the value takes effect*, which is the thing
  you cannot otherwise see: launch target, focus and readiness timeout for
  reopening; mute and the "stay quiet while focused" rule; refresh interval,
  finished-row cap and per-harness switches; and theme, shape and starting tab.
  Anything the scanner was started with says so in its heading, and a note
  appears at the bottom only once you have actually changed one.

  The two tiers are a deliberate line rather than an accident. Appearance is read
  synchronously while the store is created, because a floating widget is on
  screen before any IPC has resolved - moving it behind a round trip would trade
  a visible flash of the wrong theme for nothing - so those three stay in
  `localStorage`. Everything the *running pipeline* reads lives in one
  `settings.json`, which only the UI process touches; the Windows scanner is a
  separate process inside WSL and gets its values as command-line arguments
  instead, which is exactly why those settings need a restart.

- **The frontend has a test suite.** 282 tests over the four pure modules
  (`format`, `search`, `theme`), the store, all nine components and `App`,
  running in jsdom with the Tauri bridge mocked. `bun run test`,
  `bun run test:watch`, `bun run test:coverage`. It runs on both the Linux and
  macOS CI jobs, not just the Rust suite - `format.stamp` and
  `search.windowStart` read the machine timezone and locale, so a green Linux
  run does not mean macOS agrees.

- **Coverage is measured on both halves and reported to Codecov.** Frontend
  coverage is v8 into `coverage/`; the backend uses `cargo llvm-cov` into
  `src-tauri/coverage-lcov.info`. They upload as two flags merged into one
  badge. The Tauri glue, the window handlers and the tray are excluded from the
  report: they are reachable only from a running app, and counting them would
  make the number mostly a measurement of what cannot be tested.

- **The backend suite grew from 31 to 193 tests.** New coverage for the serde
  contract in `model.rs` (including that every enum still serialises to the
  kebab-case strings the TypeScript union is typed against), `quota::read`
  source precedence, the scanner's 20-tick ended-list cache, `PathResolver`,
  the window-placement round-trip, and the three adapters that had none at all -
  codex, gemini-cli and antigravity. Also `project_slug` and the transcript
  cursor in the Claude Code adapter, where the tests pin the two rules that are
  easy to break silently: sidechain turns are not double-counted, and a
  half-written last line is left for the next tick.

### Fixed

- **Mute survives a restart.** It lived in an `AtomicBool` and came back
  unmuted every time, so a widget silenced at 1am started shouting again at 9.
  It is written to the settings file now, and the pill's mute button goes
  through the same write as the settings switch so there is one path rather than
  two. A file that cannot be written still mutes for the session - refusing to
  mute because a disk is read-only is not a good trade.

- **A conditional hook call in the session list.** `SessionList` called
  `useMonitor` and then read `view` from the store *after* an early return, so
  the number of hooks called depended on which branch rendered. It happened to
  work and would have thrown in a test.

### Changed

- **`PathResolver::with_windows_home` states both roots.** `for_home` still
  resolves the Windows profile from the real `/mnt/c/Users` on Linux, which made
  the codex fallback and antigravity untestable on any machine but this one. The
  new constructor takes it explicitly, and the tests use it.

- **The scanner's live-over-history filter and the window-placement
  read/write take their input as a parameter.** Both resolved a path from the
  environment at the call site, which is not something a test can pin without
  mutating global state shared by every test in the process. The behaviour is
  unchanged; `without_live` and `placement::{load_from,save_to}` are just
  reachable now.

### Changed

- **Only one copy of the app runs at a time.** Two of them would fight over the
  same session database, the same saved window position and the same
  notification stream, and there would be no way to tell which one is the real
  monitor. Launching a second time now brings the running window back to the
  front and exits, instead of opening a duplicate pill. `--agent` and
  `--test-notify` are exempt, because both are meant to run alongside an app
  that is already up. On a Linux host with no D-Bus session bus the guarantee
  falls back to a pid lock file, which still blocks the second copy but cannot
  raise the first one's window.

- **The interface is black and white now.** No hue survives anywhere: the
  violet-tinted surfaces, the amber/sky/orange state accents, the one hue per
  harness and the coloured quota rings are all gone. The point is not
  minimalism, it is camouflage - a grey widget on a grey desktop is exactly
  what an editor looks like, and this one floats over editors by design. So
  the surfaces moved to the extremes instead of the middle: true white on true
  black, a hard 1px keyline all the way round, and a deep drop shadow. A
  terminal has a background but no keyline; an editor has neither.
  State is carried by value instead - the rail is full ink when something is
  waiting on you, 70% running, 30% idle, a hairline for nothing - and a
  session waiting on a permission decision is the one row in the list with a
  diagonal caution hatch across it. The two-letter harness chip is now the
  whole of a harness's identity, and it goes solid when that harness has
  something waiting on you.

### Added

- **A one-line shape** (440x44) alongside the pill and the vertical strip,
  cycled by the same button. The headline, the three counts and both quota
  rings stay; the harness chips and the reset clock are what a 44px frame has to
  give up, and the clock moves into the tooltip. For parking the widget
  somewhere you would rather not lose a row of screen to.

- **A second panel view for finished sessions**, on a *finished* tab beside
  *live*: every session this machine can see that is no longer running - 56 of
  the 61 files in `~/.claude/sessions` on the development machine are ghosts,
  and they were previously discarded. A separate view rather than a filter
  because the two questions want opposite layouts: grouped by harness and
  blocked-first for live, flat and reverse-chronological for history. Capped at
  100 per harness, and each row says `ended` plus how long ago, never the
  status the harness happened to die holding.

- **Search in the panel**, over session name, working directory, session id,
  model, terminal title and harness. Space-separated words all have to match,
  so a second word narrows rather than widens. On the finished tab it combines
  with **last active** - today / 7 days / 30 days / all, bucketed on local
  midnight rather than a rolling 24 hours - and the result line says how many
  of how many survived both filters.

  They cannot raise a notification. The snapshot now carries `sessions` and
  `ended` as separate lists and the differ only ever reads the first, which
  makes "a dead pid never announces a finished turn" a structural property
  rather than a filter to remember. Three tests cover it, two of them over the
  real filesystem. Token totals are not read for finished sessions: the
  transcript of a process that exited cannot grow, so it would be a full parse
  of every historical transcript in exchange for a number nobody is waiting for.

- **An experimental macOS build**, shipped as one universal (Apple Silicon +
  Intel) `.dmg`. macOS needs no agent half - the adapters run in-process the way
  they do on Linux - and the app registers as a tray-only accessory, so it keeps
  no Dock icon. The build is unsigned, so Gatekeeper blocks the first launch;
  liveness cannot be checked there either, which is documented in the README.

- **Per-session usage for every harness that reports it**, on each row and
  totalled per harness: tokens everywhere, plus cost where the harness knows it.
  Claude Code's token figures are new - its state file has none, so the adapter
  now tails the session transcript with a byte cursor and folds in only what was
  appended, skipping sidechain lines so subagent turns are not double-counted.
- A **usage pager** in the pill: `‹ CC ›` steps through the detected harnesses,
  defaulting to Claude Code and remembering the choice. Claude's page shows the
  5h/7d plan rings; every other harness shows the tokens and cost it reports.
  Paging rather than blending, because a percentage of a subscription and a
  token count are not comparable figures.
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
