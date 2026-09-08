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
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::HarnessAdapter;
use crate::liveness::{self, Liveness};
use crate::model::{AgentSession, FidelityTier, HarnessId, SessionKey, SessionState, TokenCounts};
use crate::paths::PathResolver;

#[derive(Default)]
pub struct ClaudeCodeAdapter {
    /// Per session id: how far we have read its transcript, and the totals so
    /// far. Transcripts are append-only, so re-reading from the start every
    /// tick would be pure waste - we keep a byte offset and read only what is
    /// new, the same shape as opencode's event cursor.
    usage: HashMap<String, TranscriptCursor>,
}

#[derive(Default)]
struct TranscriptCursor {
    offset: u64,
    totals: TokenCounts,
}

/// A transcript larger than this is tailed from the end rather than parsed in
/// full, so a months-old session cannot stall a tick. Its earlier tokens are
/// then missing, which is why the UI shows this as a running total rather than
/// a lifetime figure.
const MAX_FULL_PARSE_BYTES: u64 = 64 * 1024 * 1024;

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
                Ok(Some(mut session)) => {
                    // Claude Code's state file has no token counts; those live
                    // in the session transcript.
                    session.tokens = self.read_usage(paths, &session);
                    out.push(session)
                }
                Ok(None) => {}
                Err(err) => tracing::debug!(?path, %err, "skipping unreadable session file"),
            }
        }
        Ok(out)
    }
}

impl ClaudeCodeAdapter {
    fn read_usage(&mut self, paths: &PathResolver, session: &AgentSession) -> Option<TokenCounts> {
        let transcript = paths
            .claude_root()
            .join("projects")
            .join(project_slug(&session.cwd))
            .join(format!("{}.jsonl", session.session_id));

        let cursor = self.usage.entry(session.session_id.clone()).or_default();
        match accumulate(&transcript, cursor) {
            Ok(()) => Some(cursor.totals),
            Err(err) => {
                tracing::debug!(?transcript, %err, "transcript unreadable");
                // Zeroed totals would read as "this session used nothing".
                if cursor.offset == 0 {
                    None
                } else {
                    Some(cursor.totals)
                }
            }
        }
    }
}

/// Claude Code names a project directory after its cwd with every separator
/// flattened to a dash: /mnt/c/D/agent -> -mnt-c-D-agent.
fn project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Reads the bytes appended since last time and folds their usage into the
/// running totals.
fn accumulate(path: &Path, cursor: &mut TranscriptCursor) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();

    if cursor.offset == 0 && len > MAX_FULL_PARSE_BYTES {
        cursor.offset = len;
        return Ok(());
    }
    // Truncated or replaced (a resumed session can rewrite its transcript):
    // start over rather than reading from a meaningless offset.
    if len < cursor.offset {
        cursor.offset = 0;
        cursor.totals = TokenCounts::default();
    }
    if len == cursor.offset {
        return Ok(());
    }

    file.seek(SeekFrom::Start(cursor.offset))?;
    let mut fresh = String::new();
    file.take(len - cursor.offset).read_to_string(&mut fresh)?;

    // A tick can land mid-write, so stop at the last complete line and leave
    // the partial one for next time.
    let complete_to = match fresh.rfind('\n') {
        Some(idx) => idx + 1,
        None => return Ok(()),
    };

    for line in fresh[..complete_to].lines() {
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<TranscriptLine>(line) else {
            continue;
        };
        // Subagent turns are billed to the same account but belong to their
        // own sidechain; counting them here would double-count the parent.
        if entry.is_sidechain.unwrap_or(false) || entry.entry_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(usage) = entry.message.and_then(|m| m.usage) else {
            continue;
        };
        cursor.totals.input += usage.input_tokens.unwrap_or(0);
        cursor.totals.output += usage.output_tokens.unwrap_or(0);
        cursor.totals.cache_read += usage.cache_read_input_tokens.unwrap_or(0);
        cursor.totals.cache_write += usage.cache_creation_input_tokens.unwrap_or(0);
        cursor.totals.reasoning += usage
            .output_tokens_details
            .and_then(|d| d.thinking_tokens)
            .unwrap_or(0);
    }

    cursor.offset += complete_to as u64;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct TranscriptLine {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    message: Option<TranscriptMessage>,
}

#[derive(Debug, Deserialize)]
struct TranscriptMessage {
    usage: Option<TranscriptUsage>,
}

#[derive(Debug, Deserialize)]
struct TranscriptUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    output_tokens_details: Option<OutputDetails>,
}

#[derive(Debug, Deserialize)]
struct OutputDetails {
    thinking_tokens: Option<i64>,
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
