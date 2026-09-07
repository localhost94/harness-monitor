//! Snapshot source. Two shapes, one output channel.
//!
//! Linux/WSLg build: adapters run in-process.
//! Windows build: the same binary is launched inside WSL with `--agent` and
//! streams NDJSON back over stdout. Nothing reads `\\wsl.localhost`.
//!
//! Every (re)spawn marks its first snapshot `reseed`, so a WSL hiccup replays
//! the baseline instead of firing a toast per session.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::model::Snapshot;
use crate::scanner::Scanner;

/// Plain threads and a std channel on purpose: the pipeline is a slow poll
/// loop over blocking filesystem reads, and putting it on Tauri's async
/// runtime bought nothing but a scheduler to be wrong about.
pub fn spawn_source(tx: Sender<Snapshot>, interval_ms: u64) {
    if cfg!(windows) {
        std::thread::spawn(move || run_wsl_agent(tx, interval_ms));
    } else {
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

fn run_wsl_agent(tx: Sender<Snapshot>, interval_ms: u64) {
    let mut backoff_ms = 500u64;
    loop {
        let cfg = WslConfig::resolve();
        tracing::info!(distro = %cfg.distro, agent = %cfg.agent_path, "starting wsl agent");

        let mut command = std::process::Command::new("wsl.exe");
        command
            .args(["-d", &cfg.distro, "--", &cfg.agent_path, "--agent", "--interval-ms"])
            .arg(interval_ms.to_string())
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
    let cfg = WslConfig::resolve();
    let mut command = std::process::Command::new("wsl.exe");
    command.args(["-d", &cfg.distro, "--", &cfg.agent_path, "--focus", target]);
    hide_console(&mut command);
    let output = command.output().map_err(|e| format!("spawning wsl.exe: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr)
        .trim()
        .chars()
        .take(200)
        .collect())
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
}
