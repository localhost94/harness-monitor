//! Claude Code adapter.
//!
//! Source: ~/.claude/sessions/<pid>.json, one file per CLI process, rewritten
//! by the harness itself. It carries an authoritative `status` and
//! `waitingFor`, so "finished" vs "needs you" is read, never guessed.
//!
//! Two things this file must get right:
//!   1. dead pids are dropped (see liveness.rs) - most files are ghosts;
//!   2. identity is the process, not `sessionId`, which repeats across pids
//!      when a session is resumed.

use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

use super::HarnessAdapter;
use crate::liveness::{self, Liveness};
use crate::model::{AgentSession, FidelityTier, HarnessId, SessionKey, SessionState};
use crate::paths::PathResolver;

pub struct ClaudeCodeAdapter;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    pid: i64,
    session_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    started_at: Option<i64>,
    #[serde(default)]
    proc_start: Option<String>,
    #[serde(default)]
    pid_domain: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    waiting_for: Option<String>,
    #[serde(default)]
    status_updated_at: Option<i64>,
    #[serde(default)]
    updated_at: Option<i64>,
}

impl HarnessAdapter for ClaudeCodeAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::ClaudeCode
    }

    fn detect(&self, paths: &PathResolver) -> bool {
        paths.claude_root().is_dir()
    }

    fn scan(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        let dir = paths.claude_sessions();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return Ok(Vec::new()),
        };

        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue; // sibling <pid>.<sha>.key files
            }
            match parse_session_file(&path) {
                Ok(Some(session)) => out.push(session),
                Ok(None) => {}
                Err(err) => tracing::debug!(?path, %err, "skipping unreadable session file"),
            }
        }
        Ok(out)
    }
}

fn parse_session_file(path: &PathBuf) -> Result<Option<AgentSession>> {
    let raw = std::fs::read_to_string(path)?;
    let file: SessionFile = serde_json::from_str(&raw)?;

    let proc_start = file.proc_start.unwrap_or_default();
    // A file with no procStart cannot be verified; treat it as a ghost rather
    // than risk reporting a months-old "busy" session as live.
    if proc_start.is_empty() {
        return Ok(None);
    }
    if liveness::check(file.pid, &proc_start) == Liveness::Dead {
        return Ok(None);
    }

    let state = map_state(file.status.as_deref(), file.waiting_for.as_deref());
    let changed_at = file
        .status_updated_at
        .or(file.updated_at)
        .or(file.started_at)
        .unwrap_or(0);

    Ok(Some(AgentSession {
        key: SessionKey {
            harness: HarnessId::ClaudeCode,
            pid_domain: file.pid_domain.unwrap_or_else(|| "local".into()),
            pid: file.pid,
            proc_start,
        },
        session_id: file.session_id,
        cwd: PathBuf::from(file.cwd.unwrap_or_default()),
        name: file.name,
        state,
        state_changed_at: changed_at,
        started_at: file.started_at.unwrap_or(changed_at),
        waiting_for: file.waiting_for,
        model: None,
        tokens: None,
        cost: None,
        is_background: file.kind.as_deref() == Some("bg"),
        tier: FidelityTier::Full,
        jump_target: None,
        terminal_title: None,
    }))
}

fn map_state(status: Option<&str>, waiting_for: Option<&str>) -> SessionState {
    match status {
        Some("busy") => SessionState::Running,
        Some("waiting") => match waiting_for {
            Some(w) if w.contains("permission") => SessionState::AwaitingPermission,
            _ => SessionState::AwaitingInput,
        },
        Some("shell") => SessionState::Shell,
        Some("idle") => SessionState::Idle,
        _ => SessionState::ActiveUnknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_prompt_is_its_own_state() {
        assert_eq!(
            map_state(Some("waiting"), Some("permission prompt")),
            SessionState::AwaitingPermission
        );
        assert_eq!(
            map_state(Some("waiting"), Some("input needed")),
            SessionState::AwaitingInput
        );
        // waiting with no reason still means the user is being waited on
        assert_eq!(map_state(Some("waiting"), None), SessionState::AwaitingInput);
    }

    #[test]
    fn known_statuses_map_directly() {
        assert_eq!(map_state(Some("busy"), None), SessionState::Running);
        assert_eq!(map_state(Some("idle"), None), SessionState::Idle);
        assert_eq!(map_state(Some("shell"), None), SessionState::Shell);
    }

    #[test]
    fn unknown_status_never_claims_idle() {
        // Claiming Idle would fire a false "done" toast.
        assert_eq!(map_state(Some("teleporting"), None), SessionState::ActiveUnknown);
        assert_eq!(map_state(None, None), SessionState::ActiveUnknown);
    }
}
