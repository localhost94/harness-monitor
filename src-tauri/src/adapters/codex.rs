//! codex adapter.
//!
//! codex keeps everything in SQLite under ~/.codex (on this machine that is
//! the Windows profile, reachable from WSL at /mnt/c/Users/<user>/.codex).
//! `state_5.sqlite` holds threads with `tokens_used`; `queue_1.sqlite` holds
//! prompts the user has queued.
//!
//! Caveat worth knowing: on a current build `threads` can be empty while codex
//! is plainly running and writing to logs_2.sqlite. When that happens we
//! report a single presence row rather than an empty list - labelled
//! presence-only, never given a fake state.

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

use super::HarnessAdapter;
use crate::model::{now_ms, AgentSession, FidelityTier, HarnessId, SessionKey, SessionState, TokenCounts};
use crate::paths::PathResolver;

const RECENT_WINDOW_MS: i64 = 12 * 60 * 60 * 1000;
const RUNNING_GRACE_MS: i64 = 90 * 1000;
/// How recently the log database must have been written for codex to count as
/// present at all.
const PRESENCE_WINDOW_MS: i64 = 5 * 60 * 1000;

#[derive(Default)]
pub struct CodexAdapter {
    degraded: bool,
}

impl HarnessAdapter for CodexAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Codex
    }

    fn detect(&self, paths: &PathResolver) -> bool {
        paths.codex_root().is_some()
    }

    fn scan(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        if self.degraded {
            return Ok(Vec::new());
        }
        let Some(root) = paths.codex_root() else {
            return Ok(Vec::new());
        };
        let now = now_ms();

        let threads = match read_threads(&root, now) {
            Ok(rows) => rows,
            Err(err) => {
                self.degraded = true;
                tracing::warn!(%err, "codex adapter degraded; will stay quiet until restart");
                return Ok(Vec::new());
            }
        };
        if !threads.is_empty() {
            return Ok(threads);
        }
        Ok(presence_row(&root, now).into_iter().collect())
    }
}

fn open_ro(path: &Path) -> Result<Connection> {
    let uri = format!("file:{}?mode=ro", path.to_string_lossy());
    Ok(Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn read_threads(root: &Path, now: i64) -> Result<Vec<AgentSession>> {
    let db = root.join("state_5.sqlite");
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let conn = open_ro(&db)?;
    let queued = queued_thread_ids(root).unwrap_or_default();

    let mut stmt = conn.prepare(
        "SELECT id, cwd, name, title, model, tokens_used, created_at_ms, updated_at_ms, archived
         FROM threads
         WHERE archived = 0 AND updated_at_ms > ?1
         ORDER BY updated_at_ms DESC
         LIMIT 30",
    )?;
    let rows = stmt.query_map((now - RECENT_WINDOW_MS,), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            row.get::<_, Option<i64>>(6)?.unwrap_or(0),
            row.get::<_, Option<i64>>(7)?.unwrap_or(0),
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, cwd, name, title, model, tokens_used, created, updated) = row?;
        // A queued prompt means codex is holding work the user submitted; that
        // is the closest thing codex exposes to "waiting", so it is reported
        // as running rather than idle.
        let state = if now - updated <= RUNNING_GRACE_MS || queued.contains(&id) {
            SessionState::Running
        } else {
            SessionState::Idle
        };
        out.push(AgentSession {
            key: SessionKey {
                harness: HarnessId::Codex,
                pid_domain: "codex:db".into(),
                pid: super::opencode::stable_id(&id),
                proc_start: created.to_string(),
            },
            session_id: id,
            cwd: PathBuf::from(cwd),
            name: name.or(title),
            state,
            state_changed_at: updated,
            started_at: created,
            waiting_for: None,
            model,
            tokens: Some(TokenCounts {
                input: tokens_used,
                ..Default::default()
            }),
            cost: None,
            is_background: false,
            tier: FidelityTier::UsageOnly,
            jump_target: None,
            terminal_title: None,
        });
    }
    Ok(out)
}

fn queued_thread_ids(root: &Path) -> Result<Vec<String>> {
    let db = root.join("queue_1.sqlite");
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let conn = open_ro(&db)?;
    let mut stmt = conn.prepare("SELECT DISTINCT thread_id FROM queued_items")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .collect();
    Ok(ids)
}

/// codex is clearly alive (its log database was just written) but exposes no
/// thread rows. Say exactly that instead of inventing a session.
fn presence_row(root: &Path, now: i64) -> Option<AgentSession> {
    let touched = [ "logs_2.sqlite-wal", "logs_2.sqlite" ]
        .iter()
        .filter_map(|name| modified_ms(&root.join(name)))
        .max()?;
    if now - touched > PRESENCE_WINDOW_MS {
        return None;
    }
    Some(AgentSession {
        key: SessionKey {
            harness: HarnessId::Codex,
            pid_domain: "codex:presence".into(),
            pid: 0,
            proc_start: "presence".into(),
        },
        session_id: "codex-activity".into(),
        cwd: root.to_path_buf(),
        name: Some("codex".into()),
        state: SessionState::ActiveUnknown,
        state_changed_at: touched,
        started_at: touched,
        waiting_for: None,
        model: None,
        tokens: None,
        cost: None,
        is_background: false,
        tier: FidelityTier::PresenceOnly,
        jump_target: None,
        terminal_title: None,
    })
}

pub(crate) fn modified_ms(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_millis() as i64)
}
