# Contributing

## Adding a harness

Adapters are deliberately dumb: they parse, and return a full snapshot each
tick. Liveness, transitions, dedup and notification delivery all live in
`differ.rs`, once, for every harness.

1. Implement `HarnessAdapter` in `src-tauri/src/adapters/<name>.rs`:
   `detect()` says whether the harness's data root exists on this host,
   `scan()` returns every session it can see right now, and the optional
   `scan_ended()` returns the ones that are over.
2. Register it in `adapters::all()`.
3. Pick an honest `FidelityTier`. If the harness does not report a status,
   `PresenceOnly` is the right answer - do not derive a state you cannot know.
   A false "finished" fires a false notification, which is worse than no
   notification.
4. Give the new harness a two-letter code and a label in
   `src/types/index.ts`. There is no colour step: the interface is black and
   white, and the two letters are the whole of a harness's identity.

Rules that exist for a reason, please keep them:

- **Never claim a state you cannot verify.** Recency is not a status.
- **`scan()` is the only thing that reaches the differ.** Put anything that is
  not currently running in `scan_ended()` instead - including anything whose
  liveness this host cannot check, which is most things on macOS. The two
  lists are separate so that a dead pid has no code path to a notification.
  The default `scan_ended()` returns nothing, which is the honest answer for a
  harness that only exposes its current sessions.
- **Decide which list a row belongs to in one place.** `parse_session_file` in
  the Claude Code adapter returns the row and the decision together. Deciding
  separately in the two scan methods is how a session ends up both "running"
  and "ended" in the same snapshot.
- **Verify liveness before reporting.** State files outlive their processes:
  on the development machine 61 Claude Code session files existed and 5 were
  live, several frozen mid-turn for months.
- **Identity is the process, not the session id.** A resumed session reuses
  its id under a new pid.

## Local development

```bash
bun install
bun run build
bun run test                 # frontend: vitest, no GUI needed
cd src-tauri && cargo test   # backend: no GUI needed
cargo run                    # Linux/WSLg and macOS
cargo run -- --agent         # NDJSON snapshots on stdout
```

### Tests

The two halves are tested separately and neither needs a window.

| | Command | What it covers |
|---|---|---|
| Frontend | `bun run test` | `src/lib` (format, search, theme), the store, all 9 components, `App`. jsdom, with `@tauri-apps/api` mocked by `src/test/setup.ts`. |
| Frontend coverage | `bun run test:coverage` | v8, into `coverage/`; JSON is what CI uploads. |
| Backend | `cd src-tauri && cargo test` | adapters, differ, scanner, model, quota, paths, placement, bridge. |
| Backend coverage | `cd src-tauri && cargo llvm-cov --all-targets --lcov --output-path coverage-lcov.info` | needs `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`. |

`bun run test:watch` runs the frontend suite interactively.

Two things to know before adding a test:

- **`format.stamp` and `dayAndTime` read the machine clock and locale.** The
  timezone is pinned to UTC in `vite.config.ts`; the assertions are written
  against components, not whole strings, so an ICU upgrade is not a red build.
- **Anything that reads the environment is not directly testable.**
  `PathResolver::for_home` still resolves the Windows profile from the real
  `/mnt/c/Users`, so the codex fallback and antigravity go through
  `PathResolver::with_windows_home` instead. If you add a module that resolves a
  path or reads a clock at the call site, take the path or the clock as a
  parameter so it can be tested at all.

Coverage is reported to [Codecov](https://codecov.io/gh/localhost94/harness-monitor)
as two flags, `frontend` and `backend`, merged into one badge. Public repository,
so the upload is tokenless; if it ever goes private, add a `CODECOV_TOKEN`
secret and the workflow picks it up unchanged.

Per-OS prerequisites, and how to produce an installer for each, are in the
README's [Build and run](README.md#build-and-run) section. Short version:
Linux needs the webkit2gtk dev packages, macOS needs the Xcode command line
tools, Windows needs MSVC plus WebView2 and is built through
`scripts/build-windows.sh` because it ships a Linux agent alongside the exe.

`./scripts/preview.sh 440 620 preview.png` screenshots the UI through headless
Chrome with fixture data - the fastest way to review a visual change, and the
only way if you are working over SSH. Opened in a plain browser the app renders
that fixture, so every state is visible at once. Any further arguments are
passed through as query params, which is how the variants get previewed:

```bash
./scripts/preview.sh 440 620 preview.png "shape=line"          # the one-line widget
./scripts/preview.sh 118 400 preview.png "shape=vertical"      # the strip
./scripts/preview.sh 440 620 preview.png "view=finished"       # the history tab
./scripts/preview.sh 440 620 preview.png "view=finished&q=api" # ...searched
./scripts/preview.sh 440 620 preview.png "dates=today"         # ...by date range
./scripts/preview.sh 440 620 preview.png "many=12"             # a crowded list
```

## Before opening a PR

- `bun run test` and `bun run build` clean.
- `cargo test` and `cargo clippy --all-targets` clean. Clippy runs over test
  code too, with `-D warnings`.
- If you touched the UI, attach a screenshot from `scripts/preview.sh` in both
  themes (`?theme=dark`).
