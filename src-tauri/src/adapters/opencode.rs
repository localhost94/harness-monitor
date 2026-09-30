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
use crate::liveness::Liveness;
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
        match self.query(&paths.opencode_db(), Mode::Live) {
            Ok(sessions) => Ok(sessions),
            Err(err) => {
                self.degraded = true;
                tracing::warn!(%err, "opencode adapter degraded; will stay quiet until restart");
                Ok(Vec::new())
            }
        }
    }

    /// Everything older than the live window, newest first.
    ///
    /// opencode has no process per session - the row *is* the session - so
    /// "ended" here means precisely what it means in `scan`: outside the
    /// recency window. There is no pid to check, and no claim is made about
    /// whether one is somehow still running a month later.
    fn scan_ended(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        if self.degraded {
            return Ok(Vec::new());
        }
        match self.query(&paths.opencode_db(), Mode::Ended) {
            Ok(sessions) => Ok(sessions),
            Err(err) => {
                tracing::warn!(%err, "opencode ended-session query failed");
                Ok(Vec::new())
            }
        }
    }
}

enum Mode {
    Live,
    Ended,
}

impl OpenCodeAdapter {
    fn query(&self, db: &Path, mode: Mode) -> Result<Vec<AgentSession>> {
        let conn = open_read_only(db)?;
        let now = crate::model::now_ms();
        let cutoff = now - RECENT_WINDOW_MS;

        // Both modes want the newest rows and the same columns; they differ in
        // which side of the recency window they take and how many. The index
        // does not cover time_updated, so this is a scan either way - which is
        // why the ended list is capped and never runs on the live path.
        let sql = match mode {
            Mode::Live => (
                "SELECT id, directory, title, agent, model, cost, tokens_input, tokens_output,
                        tokens_reasoning, tokens_cache_read, tokens_cache_write,
                        time_created, time_updated
                 FROM session
                 WHERE time_updated > ?1
                 ORDER BY time_updated DESC
                 LIMIT ?2",
                cutoff,
                MAX_SESSIONS as i64,
            ),
            Mode::Ended => (
                "SELECT id, directory, title, agent, model, cost, tokens_input, tokens_output,
                        tokens_reasoning, tokens_cache_read, tokens_cache_write,
                        time_created, time_updated
                 FROM session
                 WHERE time_updated <= ?1
                 ORDER BY time_updated DESC
                 LIMIT ?2",
                cutoff,
                super::max_ended() as i64,
            ),
        };
        let mut stmt = conn.prepare(sql.0)?;
        let rows = stmt.query_map((sql.1, sql.2), |row| {
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

        let live = matches!(mode, Mode::Live);
        let mut out = Vec::new();
        for row in rows {
            let row = row?;
            // The message lookup is per-row and costs a second query each. An
            // ended session is hours old by construction, so derive_state would
            // return Idle without ever reading the message - skipping it is not
            // an approximation, it is the same answer for a fifth of the work.
            let last = match live {
                true => latest_message(&conn, &row.id).unwrap_or(None),
                false => None,
            };
            let state = if live {
                derive_state(last.as_ref(), row.time_updated, now)
            } else {
                SessionState::Ended
            };
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
                liveness: if live {
                    Liveness::Unknown
                } else {
                    Liveness::Dead
                },
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
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
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
    let raw: Option<String> = stmt.query_row((session_id,), |row| row.get(0)).ok();
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
            match (
                msg.finish.as_deref(),
                msg.time.as_ref().and_then(|t| t.completed),
            ) {
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
            derive_state(
                Some(&msg("assistant", Some("tool-calls"), None)),
                stale,
                now
            ),
            SessionState::Idle
        );
    }

    #[test]
    fn finished_turn_is_idle_and_streaming_is_running() {
        let now = 1_000_000;
        let fresh = now - 1_000;
        assert_eq!(
            derive_state(
                Some(&msg("assistant", Some("stop"), Some(999_000))),
                fresh,
                now
            ),
            SessionState::Idle
        );
        assert_eq!(
            derive_state(Some(&msg("assistant", None, None)), fresh, now),
            SessionState::Running
        );
        assert_eq!(
            derive_state(
                Some(&msg("assistant", Some("tool-calls"), Some(1))),
                fresh,
                now
            ),
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
        assert_eq!(
            derive_state(None, now - 1_000, now),
            SessionState::ActiveUnknown
        );
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

#[cfg(test)]
mod db {
    use super::*;
    use crate::model::{now_ms, FidelityTier};
    use tempfile::TempDir;

    /// The cap this test asserts against. Production reads
    /// `adapters::max_ended()`, which is process-global state a parallel test
    /// could change; a fixed number keeps the assertion about the SQL `LIMIT`
    /// rather than about whatever the cap currently is.
    const MAX_ENDED: usize = 100;

    /// opencode's `session` table is denormalised, so a fixture is a handful of
    /// columns - which is also why the adapter needs no JSON scan for usage.
    struct Fixture {
        _home: TempDir,
        db: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let home = TempDir::new().unwrap();
            let dir = home.path().join(".local/share/opencode");
            std::fs::create_dir_all(&dir).unwrap();
            let db = dir.join("opencode.db");
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY,
                    directory TEXT,
                    title TEXT,
                    agent TEXT,
                    model TEXT,
                    cost REAL,
                    tokens_input INTEGER,
                    tokens_output INTEGER,
                    tokens_reasoning INTEGER,
                    tokens_cache_read INTEGER,
                    tokens_cache_write INTEGER,
                    time_created INTEGER,
                    time_updated INTEGER
                );
                 CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT);",
            )
            .unwrap();
            Self { _home: home, db }
        }

        fn paths(&self) -> PathResolver {
            PathResolver::for_home(self._home.path().to_path_buf())
        }

        #[allow(clippy::too_many_arguments)]
        fn row(
            &self,
            id: &str,
            created: i64,
            updated: i64,
            cost: Option<f64>,
            model: Option<&str>,
        ) {
            let conn = Connection::open(&self.db).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO session
                 (id, directory, title, agent, model, cost, tokens_input, tokens_output,
                  tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
                 VALUES (?1, '/repo', 'a-title', 'build', ?2, ?3, 100, 200, 7, 30, 40, ?4, ?5)",
                rusqlite::params![id, model, cost, created, updated],
            )
            .unwrap();
        }

        /// `message.id` is a time-sortable ULID, so the newest row is simply the
        /// highest id.
        fn message(&self, session_id: &str, id: &str, data: &str) {
            let conn = Connection::open(&self.db).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, session_id, data],
            )
            .unwrap();
        }
    }

    fn ago(ms: i64) -> i64 {
        now_ms() - ms
    }

    #[test]
    fn a_row_becomes_a_usage_only_session() {
        // opencode records no status, so the tier must say so; an inferred state
        // read as a reported one is exactly what the label exists to prevent.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), Some(1.25), None);
        f.message("s1", "m1", r#"{"role":"user"}"#);

        let mut adapter = OpenCodeAdapter::default();
        let found = adapter.scan(&f.paths()).expect("scan");
        assert_eq!(found.len(), 1);
        let session = &found[0];
        assert_eq!(session.tier, FidelityTier::UsageOnly);
        assert_eq!(session.liveness, Liveness::Unknown);
        assert_eq!(session.cwd, PathBuf::from("/repo"));
        assert_eq!(session.name.as_deref(), Some("a-title"));
        assert_eq!(session.session_id, "s1");
    }

    #[test]
    fn all_five_token_counters_and_the_cost_are_read_from_the_row() {
        // Denormalised on purpose: usage must not need a scan of the 4 GB of
        // message/part blobs.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), Some(1.25), None);
        f.message("s1", "m1", r#"{"role":"user"}"#);

        let mut adapter = OpenCodeAdapter::default();
        let session = adapter.scan(&f.paths()).expect("scan").remove(0);
        let tokens = session.tokens.expect("opencode always reports tokens");
        assert_eq!(tokens.input, 100);
        assert_eq!(tokens.output, 200);
        assert_eq!(tokens.reasoning, 7);
        assert_eq!(tokens.cache_read, 30);
        assert_eq!(tokens.cache_write, 40);
        assert_eq!(session.cost, Some(1.25));
    }

    #[test]
    fn null_counters_read_as_zero_rather_than_failing_the_row() {
        let f = Fixture::new();
        let conn = Connection::open(&f.db).unwrap();
        conn.execute(
            "INSERT INTO session (id, directory, title, time_created, time_updated)
             VALUES ('s1', '/repo', 't', ?1, ?2)",
            rusqlite::params![ago(600_000), ago(1_000)],
        )
        .unwrap();

        let mut adapter = OpenCodeAdapter::default();
        let session = adapter.scan(&f.paths()).expect("scan").remove(0);
        assert_eq!(session.tokens.unwrap().input, 0);
        assert_eq!(session.cost, None);
    }

    #[test]
    fn the_model_is_unwrapped_from_its_json_blob() {
        let f = Fixture::new();
        f.row(
            "s1",
            ago(600_000),
            ago(1_000),
            None,
            Some(r#"{"id":"omen-alpha","providerID":"acme"}"#),
        );
        f.message("s1", "m1", r#"{"role":"user"}"#);

        let mut adapter = OpenCodeAdapter::default();
        let session = adapter.scan(&f.paths()).expect("scan").remove(0);
        assert_eq!(session.model.as_deref(), Some("omen-alpha"));
    }

    #[test]
    fn a_model_that_is_not_json_is_used_verbatim() {
        // Older rows, and a harness that wrote a bare name.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, Some("gpt-5-codex"));
        f.message("s1", "m1", r#"{"role":"user"}"#);
        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(
            adapter.scan(&f.paths()).expect("scan")[0].model.as_deref(),
            Some("gpt-5-codex")
        );
    }

    #[test]
    fn the_agent_is_the_model_of_last_resort() {
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, None);
        f.message("s1", "m1", r#"{"role":"user"}"#);
        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(
            adapter.scan(&f.paths()).expect("scan")[0].model.as_deref(),
            Some("build")
        );
    }

    #[test]
    fn an_empty_title_is_no_title() {
        // "" would render as a blank row the user cannot tell from a gap.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, None);
        let conn = Connection::open(&f.db).unwrap();
        conn.execute("UPDATE session SET title = '' WHERE id = 's1'", [])
            .unwrap();
        f.message("s1", "m1", r#"{"role":"user"}"#);

        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(adapter.scan(&f.paths()).expect("scan")[0].name, None);
    }

    #[test]
    fn a_user_message_with_no_reply_yet_is_running() {
        // opencode is mid-turn: the prompt landed, the answer has not.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, None);
        f.message("s1", "m1", r#"{"role":"user"}"#);

        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(
            adapter.scan(&f.paths()).expect("scan")[0].state,
            SessionState::Running
        );
    }

    #[test]
    fn a_session_with_no_messages_is_active_unknown() {
        // Neither "running" nor "idle" is knowable, and guessing idle would fire
        // a false "done" toast.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, None);
        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(
            adapter.scan(&f.paths()).expect("scan")[0].state,
            SessionState::ActiveUnknown
        );
    }

    #[test]
    fn a_malformed_message_blob_degrades_to_active_unknown() {
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, None);
        f.message("s1", "m1", "{ not json");
        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(
            adapter.scan(&f.paths()).expect("scan")[0].state,
            SessionState::ActiveUnknown
        );
    }

    #[test]
    fn the_newest_message_wins() {
        // `message.id` is a time-sortable ULID, so a higher id is a later turn.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, None);
        f.message(
            "s1",
            "01OLD",
            r#"{"role":"assistant","finish":"stop","time":{"completed":1}}"#,
        );
        f.message("s1", "02NEW", r#"{"role":"user"}"#);

        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(
            adapter.scan(&f.paths()).expect("scan")[0].state,
            SessionState::Running
        );
    }

    #[test]
    fn messages_are_scoped_to_their_own_session() {
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, None);
        f.row("s2", ago(600_000), ago(1_000), None, None);
        f.message("s1", "01", r#"{"role":"user"}"#);
        f.message(
            "s2",
            "02",
            r#"{"role":"assistant","finish":"stop","time":{"completed":1}}"#,
        );

        let mut adapter = OpenCodeAdapter::default();
        let mut found = adapter.scan(&f.paths()).expect("scan");
        found.sort_by_key(|s| s.session_id.clone());
        assert_eq!(
            found[0].state,
            SessionState::Running,
            "s1 has only a user turn"
        );
        assert_eq!(found[1].state, SessionState::Idle, "s2's turn is complete");
    }

    #[test]
    fn a_session_quiet_past_the_grace_window_is_idle() {
        // It may have been killed mid-turn, so the last message's `stop` cannot
        // be trusted to mean it is still working.
        let f = Fixture::new();
        f.row(
            "s1",
            ago(600_000),
            ago(RUNNING_GRACE_MS + 1_000),
            None,
            None,
        );
        f.message("s1", "m1", r#"{"role":"user"}"#);

        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(
            adapter.scan(&f.paths()).expect("scan")[0].state,
            SessionState::Idle
        );
    }

    #[test]
    fn the_two_lists_split_on_the_recency_window() {
        let f = Fixture::new();
        f.row("recent", ago(600_000), ago(60_000), None, None);
        f.row(
            "ancient",
            ago(90 * 86_400_000),
            ago(60 * 86_400_000),
            None,
            None,
        );
        f.message("recent", "m1", r#"{"role":"user"}"#);

        let mut adapter = OpenCodeAdapter::default();
        let live = adapter.scan(&f.paths()).expect("live");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].session_id, "recent");
        assert_eq!(live[0].liveness, Liveness::Unknown);

        let ended = adapter.scan_ended(&f.paths()).expect("ended");
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].session_id, "ancient");
        assert_eq!(ended[0].state, SessionState::Ended);
        assert_eq!(ended[0].liveness, Liveness::Dead);
    }

    #[test]
    fn an_ended_row_skips_the_message_lookup_entirely() {
        // `derive_state` would return Idle for an hours-old row anyway, so the
        // second query per row is a fifth of the work for the same answer.
        let f = Fixture::new();
        f.row(
            "ancient",
            ago(90 * 86_400_000),
            ago(60 * 86_400_000),
            None,
            None,
        );
        // A message that would say "stop" if it were consulted.
        f.message(
            "ancient",
            "01",
            r#"{"role":"assistant","finish":"stop","time":{"completed":1}}"#,
        );

        let mut adapter = OpenCodeAdapter::default();
        let ended = adapter.scan_ended(&f.paths()).expect("ended");
        assert_eq!(ended[0].state, SessionState::Ended);
    }

    #[test]
    fn the_live_list_is_capped_and_the_history_list_more_so() {
        let f = Fixture::new();
        for i in 0..(MAX_SESSIONS + 20) {
            f.row(
                &format!("live{i:04}"),
                ago(600_000),
                ago(60_000 + i as i64 * 1_000),
                None,
                None,
            );
        }
        for i in 0..(MAX_ENDED + 20) {
            f.row(
                &format!("old{i:04}"),
                ago(90 * 86_400_000),
                ago(60 * 86_400_000) - i as i64 * 1_000,
                None,
                None,
            );
        }
        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(adapter.scan(&f.paths()).expect("live").len(), MAX_SESSIONS);
        assert_eq!(
            adapter.scan_ended(&f.paths()).expect("ended").len(),
            MAX_ENDED
        );
    }

    #[test]
    fn both_lists_come_back_newest_first() {
        let f = Fixture::new();
        f.row("older", ago(600_000), ago(300_000), None, None);
        f.row("newer", ago(600_000), ago(10_000), None, None);

        let mut adapter = OpenCodeAdapter::default();
        let live = adapter.scan(&f.paths()).expect("live");
        assert_eq!(live[0].session_id, "newer");
    }

    #[test]
    fn the_key_is_a_stable_hash_of_the_row_id() {
        // There is no process behind a database row, so the id stands in for
        // one - and it has to be stable or the UI would show duplicates.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), None, None);
        f.message("s1", "m1", r#"{"role":"user"}"#);
        let mut adapter = OpenCodeAdapter::default();
        let first = adapter.scan(&f.paths()).expect("scan");
        let second = adapter.scan(&f.paths()).expect("scan");
        assert_eq!(first[0].key, second[0].key);
        assert_eq!(first[0].key.pid, stable_id("s1"));
        assert_eq!(first[0].key.proc_start, first[0].started_at.to_string());
    }

    #[test]
    fn two_different_rows_get_two_different_keys() {
        let f = Fixture::new();
        f.row("a", ago(600_000), ago(1_000), None, None);
        f.row("b", ago(600_000), ago(1_000), None, None);
        let mut adapter = OpenCodeAdapter::default();
        let found = adapter.scan(&f.paths()).expect("scan");
        assert_eq!(found.len(), 2);
        assert_ne!(found[0].key, found[1].key);
    }

    #[test]
    fn stable_id_is_never_negative() {
        // The key's pid is an i64 that reaches the UI as a number, and a
        // negative one would be indistinguishable from a real pid.
        for id in [
            "a",
            "b",
            "ses_01H8XYZ",
            "",
            "a-very-long-session-identifier",
        ] {
            assert!(stable_id(id) >= 0, "{id} produced {}", stable_id(id));
        }
        assert_eq!(stable_id("same"), stable_id("same"));
        assert_ne!(stable_id("a"), stable_id("b"));
    }

    #[test]
    fn a_missing_database_is_not_detected_and_does_not_error() {
        // Absent is different from "installed and quiet", and neither is a
        // reason to fail the tick.
        let home = TempDir::new().unwrap();
        let paths = PathResolver::for_home(home.path().to_path_buf());
        let mut adapter = OpenCodeAdapter::default();
        assert!(!adapter.detect(&paths));
        assert!(adapter.scan(&paths).expect("scan").is_empty());
        assert!(adapter.scan_ended(&paths).expect("ended").is_empty());
    }

    #[test]
    fn detection_follows_the_database_file() {
        let f = Fixture::new();
        let adapter = OpenCodeAdapter::default();
        assert!(adapter.detect(&f.paths()));
    }

    #[test]
    fn a_corrupt_database_degrades_this_harness_for_the_rest_of_the_run() {
        // One unreadable schema must not spam a warning every 1.5s, and must
        // not take the other four harnesses down with it.
        let f = Fixture::new();
        std::fs::remove_file(&f.db).unwrap();
        std::fs::write(&f.db, b"not a database").unwrap();
        let mut adapter = OpenCodeAdapter::default();
        assert!(adapter.scan(&f.paths()).expect("scan").is_empty());
        assert!(adapter.scan(&f.paths()).expect("scan").is_empty());
        assert!(adapter.scan_ended(&f.paths()).expect("ended").is_empty());
    }

    #[test]
    fn a_schema_that_lost_a_column_degrades_rather_than_panicking() {
        // An upgraded opencode is the realistic version of this.
        let f = Fixture::new();
        let conn = Connection::open(&f.db).unwrap();
        conn.execute_batch("DROP TABLE session; CREATE TABLE session (id TEXT);")
            .unwrap();
        let mut adapter = OpenCodeAdapter::default();
        assert!(adapter.scan(&f.paths()).expect("scan").is_empty());
    }

    #[test]
    fn a_wal_database_is_readable() {
        // opencode runs in WAL mode, and the whole Windows design exists
        // because opening a WAL database over a 9p share needs shared memory the
        // filesystem cannot provide. Read it the way the adapter does.
        let f = Fixture::new();
        f.row("s1", ago(600_000), ago(1_000), Some(0.5), None);
        f.message("s1", "m1", r#"{"role":"user"}"#);
        let conn = Connection::open(&f.db).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        drop(conn);
        assert!(f.db.with_extension("db-wal").exists() || f.db.exists());

        let mut adapter = OpenCodeAdapter::default();
        assert_eq!(adapter.scan(&f.paths()).expect("scan").len(), 1);
    }
}
