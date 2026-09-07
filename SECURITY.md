# Security Policy

## Scope

HarnessMonitor reads local files and exposes no network service, so the
realistic risk surface is narrow but not empty:

- **It reads paths derived from the environment** (`HM_HOME`, `HM_AGENT_PATH`,
  `HM_WSL_DISTRO`, `XDG_STATE_HOME`) and from harness data on disk. A report
  showing that a hostile value there can make the app write outside its own
  state directory, or execute something unintended, is in scope.
- **It spawns processes**: `wsl.exe`, its own agent binary, and `herdr`. Anything
  that turns a session name, path, or pane id read from disk into a command is
  in scope.
- **The statusline shim** (`installer/`) wraps a script the user already had and
  writes a state file. Anything that lets it clobber an unrelated file, or break
  the wrapped script in a way that leaks its input, is in scope.
- **The quota state file** holds plan-usage percentages, not credentials. If you
  find any path where a token, key, or transcript content ends up in a file this
  app writes, that is a bug and in scope.

Out of scope: the harnesses themselves (report those upstream), and the fact
that session titles and working directories are shown in a window on your own
screen.

## Reporting

Use GitHub's [private vulnerability reporting](../../security/advisories/new) on
this repository. If that is unavailable to you, email
**arya@badr-interactive.com** with `harness-monitor` in the subject.

Please include the OS and build (Windows ARM64/x64, or Linux), how the app was
launched, and the smallest reproduction you can manage. A log from
`HM_LOG_FILE=<path>` helps a great deal.

Expect an acknowledgement within about a week. This is a spare-time project
maintained by one person, so please do not expect an SLA - but do expect the
issue to be taken seriously, fixed in the open, and credited to you unless you
ask otherwise.

## Supported versions

Only the latest release. There are no backports.
