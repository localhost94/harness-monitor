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
use crate::liveness::Liveness;
use crate::model::{
    now_ms, AgentSession, FidelityTier, HarnessId, SessionKey, SessionState, TokenCounts,
};
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

    /// Threads older than the recency window, newest first.
    ///
    /// codex threads are database rows with no process behind them, so "ended"
    /// is the same judgement the live query makes by omission - these are the
    /// threads it deliberately does not show.
    fn scan_ended(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        let Some(root) = paths.codex_root() else {
            return Ok(Vec::new());
        };
        match read_threads_ended(&root, now_ms()) {
            Ok(rows) => Ok(rows),
            Err(err) => {
                tracing::warn!(%err, "codex ended-session query failed");
                Ok(Vec::new())
            }
        }
    }
}

fn open_ro(path: &Path) -> Result<Connection> {
    let uri = format!("file:{}?mode=ro", path.to_string_lossy());
    Ok(Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
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
            liveness: Liveness::Unknown,
        });
    }
    Ok(out)
}

/// Threads outside the recency window. Separate from `read_threads` because
/// the two differ in more than their WHERE clause: an ended thread has no
/// state to derive, so it skips the queue lookup entirely.
fn read_threads_ended(root: &Path, now: i64) -> Result<Vec<AgentSession>> {
    let db = root.join("state_5.sqlite");
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let conn = open_ro(&db)?;

    let mut stmt = conn.prepare(
        "SELECT id, cwd, name, title, model, tokens_used, created_at_ms, updated_at_ms, archived
         FROM threads
         WHERE archived = 0 AND updated_at_ms <= ?1
         ORDER BY updated_at_ms DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map((now - RECENT_WINDOW_MS, super::max_ended() as i64), |row| {
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
            state: SessionState::Ended,
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
            liveness: Liveness::Dead,
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
    let touched = ["logs_2.sqlite-wal", "logs_2.sqlite"]
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
        liveness: Liveness::Unknown,
    })
}

pub(crate) fn modified_ms(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    /// The cap these tests assert against. Production reads
    /// `adapters::max_ended()`, which is process-global state a parallel
    /// test could change; a fixed number here keeps them independent.
    const MAX_ENDED: usize = 100;

    use super::*;
    use crate::adapters::testutil;
    use std::time::Duration;
    use tempfile::TempDir;

    /// codex keeps its threads in `state_5.sqlite` and its queue in
    /// `queue_1.sqlite`, both under a `.codex` root.
    struct Fixture {
        _home: TempDir,
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let home = TempDir::new().unwrap();
            let root = home.path().join(".codex");
            std::fs::create_dir_all(&root).unwrap();
            Self { _home: home, root }
        }

        fn paths(&self) -> PathResolver {
            PathResolver::for_home(self._home.path().to_path_buf())
        }

        /// `threads` with the columns the adapter actually reads.
        fn with_threads(&self, rows: &[(&str, i64, i64, bool)]) -> Connection {
            let db = self.root.join("state_5.sqlite");
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS threads (
                    id TEXT PRIMARY KEY,
                    cwd TEXT,
                    name TEXT,
                    title TEXT,
                    model TEXT,
                    tokens_used INTEGER,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER,
                    archived INTEGER
                )",
            )
            .unwrap();
            for (id, created, updated, archived) in rows {
                conn.execute(
                    "INSERT OR REPLACE INTO threads
                     (id, cwd, name, title, model, tokens_used, created_at_ms, updated_at_ms, archived)
                     VALUES (?1, '/repo', 'a-name', 'a-title', 'gpt-5-codex', 4200, ?2, ?3, ?4)",
                    rusqlite::params![id, created, updated, i32::from(*archived)],
                )
                .unwrap();
            }
            conn
        }

        fn with_queue(&self, thread_ids: &[&str]) {
            let db = self.root.join("queue_1.sqlite");
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch("CREATE TABLE queued_items (thread_id TEXT);")
                .unwrap();
            for id in thread_ids {
                conn.execute("INSERT INTO queued_items VALUES (?1)", [id])
                    .unwrap();
            }
        }

        /// A freshly written log database, which is how codex announces that it
        /// is running at all when `threads` is empty.
        fn with_fresh_logs(&self) -> PathBuf {
            let path = self.root.join("logs_2.sqlite");
            std::fs::write(&path, b"log").unwrap();
            path
        }
    }

    #[test]
    fn a_thread_becomes_a_usage_only_session() {
        // codex reports no state, so the tier must say so; otherwise an inferred
        // state reads as a reported one.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[("t1", now - 60_000, now - 30_000, false)]);

        let found = read_threads(&f.root, now).expect("threads");
        assert_eq!(found.len(), 1);
        let session = &found[0];
        assert_eq!(session.tier, FidelityTier::UsageOnly);
        assert_eq!(session.key.harness, HarnessId::Codex);
        assert_eq!(session.liveness, Liveness::Unknown);
        assert_eq!(session.name.as_deref(), Some("a-name"));
        assert_eq!(session.model.as_deref(), Some("gpt-5-codex"));
    }

    #[test]
    fn the_name_falls_back_to_the_thread_title() {
        let f = Fixture::new();
        let now = now_ms();
        let conn = f.with_threads(&[("t1", now - 60_000, now - 30_000, false)]);
        conn.execute("UPDATE threads SET name = NULL", []).unwrap();
        drop(conn);

        let found = read_threads(&f.root, now).expect("threads");
        assert_eq!(found[0].name.as_deref(), Some("a-title"));
    }

    #[test]
    fn token_totals_come_from_tokens_used() {
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[("t1", now - 60_000, now - 30_000, false)]);
        let found = read_threads(&f.root, now).expect("threads");
        let tokens = found[0].tokens.expect("codex reports usage");
        assert_eq!(tokens.input, 4200);
        assert_eq!(tokens.output, 0, "codex exposes one counter, not five");
    }

    #[test]
    fn a_recently_updated_thread_is_running() {
        // codex has no status field, so recency is the only signal - and a
        // thread touched seconds ago is plainly in use.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[("t1", now - 600_000, now - 5_000, false)]);
        assert_eq!(
            read_threads(&f.root, now).expect("t")[0].state,
            SessionState::Running
        );
    }

    #[test]
    fn a_thread_past_the_grace_window_is_idle() {
        // Idle, not ended: it is still inside the recency window, so it is in
        // the live list - it has simply not been touched in a while.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[("t1", now - 600_000, now - RUNNING_GRACE_MS - 1_000, false)]);
        assert_eq!(
            read_threads(&f.root, now).expect("t")[0].state,
            SessionState::Idle
        );
    }

    #[test]
    fn a_queued_prompt_makes_a_stale_thread_running_again() {
        // The closest thing codex exposes to "waiting": the user submitted work
        // that has not started yet, so the thread is not idle.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[("t1", now - 600_000, now - 3_600_000, false)]);
        f.with_queue(&["t1"]);

        let found = read_threads(&f.root, now).expect("t");
        assert_eq!(found[0].state, SessionState::Running);
    }

    #[test]
    fn a_queued_prompt_only_wakes_its_own_thread() {
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[
            ("t1", now - 600_000, now - 3_600_000, false),
            ("t2", now - 600_000, now - 3_600_000, false),
        ]);
        f.with_queue(&["t1"]);

        let found = read_threads(&f.root, now).expect("t");
        let by_id: std::collections::HashMap<_, _> = found
            .iter()
            .map(|s| (s.session_id.as_str(), s.state))
            .collect();
        assert_eq!(by_id["t1"], SessionState::Running);
        assert_eq!(by_id["t2"], SessionState::Idle);
    }

    #[test]
    fn archived_threads_are_not_shown() {
        // Archived means the user filed it away on purpose.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[
            ("visible", now - 60_000, now - 30_000, false),
            ("hidden", now - 60_000, now - 30_000, true),
        ]);
        let found = read_threads(&f.root, now).expect("t");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].session_id, "visible");
    }

    #[test]
    fn threads_outside_the_recency_window_are_history_not_live() {
        // The split that keeps a months-old row out of the differ's reach.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[
            ("recent", now - 600_000, now - 60_000, false),
            (
                "ancient",
                now - 90 * 86_400_000,
                now - 60 * 86_400_000,
                false,
            ),
        ]);

        let live = read_threads(&f.root, now).expect("t");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].session_id, "recent");

        let mut adapter = CodexAdapter::default();
        let ended = adapter.scan_ended(&f.paths()).expect("ended");
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].session_id, "ancient");
        assert_eq!(ended[0].state, SessionState::Ended);
        assert_eq!(ended[0].liveness, Liveness::Dead);
    }

    #[test]
    fn an_ended_thread_has_no_derived_state() {
        // Its last recorded state is history, not something the user is being
        // waited on for.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[(
            "ancient",
            now - 90 * 86_400_000,
            now - 60 * 86_400_000,
            false,
        )]);
        let ended = read_threads_ended(&f.root, now).expect("ended");
        assert_eq!(ended[0].state, SessionState::Ended);
    }

    #[test]
    fn history_is_newest_first_and_capped() {
        let f = Fixture::new();
        let now = now_ms();
        let rows: Vec<(String, i64, i64, bool)> = (0..(MAX_ENDED + 20))
            .map(|i| {
                (
                    format!("t{i:04}"),
                    now - 60 * 86_400_000,
                    now - 60 * 86_400_000 - i as i64 * 1_000,
                    false,
                )
            })
            .collect();
        let borrowed: Vec<(&str, i64, i64, bool)> = rows
            .iter()
            .map(|(id, c, u, a)| (id.as_str(), *c, *u, *a))
            .collect();
        f.with_threads(&borrowed);

        let ended = read_threads_ended(&f.root, now).expect("ended");
        assert_eq!(ended.len(), MAX_ENDED);
        assert_eq!(ended[0].session_id, "t0000", "newest history first");
    }

    #[test]
    fn a_missing_state_database_is_not_an_error() {
        let f = Fixture::new();
        assert!(read_threads(&f.root, now_ms()).expect("t").is_empty());
        assert!(read_threads_ended(&f.root, now_ms()).expect("t").is_empty());
    }

    #[test]
    fn a_corrupt_state_database_degrades_the_adapter_for_good() {
        // A parse failure must degrade one harness, never kill the 1.5s loop
        // and never retry a file that is not going to fix itself.
        let f = Fixture::new();
        std::fs::write(f.root.join("state_5.sqlite"), b"not a database").unwrap();
        let mut adapter = CodexAdapter::default();
        assert!(adapter.scan(&f.paths()).expect("scan").is_empty());
        assert!(
            adapter.scan(&f.paths()).expect("scan").is_empty(),
            "still quiet on the next tick"
        );
    }

    #[test]
    fn a_running_codex_with_no_threads_reports_presence_rather_than_nothing() {
        // On a current build `threads` can be empty while codex is plainly
        // running and writing to logs_2.sqlite. An empty list would tell the
        // user nothing is running.
        let f = Fixture::new();
        f.with_threads(&[]);
        f.with_fresh_logs();

        let mut adapter = CodexAdapter::default();
        let found = adapter.scan(&f.paths()).expect("scan");
        assert_eq!(found.len(), 1);
        let session = &found[0];
        assert_eq!(session.tier, FidelityTier::PresenceOnly);
        assert_eq!(session.state, SessionState::ActiveUnknown);
        assert_eq!(session.session_id, "codex-activity");
        assert_eq!(session.liveness, Liveness::Unknown);
    }

    #[test]
    fn stale_logs_mean_no_presence_row() {
        // Otherwise a codex that ran once and quit would look alive forever.
        let f = Fixture::new();
        f.with_threads(&[]);
        let logs = f.with_fresh_logs();
        testutil::backdate(
            &logs,
            Duration::from_millis(PRESENCE_WINDOW_MS as u64 + 60_000),
        );

        let mut adapter = CodexAdapter::default();
        assert!(adapter.scan(&f.paths()).expect("scan").is_empty());
    }

    #[test]
    fn a_wal_that_outlives_the_database_still_counts_as_activity() {
        // SQLite in WAL mode writes to `-wal` first, so reading only the
        // database file would miss the most recent writes entirely.
        let f = Fixture::new();
        f.with_threads(&[]);
        f.with_fresh_logs();
        let wal = f.root.join("logs_2.sqlite-wal");
        std::fs::write(&wal, b"wal").unwrap();
        testutil::backdate(&f.root.join("logs_2.sqlite"), Duration::from_secs(3_600));

        let now = now_ms();
        assert!(presence_row(&f.root, now).is_some());
    }

    #[test]
    fn real_threads_win_over_the_presence_row() {
        // Reporting "codex-activity" alongside real threads would double-count.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[("t1", now - 60_000, now - 30_000, false)]);
        f.with_fresh_logs();
        let mut adapter = CodexAdapter::default();
        let found = adapter.scan(&f.paths()).expect("scan");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].session_id, "t1");
    }

    #[test]
    fn no_logs_and_no_threads_means_nothing_at_all() {
        // Absent is different from "installed and quiet", and neither is a
        // reason to invent a row.
        let f = Fixture::new();
        f.with_threads(&[]);
        assert!(presence_row(&f.root, now_ms()).is_none());
    }

    #[test]
    fn the_thread_key_is_stable_across_scans() {
        // There is no process behind a database row, so the id has to stand in
        // for one - and it has to be stable, or the UI would show duplicates.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[("t1", now - 60_000, now - 30_000, false)]);
        let a = read_threads(&f.root, now).expect("t");
        let b = read_threads(&f.root, now).expect("t");
        assert_eq!(a[0].key, b[0].key);
        assert_eq!(a[0].key.proc_start, a[0].started_at.to_string());
    }

    #[test]
    fn the_thread_key_does_not_collide_when_two_rows_share_an_id() {
        // The key is the id plus the creation time, so a re-created thread with
        // the same id is a different row.
        let f = Fixture::new();
        let now = now_ms();
        f.with_threads(&[("same", now - 600_000, now - 30_000, false)]);
        let first = read_threads(&f.root, now).expect("t");
        f.with_threads(&[("same", now - 300_000, now - 20_000, false)]);
        let second = read_threads(&f.root, now).expect("t");
        assert_ne!(first[0].key, second[0].key);
    }

    #[test]
    fn detection_follows_the_codex_root() {
        // `for_home` is no use here: on Linux `codex_root` falls through to the
        // real /mnt/c/Users, which on a WSL box has a .codex in it. Pinning both
        // roots is what makes "not installed" assertable.
        let home = TempDir::new().unwrap();
        let mut adapter = CodexAdapter::default();
        let without = PathResolver::with_windows_home(home.path().to_path_buf(), None);
        assert!(!adapter.detect(&without));
        assert!(adapter.scan(&without).expect("scan").is_empty());

        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        let with = PathResolver::with_windows_home(home.path().to_path_buf(), None);
        assert!(
            adapter.detect(&with),
            "installed and quiet is still installed"
        );
    }
}
