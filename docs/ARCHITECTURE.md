# Architecture

One crate, one binary, two roles. Everything here exists because of something
the data actually does - the notes explain which.

```
UI role (default)                    agent role (--agent)
  window, tray, notifications          no GUI
  differ (state machine)               runs the adapters
  reads NDJSON  <---- stdout ------    one snapshot per line
```

## Why the Windows build talks to WSL instead of reading files

Path translation is not enough. Three things break when a Windows process reads
WSL data directly:

1. **Liveness cannot be checked.** Claude Code session files carry a
   `pidDomain` like `linux:...:pid:[4026532225]` - a pid inside a Linux pid
   namespace. A Windows process enumerating Windows processes can never verify
   it. Without that check the app shows every stale file as a live session: on
   the development machine, 23 files and 2 live processes, several frozen
   mid-turn since February.
2. **opencode's database is WAL.** Opening it read-only over a 9p share needs
   shared-memory the filesystem cannot provide.
3. **Filesystem notifications do not fire.** `ReadDirectoryChangesW` does not
   work over 9p and inotify does not fire on drvfs `/mnt/c`, so polling is the
   primary mode by design, not a fallback. There is no filesystem watcher in
   this codebase on purpose.

So the Windows UI spawns `wsl.exe -d <distro> -- <binary> --agent` and reads
newline-delimited JSON snapshots from its stdout. The adapters run natively in
Linux, where pids mean something and the database is local. On Linux/WSLg the
same adapters run in-process and the agent role is unused.

## Snapshots, not streams

`HarnessAdapter::scan()` returns every session it can currently see. There is
no streaming trait, because every source except opencode is poll-shaped - state
files, JSON chats, mtimes - and building a streaming abstraction to serve one
adapter inverts the cost. opencode does have an event log, and its adapter
keeps a cursor internally behind `&mut self`; from the outside it still returns
a snapshot.

Adapters only parse. Liveness, transitions, dedup and delivery live in
`differ.rs`, once, for every harness. That is deliberate: staleness needs a
process table, and per-adapter streaming would give every adapter its own place
to get it wrong.

## Identity is the process

`sessionId` is **not** unique - resuming a session reuses it under a new pid
(observed twice on one machine). Identity is
`(harness, pidDomain, pid, procStart)`. Anything keyed on the session id merges
two sessions and cross-fires their notifications.

Liveness compares `procStart` against field 22 of `/proc/<pid>/stat`, parsed
after the **last** `)` because the comm field can contain spaces and parens.
Process names are useless for this: a live Claude Code process is named after
its version (`2.1.259`), not `claude`.

## The differ's rules, and what each one prevents

| Rule | Prevents |
|---|---|
| First snapshot, and the first after an agent respawn, seed silently | 20+ toasts on startup, and a burst every time WSL hiccups |
| Fire on the harness's own `statusUpdatedAt` advancing **and** the state class changing | Re-firing forever on an unchanged re-read; missing a transition that happened between two polls |
| Only two kinds: attention (`-> waiting`) and done (`busy -> idle`) | Announcing sessions appearing, disappearing, or going busy |
| 30s cooldown per session | Flapping between tool calls, the dominant storm source |
| Repeat guard on attention within 5 minutes, unless the reason changed | "Needs you" repeated for the same prompt |
| Global cap of 3 per 10s, rest coalesced | A resume storm burying the desktop |
| A session that fails liveness, or vanishes, is forgotten silently | A deleted state file reading as a completed turn |

## Fidelity is labelled, not averaged

| Harness | State | Usage |
|---|---|---|
| Claude Code | reported by the harness (`status`, `waitingFor`) | tokens, plan window |
| opencode | inferred from the last message plus recency | cost and five token counters |
| codex | inferred from recency; queued prompts visible | `tokens_used` |
| gemini-cli | recency only - it records no status | per-message tokens |
| antigravity | activity only - binary protobuf, no schema | none |

The UI shows the tier on every row, so an inferred state is never mistaken for
a reported one. Two things are deliberately **not** implemented:

- **opencode permission prompts.** The only trace is a log line with no session
  id and no matching resolution event, so a state machine latched on it would
  never unlatch.
- **antigravity state.** An mtime cannot distinguish running from waiting from
  finished, and a false "finished" fires a false notification - worse than none.

## Quota is mirrored, never derived

The 5-hour plan percentage is read from whichever source is newer: the
statusline shim's state file (written on every statusline render) or a `Stop`
hook's analytics JSONL (written at the end of a turn). Both are pushed by a
live Claude Code process, so the reading freezes when no session runs - the UI
then greys it out with its age rather than extrapolating.

It is never computed from token counts. The server's own numbers show why:
`extra_used_credits: 50253` against `extra_limit_credits: 10000`, with the
percentage field clamped at 100.

## Jumping to a session

Adapters know what a session is doing; they cannot know where it is. herdr - a
terminal workspace manager for AI coding agents - does, because its panes
record which agent session they host. `herdr agent list` maps session id to
pane id, and `herdr agent focus <pane>` moves the user there. A session id is
not a valid focus target, hence the mapping. Without herdr the app offers no
jump and says so.
