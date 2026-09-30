//! Snapshot source. Two shapes, one output channel.
//!
//! Linux/WSLg and macOS builds: adapters run in-process.
//! Windows build: the same binary is launched inside WSL with `--agent` and
//! streams NDJSON back over stdout. Nothing reads `\\wsl.localhost`.
//!
//! Every (re)spawn marks its first snapshot `reseed`, so a WSL hiccup replays
//! the baseline instead of firing a toast per session.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::adapters;
use crate::model::{HarnessId, Snapshot};
use crate::scanner::Scanner;

/// Plain threads and a std channel on purpose: the pipeline is a slow poll
/// loop over blocking filesystem reads, and putting it on Tauri's async
/// runtime bought nothing but a scheduler to be wrong about.
///
/// The scan settings are parameters rather than something read from disk in
/// here, because on Windows the caller is on the other side of WSL and there is
/// no settings file this process could reach. That is also why changing them
/// needs a restart of the app: they are fixed at spawn.
pub fn spawn_source(
    tx: Sender<Snapshot>,
    interval_ms: u64,
    enabled: &[HarnessId],
    max_ended: usize,
) {
    let enabled: Vec<String> = enabled
        .iter()
        .filter_map(|h| serde_json::to_value(h).ok())
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    if cfg!(windows) {
        std::thread::spawn(move || run_wsl_agent(tx, interval_ms, enabled, max_ended));
    } else {
        adapters::configure(max_ended, &enabled);
        std::thread::spawn(move || run_local(tx, interval_ms));
    };
}

fn run_local(tx: Sender<Snapshot>, interval_ms: u64) {
    let mut scanner = Scanner::new();
    tracing::info!(detected = ?scanner.detected(), "local scanner ready");
    loop {
        let snapshot = scanner.tick();
        if tx.send(snapshot).is_err() {
            return; // UI gone
        }
        std::thread::sleep(Duration::from_millis(interval_ms));
    }
}

fn run_wsl_agent(tx: Sender<Snapshot>, interval_ms: u64, enabled: Vec<String>, max_ended: usize) {
    let mut backoff_ms = 500u64;
    loop {
        let cfg = WslConfig::resolve();
        tracing::info!(distro = %cfg.distro, agent = %cfg.agent_path, "starting wsl agent");

        let mut command = std::process::Command::new("wsl.exe");
        command
            .args([
                "-d",
                &cfg.distro,
                "--",
                &cfg.agent_path,
                "--agent",
                "--interval-ms",
            ])
            .arg(interval_ms.to_string())
            .args(["--max-ended", &max_ended.to_string()])
            .args(["--harnesses", &enabled.join(",")])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        hide_console(&mut command);
        let child = command.spawn();

        match child {
            Ok(mut child) => {
                backoff_ms = 500;
                if let Some(stdout) = child.stdout.take() {
                    let mut first = true;
                    for line in std::io::BufReader::new(stdout).lines() {
                        let Ok(line) = line else { break };
                        match serde_json::from_str::<Snapshot>(&line) {
                            Ok(mut snapshot) => {
                                // First snapshot of every (re)spawn is a
                                // baseline, not a diff - otherwise a WSL
                                // hiccup fires one toast per live session.
                                snapshot.reseed = std::mem::take(&mut first);
                                if tx.send(snapshot).is_err() {
                                    let _ = child.kill();
                                    return;
                                }
                            }
                            Err(err) => tracing::warn!(%err, "unparseable agent line"),
                        }
                    }
                }
                let _ = child.wait();
                tracing::warn!("wsl agent exited, restarting");
            }
            Err(err) => tracing::error!(%err, "failed to spawn wsl agent"),
        }

        std::thread::sleep(Duration::from_millis(backoff_ms));
        backoff_ms = (backoff_ms * 2).min(30_000);
    }
}

/// One-shot: ask the WSL-side copy of this binary to focus a pane.
pub fn focus_via_wsl(target: &str) -> Result<(), String> {
    run_wsl_one_shot(&["--focus", target])
}

/// One-shot: ask the WSL-side copy of this binary to reopen a session.
///
/// The resume options travel as arguments for the same reason they exist on the
/// command line at all: the settings file lives in the Windows user's AppData,
/// which the process inside WSL has no reason to be able to read. `hide_console`
/// matters more here than for `--focus` - this one can spawn a terminal.
pub fn run_again_via_wsl(
    harness: &str,
    session_id: &str,
    cwd: &str,
    target: &str,
    focus: bool,
    timeout_ms: u64,
) -> Result<(), String> {
    let focus_flag = if focus {
        "--rerun-focus"
    } else {
        "--no-rerun-focus"
    };
    let timeout = timeout_ms.to_string();
    run_wsl_one_shot(&[
        "--run-again",
        harness,
        session_id,
        cwd,
        "--rerun-target",
        target,
        focus_flag,
        "--rerun-timeout-ms",
        &timeout,
    ])
}

/// `wsl.exe -d <distro> -- <binary> <args>`, waiting for it to finish.
fn run_wsl_one_shot(args: &[&str]) -> Result<(), String> {
    let cfg = WslConfig::resolve();
    let mut command = std::process::Command::new("wsl.exe");
    command
        .args(["-d", &cfg.distro, "--", &cfg.agent_path])
        .args(args);
    hide_console(&mut command);
    let output = command
        .output()
        .map_err(|e| format!("spawning wsl.exe: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    // The WSL-side binary writes the reason to stderr before exiting 1, so this
    // is the actual failure rather than a bare non-zero status.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        return Err("the WSL-side agent exited without saying why".to_string());
    }
    Err(stderr.chars().take(200).collect())
}

#[derive(Debug, Clone)]
pub struct WslConfig {
    pub distro: String,
    /// Linux-side path to this same binary.
    pub agent_path: String,
}

impl WslConfig {
    fn resolve() -> Self {
        let distro = std::env::var("HM_WSL_DISTRO")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(default_distro)
            .unwrap_or_else(|| "Ubuntu".into());
        let agent_path = std::env::var("HM_AGENT_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(default_agent_path)
            .unwrap_or_else(|| "harness-monitor".into());
        Self { distro, agent_path }
    }
}

/// Without this, every `wsl.exe` call flashes (or parks) a console window on
/// the desktop - the agent is meant to be invisible plumbing.
#[cfg(windows)]
fn hide_console(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut std::process::Command) {}

/// First entry of `wsl.exe -l -q`. Output is UTF-16LE, hence the NUL strip.
fn default_distro() -> Option<String> {
    let mut command = std::process::Command::new("wsl.exe");
    command.args(["-l", "-q"]);
    hide_console(&mut command);
    let out = command.output().ok()?;
    let text: String = String::from_utf8_lossy(&out.stdout)
        .chars()
        .filter(|c| *c != '\0' && *c != '\r')
        .collect();
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|s| s.to_string())
}

/// The Linux agent binary ships next to the .exe as `harness-monitor-agent`.
fn default_agent_path() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let sibling = exe.with_file_name("harness-monitor-agent");
    windows_path_to_wsl(&sibling)
}

pub fn windows_path_to_wsl(path: &Path) -> Option<String> {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut chars = s.chars();
    let drive = chars.next()?.to_ascii_lowercase();
    if !drive.is_ascii_alphabetic() || chars.next() != Some(':') {
        return None;
    }
    let rest: String = chars.collect();
    Some(format!("/mnt/{drive}{rest}"))
}

pub fn agent_binary_hint() -> PathBuf {
    std::env::current_exe()
        .map(|p| p.with_file_name("harness-monitor-agent"))
        .unwrap_or_else(|_| PathBuf::from("harness-monitor-agent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_windows_paths() {
        assert_eq!(
            windows_path_to_wsl(Path::new(r"C:\code\harness-monitor-agent")).as_deref(),
            Some("/mnt/c/code/harness-monitor-agent")
        );
        assert_eq!(
            windows_path_to_wsl(Path::new(r"D:\tools\x")).as_deref(),
            Some("/mnt/d/tools/x")
        );
    }

    #[test]
    fn rejects_non_drive_paths() {
        assert_eq!(windows_path_to_wsl(Path::new(r"\\server\share\x")), None);
        assert_eq!(windows_path_to_wsl(Path::new("/already/linux")), None);
    }

    #[test]
    fn the_agent_hint_is_a_sibling_of_the_running_binary() {
        // The UI process ships next to the agent it has to launch inside WSL,
        // and the name is what the release workflow produces.
        let hint = agent_binary_hint();
        assert_eq!(hint.file_name().unwrap(), "harness-monitor-agent");
        // Never the UI binary itself: spawning that with --agent would open a
        // second window instead of streaming snapshots.
        assert_ne!(hint, std::env::current_exe().unwrap_or_default());
    }

    #[test]
    fn a_lowercase_drive_letter_is_translated_too() {
        // Explorer produces `c:\`, not `C:\`, when a path is copied.
        assert_eq!(
            windows_path_to_wsl(Path::new(r"c:\code\agent")).as_deref(),
            Some("/mnt/c/code/agent")
        );
    }

    #[test]
    fn a_bare_drive_root_keeps_its_trailing_separator() {
        // The separator survives, so the result is still a directory and not a
        // file that happens to be named like one.
        assert_eq!(
            windows_path_to_wsl(Path::new(r"C:\")).as_deref(),
            Some("/mnt/c/")
        );
    }
}
