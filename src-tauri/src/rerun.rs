//! Starting a finished session's conversation again.
//!
//! The finished list is the one part of the app that knows about sessions with
//! no process behind them, and it deliberately has no jump button - a dead pid
//! has no pane to focus. But "I finished this twenty minutes ago and want to go
//! back into it" is a real question, and a session id plus a working directory
//! is enough to answer it, so this module can.
//!
//! What it cannot do is *replay*. No adapter records the command, the argv or
//! the prompt that started a session, so there is nothing to replay from. The
//! only honest reconstruction is to relaunch the same harness on the same
//! conversation: `claude --resume <id>`, `opencode --session <id>`,
//! `codex resume <id>`. Two of the five harnesses cannot even do that (see
//! `resume_args`), and for those the UI disables the button rather than
//! quietly opening some *other* conversation.
//!
//! Where the new session opens is herdr's business when herdr is installed -
//! `pane split` for a pane at the right cwd, then `agent start` for a
//! readiness-checked launch. Without herdr there is no addressable terminal at
//! all, so the only honest fallback is opening a new one ourselves.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use crate::herdr;
use crate::model::HarnessId;
pub use crate::settings::LaunchTarget;

/// Everything the user can change about the launch. Defaults match what the
/// settings file assumes when it has never been written.
#[derive(Debug, Clone, Copy)]
pub struct RerunOptions {
    pub target: LaunchTarget,
    /// Bring the new pane to the front once it is up.
    pub focus: bool,
    /// How long to wait for the agent to become interactive. herdr's own
    /// default is 30s, which is a reasonable ceiling rather than a good wait.
    pub timeout_ms: u64,
}

impl Default for RerunOptions {
    fn default() -> Self {
        Self {
            target: LaunchTarget::Auto,
            focus: true,
            timeout_ms: 30_000,
        }
    }
}

/// The flags that put a harness back into an existing conversation.
///
/// Arguments only, deliberately without the executable name: herdr's
/// `agent start --kind` supplies its own canonical executable, and passing one
/// too makes the harness read it as a positional - `opencode opencode --session
/// <id>` fails with "Failed to change directory to .../opencode".
///
/// `None` when the harness cannot do it, which is a fact about the harness
/// rather than a limitation of this app, and one the UI states outright.
pub fn resume_args(harness: HarnessId, session_id: &str) -> Option<Vec<String>> {
    let args: &[&str] = match harness {
        // `--resume` takes a session id directly.
        HarnessId::ClaudeCode => &["--resume"],
        // opencode spells it `--session`, and `-c` would pick the wrong one.
        HarnessId::OpenCode => &["--session"],
        // codex's is a subcommand, not a flag.
        HarnessId::Codex => &["resume"],
        // `gemini --resume` accepts "latest" or an index from a list, never a
        // session id. Resuming "latest" would hand back whichever conversation
        // happens to be newest in that directory, which is a different session
        // than the row the user clicked.
        HarnessId::Gemini => return None,
        // No resume flag at all, and the conversation lives under
        // `~/.gemini/antigravity` rather than a project directory.
        HarnessId::Antigravity => return None,
    };
    Some(
        args.iter()
            .map(|a| a.to_string())
            .chain(std::iter::once(session_id.to_string()))
            .collect(),
    )
}

/// The whole command line, executable included.
///
/// Only the no-herdr fallback needs this: it execs the harness itself, so
/// nothing else is going to supply the program name.
pub fn resume_command(harness: HarnessId, session_id: &str) -> Option<Vec<String>> {
    let executable = match harness {
        HarnessId::ClaudeCode => "claude",
        HarnessId::OpenCode => "opencode",
        HarnessId::Codex => "codex",
        HarnessId::Gemini | HarnessId::Antigravity => return None,
    };
    let mut argv = vec![executable.to_string()];
    argv.extend(resume_args(harness, session_id)?);
    Some(argv)
}

/// The herdr `--kind` for a harness, which is not the id our adapters use.
pub fn herdr_kind(harness: HarnessId) -> Option<&'static str> {
    match harness {
        HarnessId::ClaudeCode => Some("claude"),
        HarnessId::OpenCode => Some("opencode"),
        HarnessId::Codex => Some("codex"),
        HarnessId::Gemini | HarnessId::Antigravity => None,
    }
}

/// Resume a session and leave it running in a terminal.
///
/// `harness` and `session_id` come off a finished row; `cwd` is where that row
/// was last known to be working. Every failure returns a string naming what
/// went wrong, because the alternative - a button that silently does nothing -
/// is indistinguishable from a broken app.
pub fn run_again(
    harness: HarnessId,
    session_id: &str,
    cwd: &str,
    opts: RerunOptions,
) -> Result<(), String> {
    if session_id.trim().is_empty() {
        return Err("this row has no session id to reopen".into());
    }
    if cwd.trim().is_empty() {
        return Err("this row has no working directory to reopen it in".into());
    }
    // Checked before the directory, so an unresumable harness says so even on a
    // row whose directory has since been deleted - that is the more useful of
    // the two facts.
    let args = resume_args(harness, session_id)
        .ok_or_else(|| format!("{} cannot reopen a session by its id", harness.label()))?;
    let cwd_path = Path::new(cwd);
    if !cwd_path.is_dir() {
        // Not "just start it anyway": resuming a conversation in a directory
        // that has since been deleted or renamed would silently start a fresh
        // one somewhere else, which is the same lie as gemini's "latest".
        return Err(format!("{cwd} is not a directory any more"));
    }

    let herdr_available = opts.target != LaunchTarget::Terminal && herdr::binary().is_some();
    match opts.target {
        LaunchTarget::Terminal if herdr_available => {
            return Err("herdr is being used for this launch; uncheck Terminal only".into())
        }
        _ if herdr_available => return herdr_launch(harness, session_id, cwd, &args, &opts),
        LaunchTarget::Herdr => return Err("herdr is not installed on this host".into()),
        _ => {}
    }
    // Without herdr nothing supplies the executable, so this is the one path
    // that needs the whole command line.
    let command = resume_command(harness, session_id)
        .ok_or_else(|| format!("{} cannot reopen a session by its id", harness.label()))?;
    launch_terminal(&command, cwd_path)
}

/// Split a pane at `cwd` and start the agent in it, then wait for it to be
/// interactive. Two steps because herdr has no "open a pane running X".
fn herdr_launch(
    harness: HarnessId,
    session_id: &str,
    cwd: &str,
    argv: &[String],
    opts: &RerunOptions,
) -> Result<(), String> {
    let bin = herdr::binary().ok_or_else(|| "herdr not found on this host".to_string())?;
    let kind =
        herdr_kind(harness).ok_or_else(|| format!("herdr cannot host {}", harness.label()))?;

    let mut split = Command::new(&bin);
    split
        .args(["pane", "split", "--direction", "down", "--cwd", cwd])
        // herdr defaults to leaving focus where it is. Focusing a pane that
        // does not exist yet is not a thing, so the flag has to be set here
        // rather than after the agent is up.
        .arg(if opts.focus { "--focus" } else { "--no-focus" });
    let split_out = split
        .output()
        .map_err(|e| format!("running herdr pane split: {e}"))?;
    if !split_out.status.success() {
        return Err(with_stderr(
            "herdr could not open a pane",
            &split_out.stderr,
        ));
    }
    let pane = extract_pane_id(&split_out.stdout)
        .ok_or_else(|| "herdr opened a pane but did not say which one".to_string())?;

    // A new pane is not at a shell prompt the instant it exists - it has to
    // start a shell first, and herdr refuses to start an agent in a pane that
    // is not sitting at one (`agent_pane_busy`). There is no readiness field to
    // poll: `pane get` reports nothing for a bare shell. So herdr's own refusal
    // is the signal, and this waits for the shell to catch up.
    //
    // Retrying only on that one code matters. Any other failure is a real
    // failure, and retrying it would either launch a second agent or hide the
    // reason behind a timeout.
    let deadline = std::time::Instant::now() + Duration::from_millis(SHELL_READY_BUDGET_MS);
    let mut backoff = Duration::from_millis(100);
    loop {
        let mut start = Command::new(&bin);
        start
            .args([
                "agent",
                "start",
                &agent_name(cwd, session_id),
                "--kind",
                kind,
                "--pane",
                &pane,
            ])
            .args(["--timeout", &opts.timeout_ms.to_string()])
            // Everything after a bare `--` goes to the agent itself, which is how
            // `claude --resume <id>` gets through without us parsing a command
            // line.
            .arg("--")
            .args(argv);
        let out = start
            .output()
            .map_err(|e| format!("running herdr agent start: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        if !is_pane_busy(&out.stderr) || std::time::Instant::now() >= deadline {
            // The pane is open at this point either way, so a failure leaves a
            // bare shell behind. Say which pane that is rather than leaving the
            // user to work out what happened to it.
            return Err(with_stderr(
                &format!("{kind} did not come up in the new pane (pane {pane})"),
                &out.stderr,
            ));
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_millis(800));
    }
}

/// Total time to keep re-asking herdr to start the agent while the new pane
/// finishes starting its shell. Generous, because a cold shell on a loaded
/// machine can take a couple of seconds, and bounded because a pane that never
/// becomes ready is a real failure the user needs to hear about.
const SHELL_READY_BUDGET_MS: u64 = 5_000;

/// herdr's refusal to use a pane that is not at an interactive shell prompt.
///
/// Matched on herdr's own error code rather than on the message text, which is
/// the part most likely to be reworded.
fn is_pane_busy(stderr: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(default)]
        error: Option<Failure>,
    }
    #[derive(Deserialize)]
    struct Failure {
        #[serde(default)]
        code: Option<String>,
    }
    serde_json::from_slice::<Envelope>(stderr)
        .ok()
        .and_then(|e| e.error)
        .and_then(|e| e.code)
        .as_deref()
        == Some("agent_pane_busy")
}

/// A label for herdr's agent list. The directory reads better than a uuid at a
/// glance, so it leads and the id only keeps two rows in one directory apart.
fn agent_name(cwd: &str, session_id: &str) -> String {
    let dir = Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "agent".into());
    let short: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(6)
        .collect();
    let name = if short.is_empty() {
        dir
    } else {
        format!("{dir}-{short}")
    };
    // herdr keys agents by name; keep it to something a terminal title can hold.
    name.chars().take(64).collect()
}

/// herdr's `pane split` answers with a `pane_info` envelope. Only `result.pane`
/// is known to carry the id; the other two fields are read because a future
/// herdr that flattens the envelope should not turn into a parse failure.
#[derive(Debug, Deserialize)]
struct SplitEnvelope {
    #[serde(default)]
    result: Option<SplitResult>,
}

#[derive(Debug, Default, Deserialize)]
struct SplitResult {
    #[serde(default)]
    pane: Option<SplitPane>,
    #[serde(default)]
    pane_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SplitPane {
    #[serde(default)]
    pane_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

pub fn extract_pane_id(stdout: &[u8]) -> Option<String> {
    let envelope = serde_json::from_slice::<SplitEnvelope>(stdout).ok()?;
    let result = envelope.result?;
    result
        .pane
        .and_then(|p| p.pane_id.or(p.id))
        .or(result.pane_id)
        .or(result.id)
        .filter(|id| !id.is_empty())
}

/// Terminals to try, in order, with the argv that separates them from the
/// command being run. First one present on PATH wins.
///
/// Ordered by how likely it is to be the thing already on this desktop, not by
/// alphabet: a machine with both gnome-terminal and xterm wants the former.
#[cfg(target_os = "macos")]
const TERMINALS: &[(&str, &[&str])] = &[
    ("Terminal", &[]),
    ("iTerm", &[]),
    ("kitty", &[]),
    ("alacritty", &[]),
];

#[cfg(not(target_os = "macos"))]
const TERMINALS: &[(&str, &[&str])] = &[
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("alacritty", &["-e"]),
    ("kitty", &[]),
    ("wezterm", &["start", "--"]),
    ("foot", &[]),
    ("xterm", &["-e"]),
];

/// No herdr: open a new terminal window ourselves and run the agent in it.
///
/// The fallback is deliberately quieter than the herdr path. There is nothing
/// to wait for - the process is detached and the window comes up whenever the
/// window manager gets to it - so this returns as soon as the launch is
/// accepted, not when the agent is ready.
///
/// The two platforms are separate functions rather than one with `cfg` blocks
/// inside because the mechanism is genuinely different, not cosmetically: a
/// Linux terminal execs a command with a working directory, and macOS has no
/// such thing and must go through AppleScript.
#[cfg(not(target_os = "macos"))]
fn launch_terminal(argv: &[String], cwd: &Path) -> Result<(), String> {
    let Some((program, name, prefix)) = find_terminal() else {
        return Err(no_terminal_message());
    };
    let out = Command::new(program)
        .args(prefix)
        .args(argv)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("running {name}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(with_stderr(
            &format!("{name} refused the command"),
            &out.stderr,
        ))
    }
}

#[cfg(target_os = "macos")]
fn launch_terminal(argv: &[String], cwd: &Path) -> Result<(), String> {
    let Some((_, name, _)) = find_terminal() else {
        return Err(no_terminal_message());
    };
    // No macOS terminal takes a command together with a working directory, and
    // there is no `cd` without a shell. AppleScript is the only way in, which
    // means the whole thing has to survive being pasted into a shell string.
    let script = format!(
        "tell application \"{name}\" to do script \"cd {} && exec {}\"",
        posix_quote(&cwd.to_string_lossy()),
        argv.iter()
            .map(|a| posix_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let osascript = resolve_on_path("osascript").ok_or("osascript not found on this host")?;
    let out = Command::new(osascript)
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("running osascript: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(with_stderr(&format!("could not open {name}"), &out.stderr))
    }
}

fn no_terminal_message() -> String {
    if cfg!(windows) {
        // Reached from inside WSL, where the only terminal worth opening is a
        // Windows one, and getting to it means shelling back out through
        // powershell.exe. Not done: it cannot be tested from the machine this
        // was written on, and a launch that works on the developer's box and
        // silently fails on a user's is worse than an error.
        "herdr is not installed in WSL, and this build will not open a Windows terminal for you"
            .to_string()
    } else {
        "herdr is not installed and no terminal emulator was found on PATH".to_string()
    }
}

/// First candidate terminal that is actually present, as (path, name, prefix).
#[cfg(not(target_os = "macos"))]
fn find_terminal() -> Option<(PathBuf, &'static str, &'static [&'static str])> {
    TERMINALS
        .iter()
        .find_map(|&(program, prefix)| resolve_on_path(program).map(|path| (path, program, prefix)))
}

/// macOS terminals are `.app` bundles: the binary lives inside the bundle and
/// PATH holds nothing useful, so a candidate is matched as a bundle on disk
/// and driven through AppleScript rather than executed. The path is therefore
/// unused on this platform and left empty.
#[cfg(target_os = "macos")]
fn find_terminal() -> Option<(PathBuf, &'static str, &'static [&'static str])> {
    TERMINALS
        .iter()
        .find(|&&(name, _)| app_bundle_exists(name))
        .map(|&(name, prefix)| (PathBuf::new(), name, prefix))
}

#[cfg(target_os = "macos")]
fn app_bundle_exists(name: &str) -> bool {
    [
        "/System/Applications",
        "/Applications",
        "/System/Applications/Utilities",
        "/Applications/Utilities",
    ]
    .iter()
    .any(|dir| Path::new(dir).join(format!("{name}.app")).exists())
}

/// Find an executable on PATH.
///
/// A `~/.local/bin` that a login shell adds but a non-interactive one does not
/// is the same problem `herdr::binary` exists to solve, so this walks PATH
/// itself rather than trusting the environment we were handed.
pub fn resolve_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(program);
        is_executable(&candidate).then_some(candidate)
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Single-quote for a POSIX shell. The values here are harness paths and
/// session ids rather than free text, but a working directory containing an
/// apostrophe is legal and would otherwise end the quote early.
#[cfg(target_os = "macos")]
fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn with_stderr(context: &str, stderr: &[u8]) -> String {
    let raw = String::from_utf8_lossy(stderr);
    let detail = raw.trim();
    if detail.is_empty() {
        context.to_string()
    } else {
        // Enough to identify the failure, short enough for a 440px panel.
        let detail: String = detail.chars().take(200).collect();
        format!("{context}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_flags_match_each_cli() {
        // Arguments only. herdr's `agent start --kind` supplies the executable
        // itself, and sending one too made opencode read it as a directory to
        // change into.
        assert_eq!(
            resume_args(HarnessId::ClaudeCode, "abc").unwrap(),
            vec!["--resume", "abc"]
        );
        assert_eq!(
            resume_args(HarnessId::OpenCode, "ses_1").unwrap(),
            vec!["--session", "ses_1"]
        );
        assert_eq!(
            resume_args(HarnessId::Codex, "0199e96c").unwrap(),
            vec!["resume", "0199e96c"]
        );
    }

    #[test]
    fn the_terminal_fallback_gets_the_executable_too() {
        // The same flags, prefixed - because on that path nothing else is going
        // to run the program for us.
        assert_eq!(
            resume_command(HarnessId::OpenCode, "ses_1").unwrap(),
            vec!["opencode", "--session", "ses_1"]
        );
        assert_eq!(
            resume_command(HarnessId::Codex, "abc").unwrap(),
            vec!["codex", "resume", "abc"]
        );
        assert!(resume_command(HarnessId::Gemini, "1").is_none());
    }

    #[test]
    fn harnesses_that_cannot_resume_by_id_say_so() {
        // gemini takes "latest" or an index; antigravity has no flag. Returning
        // None is what keeps the button disabled rather than quietly opening a
        // different conversation.
        assert!(resume_args(HarnessId::Gemini, "5").is_none());
        assert!(resume_args(HarnessId::Antigravity, "5").is_none());
        assert!(herdr_kind(HarnessId::Gemini).is_none());
    }

    #[test]
    fn reads_the_pane_id_from_a_real_split_envelope() {
        // Captured from `herdr pane split --direction down --cwd /tmp --no-focus`.
        let raw = br#"{"id":"cli:pane:split","result":{"pane":{"agent_status":"unknown","cwd":"/tmp","focused":false,"pane_id":"w0:p5","tab_id":"w0:t1","workspace_id":"w0"},"type":"pane_info"}}"#;
        assert_eq!(extract_pane_id(raw).as_deref(), Some("w0:p5"));
    }

    #[test]
    fn tolerates_a_flattened_envelope() {
        let flat = br#"{"result":{"pane_id":"w1:p2"}}"#;
        assert_eq!(extract_pane_id(flat).as_deref(), Some("w1:p2"));
        let nested_id = br#"{"result":{"pane":{"id":"w1:p3"}}}"#;
        assert_eq!(extract_pane_id(nested_id).as_deref(), Some("w1:p3"));
    }

    #[test]
    fn a_pane_id_is_required_not_guessed() {
        assert_eq!(extract_pane_id(b"{}"), None);
        assert_eq!(
            extract_pane_id(br#"{"result":{"pane":{"pane_id":""}}}"#),
            None
        );
        assert_eq!(extract_pane_id(b"not json"), None);
    }

    #[test]
    fn agent_name_leads_with_the_directory() {
        assert_eq!(
            agent_name("/home/you/code/checkout", "abc123def"),
            "checkout-abc123"
        );
        // A root or trailing-slash path has no last segment to lead with.
        assert_eq!(agent_name("/", "abc123"), "agent-abc123");
        // Nothing worth naming in the id: the directory is the whole name.
        assert_eq!(
            agent_name("/code/harness-monitor", "---"),
            "harness-monitor"
        );
    }

    #[test]
    fn a_missing_directory_is_refused_rather_than_guessed() {
        let err = run_again(
            HarnessId::ClaudeCode,
            "abc",
            "/definitely/not/a/directory",
            RerunOptions::default(),
        )
        .unwrap_err();
        assert!(err.contains("not a directory"), "{err}");
    }

    #[test]
    fn a_row_without_a_session_id_is_refused() {
        let err =
            run_again(HarnessId::ClaudeCode, "  ", "/tmp", RerunOptions::default()).unwrap_err();
        assert!(err.contains("no session id"), "{err}");
    }

    #[test]
    fn an_unresumable_harness_is_named_in_the_error() {
        let err = run_again(HarnessId::Gemini, "5", "/tmp", RerunOptions::default()).unwrap_err();
        assert!(err.contains("gemini-cli"), "{err}");
    }

    #[test]
    fn only_a_pane_still_starting_is_worth_retrying() {
        // Captured from a real `herdr agent start` against a pane that had been
        // split a few milliseconds earlier.
        let busy = br#"{"error":{"code":"agent_pane_busy","message":"agent target pane w0:p9 is not an available shell"},"id":"cli:agent:start"}"#;
        assert!(is_pane_busy(busy));

        // Everything else is a real failure and must reach the panel rather than
        // be retried into a timeout that says nothing.
        for other in [
            &br#"{"error":{"code":"agent_start_timeout","message":"timed out"}}"#[..],
            &br#"{"error":{"code":"agent_pane_busy_extra","message":"x"}}"#[..],
            &br#"{"error":{"message":"no code at all"}}"#[..],
            &br#"{"result":{"type":"agent_started"}}"#[..],
            b"not json",
            b"",
        ] {
            assert!(!is_pane_busy(other), "{other:?} should not be retried");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn quoting_survives_an_apostrophe_in_the_path() {
        assert_eq!(posix_quote("/home/you/o'brien"), r"'/home/you/o'\''brien'");
        assert_eq!(posix_quote("plain"), "'plain'");
    }
}
