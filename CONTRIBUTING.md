# Contributing

## Adding a harness

Adapters are deliberately dumb: they parse, and return a full snapshot each
tick. Liveness, transitions, dedup and notification delivery all live in
`differ.rs`, once, for every harness.

1. Implement `HarnessAdapter` in `src-tauri/src/adapters/<name>.rs`:
   `detect()` says whether the harness's data root exists on this host, and
   `scan()` returns every session it can see right now.
2. Register it in `adapters::all()`.
3. Pick an honest `FidelityTier`. If the harness does not report a status,
   `PresenceOnly` is the right answer - do not derive a state you cannot know.
   A false "finished" fires a false notification, which is worse than no
   notification.
4. Give the new state a two-letter code and a colour in `src/types/index.ts`.

Rules that exist for a reason, please keep them:

- **Never claim a state you cannot verify.** Recency is not a status.
- **Verify liveness before reporting.** State files outlive their processes:
  on the development machine 23 Claude Code session files existed and 2 were
  live, several frozen mid-turn for months.
- **Identity is the process, not the session id.** A resumed session reuses
  its id under a new pid.

## Local development

```bash
bun install
bun run build
cd src-tauri && cargo test        # no GUI needed
cargo run                          # Linux/WSLg
cargo run -- --agent               # NDJSON snapshots on stdout
```

`./scripts/preview.sh 440 620 preview.png` screenshots the UI through headless
Chrome with fixture data - the fastest way to review a visual change, and the
only way if you are working over SSH. Opened in a plain browser the app renders
that fixture, so every state is visible at once.

## Before opening a PR

- `cargo test` and `cargo clippy --all-targets` clean.
- If you touched the UI, attach a screenshot from `scripts/preview.sh` in both
  themes (`?theme=dark`).
