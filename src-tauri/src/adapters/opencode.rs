//! opencode adapter.
//!
//! Source: ~/.local/share/opencode/opencode.db (SQLite, WAL). The `session`
//! table is denormalised - cost and all five token counters sit on the row, so
//! usage needs no JSON parsing and no scan of the 4 GB of message/part blobs.
//!
//! State is weaker than Claude Code's and is labelled as such in the UI:
//! opencode records no status field, so "running" comes from the latest
//! message (`data.finish`, `data.time.completed`) plus recency. Pending
//! permission prompts are deliberately NOT detected - the only trace is a line
//! in log/opencode.log that carries no session id and has no matching resolved
//! event, so anything latched from it would never unlatch.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::HarnessAdapter;
use crate::model::{AgentSession, FidelityTier, HarnessId, SessionKey, SessionState, TokenCounts};
use crate::paths::PathResolver;

/// Sessions untouched for longer than this are not worth showing.
const RECENT_WINDOW_MS: i64 = 12 * 60 * 60 * 1000;
/// A session whose last write is older than this is idle regardless of what
/// the final message says - it may have been killed mid-turn.
const RUNNING_GRACE_MS: i64 = 90 * 1000;
const MAX_SESSIONS: usize = 30;

#[derive(Default)]
pub struct OpenCodeAdapter {
    /// Set once the database has proven unreadable, so a broken or upgraded
    /// schema degrades this one harness instead of spamming every tick.
    degraded: bool,
}

impl HarnessAdapter for OpenCodeAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::OpenCode
    }

    fn detect(&self, paths: &PathResolver) -> bool {
        paths.opencode_db().is_file()
    }

    fn scan(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        if self.degraded {
            return Ok(Vec::new());
        }
        match self.query(&paths.opencode_db()) {
            Ok(sessions) => Ok(sessions),
            Err(err) => {
                self.degraded = true;
                tracing::warn!(%err, "opencode adapter degraded; will stay quiet until restart");
                Ok(Vec::new())
            }
        }
    }
}

impl OpenCodeAdapter {
    fn query(&self, db: &Path) -> Result<Vec<AgentSession>> {
        let conn = open_read_only(db)?;
        let now = crate::model::now_ms();
        let cutoff = now - RECENT_WINDOW_MS;

        let mut stmt = conn.prepare(
            "SELECT id, directory, title, agent, model, cost, tokens_input, tokens_output,
                    tokens_reasoning, tokens_cache_read, tokens_cache_write,
                    time_created, time_updated
             FROM session
             WHERE time_updated > ?1
             ORDER BY time_updated DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map((cutoff, MAX_SESSIONS as i64), |row| {
            Ok(Row {
                id: row.get(0)?,
                directory: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                title: row.get::<_, Option<String>>(2)?,
                agent: row.get::<_, Option<String>>(3)?,
                model: row.get::<_, Option<String>>(4)?,
                cost: row.get::<_, Option<f64>>(5)?,
                tokens: TokenCounts {
                    input: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    output: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    reasoning: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    cache_write: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                },
                time_created: row.get::<_, Option<i64>>(11)?.unwrap_or(0),
                time_updated: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            let row = row?;
            let last = latest_message(&conn, &row.id).unwrap_or(None);
            let state = derive_state(last.as_ref(), row.time_updated, now);
            out.push(AgentSession {
                key: SessionKey {
                    harness: HarnessId::OpenCode,
                    // Not a process: opencode sessions are database rows, so
                    // identity is a stable hash of the row id.
                    pid_domain: "opencode:db".into(),
                    pid: stable_id(&row.id),
                    proc_start: row.time_created.to_string(),
                },
                session_id: row.id,
                cwd: PathBuf::from(row.directory),
                name: row.title.filter(|t| !t.is_empty()),
                state,
                state_changed_at: row.time_updated,
                started_at: row.time_created,
                waiting_for: None,
                model: parse_model(row.model.as_deref()).or(row.agent),
                tokens: Some(row.tokens),
                cost: row.cost,
                is_background: false,
                tier: FidelityTier::UsageOnly,
                jump_target: None,
                terminal_title: None,
            });
        }
        Ok(out)
    }
}

struct Row {
    id: String,
    directory: String,
    title: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    cost: Option<f64>,
    tokens: TokenCounts,
    time_created: i64,
    time_updated: i64,
}

fn open_read_only(db: &Path) -> Result<Connection> {
    let uri = format!("file:{}?mode=ro", db.to_string_lossy());
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("opening opencode.db read-only")
}

#[derive(Debug, Deserialize)]
struct MessageData {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    finish: Option<String>,
    #[serde(default)]
    time: Option<MessageTime>,
}

#[derive(Debug, Deserialize)]
struct MessageTime {
    #[serde(default)]
    completed: Option<i64>,
}

/// `message.id` is a time-sortable ULID, so ORDER BY id DESC is the newest row
/// without touching the table's JSON blobs more than once.
fn latest_message(conn: &Connection, session_id: &str) -> Result<Option<MessageData>> {
    let mut stmt = conn.prepare_cached(
        "SELECT data FROM message WHERE session_id = ?1 ORDER BY id DESC LIMIT 1",
    )?;
    let raw: Option<String> = stmt
        .query_row((session_id,), |row| row.get(0))
        .ok();
    Ok(raw.and_then(|json| serde_json::from_str(&json).ok()))
}

fn derive_state(last: Option<&MessageData>, time_updated: i64, now: i64) -> SessionState {
    // Nothing has been written for a while: whatever the last message says,
    // this session is not currently working.
    if now - time_updated > RUNNING_GRACE_MS {
        return SessionState::Idle;
    }
    match last {
        Some(msg) if msg.role.as_deref() == Some("assistant") => {
            match (msg.finish.as_deref(), msg.time.as_ref().and_then(|t| t.completed)) {
                // A completed turn.
                (Some("stop"), Some(_)) => SessionState::Idle,
                // Mid-turn: either still streaming, or between tool calls.
                _ => SessionState::Running,
            }
        }
        // A user message just landed and no assistant reply exists yet.
        Some(_) => SessionState::Running,
        None => SessionState::ActiveUnknown,
    }
}

/// `model` is stored as a JSON blob, e.g. {"id":"omen-alpha","providerID":"..."}.
fn parse_model(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| v.get("id").and_then(|id| id.as_str()).map(str::to_string))
        .or_else(|| Some(raw.to_string()).filter(|s| !s.is_empty()))
}

/// Logical (non-process) harnesses key on a stable hash of their row/file id.
pub(crate) fn stable_id(session_id: &str) -> i64 {
    let mut hasher = DefaultHasher::new();
    session_id.hash(&mut hasher);
    (hasher.finish() & 0x7fff_ffff_ffff_ffff) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, finish: Option<&str>, completed: Option<i64>) -> MessageData {
        MessageData {
            role: Some(role.into()),
            finish: finish.map(str::to_string),
            time: Some(MessageTime { completed }),
        }
    }

    #[test]
    fn quiet_sessions_are_idle_whatever_the_last_message_says() {
        let now = 1_000_000;
        let stale = now - RUNNING_GRACE_MS - 1;
        assert_eq!(
            derive_state(Some(&msg("assistant", Some("tool-calls"), None)), stale, now),
            SessionState::Idle
        );
    }

    #[test]
    fn finished_turn_is_idle_and_streaming_is_running() {
        let now = 1_000_000;
        let fresh = now - 1_000;
        assert_eq!(
            derive_state(Some(&msg("assistant", Some("stop"), Some(999_000))), fresh, now),
            SessionState::Idle
        );
        assert_eq!(
            derive_state(Some(&msg("assistant", None, None)), fresh, now),
            SessionState::Running
        );
        assert_eq!(
            derive_state(Some(&msg("assistant", Some("tool-calls"), Some(1))), fresh, now),
            SessionState::Running
        );
        assert_eq!(
            derive_state(Some(&msg("user", None, None)), fresh, now),
            SessionState::Running
        );
    }

    #[test]
    fn no_messages_is_never_reported_as_finished() {
        let now = 1_000_000;
        assert_eq!(derive_state(None, now - 1_000, now), SessionState::ActiveUnknown);
    }

    #[test]
    fn model_blob_is_unwrapped() {
        assert_eq!(
            parse_model(Some(r#"{"id":"omen-alpha","providerID":"opencode-go"}"#)).as_deref(),
            Some("omen-alpha")
        );
        assert_eq!(parse_model(Some("gpt-5")).as_deref(), Some("gpt-5"));
        assert_eq!(parse_model(None), None);
    }

    #[test]
    fn ids_are_stable_and_non_negative() {
        assert_eq!(stable_id("ses_abc"), stable_id("ses_abc"));
        assert!(stable_id("ses_abc") >= 0);
        assert_ne!(stable_id("ses_abc"), stable_id("ses_abd"));
    }
}
