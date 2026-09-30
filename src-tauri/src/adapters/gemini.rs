//! gemini-cli adapter.
//!
//! Source: ~/.gemini/tmp/<project-hash>/chats/session-*.json. Each file is a
//! whole conversation with per-message `tokens{input,output,cached,thoughts,
//! tool,total}`, so usage is exact. There is no status field anywhere, so
//! state is recency only and the session is labelled presence-only - it never
//! claims a turn finished, because it cannot know.

use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

use super::HarnessAdapter;
use crate::liveness::Liveness;
use crate::model::{
    now_ms, AgentSession, FidelityTier, HarnessId, SessionKey, SessionState, TokenCounts,
};
use crate::paths::PathResolver;

const RECENT_WINDOW_MS: i64 = 12 * 60 * 60 * 1000;
const ACTIVE_GRACE_MS: i64 = 120 * 1000;

#[derive(Default)]
pub struct GeminiAdapter;

#[derive(Debug, Deserialize)]
struct Chat {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    last_updated: Option<String>,
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tokens: Option<GeminiTokens>,
}

#[derive(Debug, Deserialize, Default)]
struct GeminiTokens {
    #[serde(default)]
    input: i64,
    #[serde(default)]
    output: i64,
    #[serde(default)]
    cached: i64,
    #[serde(default)]
    thoughts: i64,
}

impl HarnessAdapter for GeminiAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Gemini
    }

    fn detect(&self, paths: &PathResolver) -> bool {
        paths.gemini_root().join("tmp").is_dir()
    }

    fn scan(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        let tmp = paths.gemini_root().join("tmp");
        let now = now_ms();
        let mut out = Vec::new();

        let Ok(projects) = std::fs::read_dir(&tmp) else {
            return Ok(out);
        };
        for project in projects.flatten() {
            let chats = project.path().join("chats");
            let Ok(files) = std::fs::read_dir(&chats) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                // mtime is the cheap gate: skip parsing conversations that
                // cannot possibly be recent.
                let Some(touched) = super::codex::modified_ms(&path) else {
                    continue;
                };
                if now - touched > RECENT_WINDOW_MS {
                    continue;
                }
                if let Some(session) = parse_chat(&path, touched, now) {
                    out.push(session);
                }
            }
        }
        Ok(out)
    }

    /// Conversations older than the recency window, newest first.
    fn scan_ended(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        let tmp = paths.gemini_root().join("tmp");
        let now = now_ms();
        let mut out: Vec<AgentSession> = Vec::new();

        let Ok(projects) = std::fs::read_dir(&tmp) else {
            return Ok(out);
        };
        for project in projects.flatten() {
            let chats = project.path().join("chats");
            let Ok(files) = std::fs::read_dir(&chats) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Some(touched) = super::codex::modified_ms(&path) else {
                    continue;
                };
                if now - touched <= RECENT_WINDOW_MS {
                    continue;
                }
                if out.len() >= super::max_ended() {
                    break;
                }
                let Some(mut session) = parse_chat(&path, touched, now) else {
                    continue;
                };
                session.state = SessionState::Ended;
                session.liveness = Liveness::Dead;
                out.push(session);
            }
        }
        out.sort_by(|a, b| b.state_changed_at.cmp(&a.state_changed_at));
        out.truncate(super::max_ended());
        Ok(out)
    }
}

fn parse_chat(path: &PathBuf, touched: i64, now: i64) -> Option<AgentSession> {
    let raw = std::fs::read_to_string(path).ok()?;
    let chat: Chat = serde_json::from_str(&raw).ok()?;

    let mut tokens = TokenCounts::default();
    let mut model = None;
    for message in &chat.messages {
        if let Some(t) = &message.tokens {
            tokens.input += t.input;
            tokens.output += t.output;
            tokens.cache_read += t.cached;
            tokens.reasoning += t.thoughts;
        }
        if message.model.is_some() {
            model = message.model.clone();
        }
    }

    let changed_at = chat
        .last_updated
        .as_deref()
        .and_then(crate::quota::parse_iso)
        .unwrap_or(touched);
    let started_at = chat
        .start_time
        .as_deref()
        .and_then(crate::quota::parse_iso)
        .unwrap_or(changed_at);

    Some(AgentSession {
        key: SessionKey {
            harness: HarnessId::Gemini,
            pid_domain: "gemini:file".into(),
            pid: super::opencode::stable_id(&path.to_string_lossy()),
            proc_start: started_at.to_string(),
        },
        session_id: chat
            .session_id
            .unwrap_or_else(|| path.to_string_lossy().to_string()),
        cwd: path.parent()?.parent()?.to_path_buf(),
        name: path.file_stem().map(|s| s.to_string_lossy().to_string()),
        // Recency, and nothing more. gemini-cli records no state.
        state: if now - changed_at <= ACTIVE_GRACE_MS {
            SessionState::ActiveUnknown
        } else {
            SessionState::Idle
        },
        state_changed_at: changed_at,
        started_at,
        waiting_for: None,
        model,
        tokens: Some(tokens),
        cost: None,
        is_background: false,
        tier: FidelityTier::PresenceOnly,
        jump_target: None,
        terminal_title: None,
        liveness: Liveness::Unknown,
    })
}

#[cfg(test)]
mod tests {
    /// The cap these tests assert against. Production reads
    /// `adapters::max_ended()`, which is process-global state a parallel
    /// test could change; a fixed number here keeps them independent.
    const MAX_ENDED: usize = 100;

    use super::*;
    use crate::model::FidelityTier;
    use std::time::Duration;
    use tempfile::TempDir;

    /// gemini writes `~/.gemini/tmp/<project-hash>/chats/session-<id>.json`.
    fn fixture(tmp: &TempDir, project: &str, file: &str, body: &str) -> PathBuf {
        let dir = tmp.path().join(".gemini/tmp").join(project).join("chats");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(file);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn resolver(tmp: &TempDir) -> PathResolver {
        PathResolver::for_home(tmp.path().to_path_buf())
    }

    /// The keys are snake_case, which is what the `Chat` struct asks for.
    const CHAT: &str = r#"{
        "session_id":"abc-123",
        "start_time":"2026-09-06T04:00:00.000Z",
        "last_updated":"2026-09-06T05:30:00.000Z",
        "messages":[
            {"model":"gemini-2.5-pro","tokens":{"input":100,"output":200,"cached":30,"thoughts":7}},
            {"tokens":{"input":50,"output":60,"cached":0,"thoughts":0}}
        ]
    }"#;

    fn parse_now(path: &PathBuf) -> Option<AgentSession> {
        let now = now_ms();
        parse_chat(path, now, now)
    }

    #[test]
    fn a_chat_becomes_a_presence_only_session() {
        // gemini-cli records no status at all, so the tier must say so or an
        // inferred state would be read as a reported one.
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "p1", "session-abc-123.json", CHAT);
        let session = parse_now(&path).expect("a session");

        assert_eq!(session.tier, FidelityTier::PresenceOnly);
        assert_eq!(session.key.harness, HarnessId::Gemini);
        assert_eq!(session.session_id, "abc-123");
        assert_eq!(session.liveness, Liveness::Unknown);
        assert_eq!(session.cost, None, "gemini reports no cost");
    }

    #[test]
    fn token_counts_are_summed_across_messages() {
        // Per-message counts only; the totals have to be folded, or a long
        // conversation would forever report its first turn.
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "p1", "s.json", CHAT);
        let tokens = parse_now(&path).expect("a session").tokens.unwrap();
        assert_eq!(tokens.input, 150);
        assert_eq!(tokens.output, 260);
        assert_eq!(tokens.cache_read, 30);
        assert_eq!(tokens.reasoning, 7);
    }

    #[test]
    fn a_message_with_no_token_block_does_not_break_the_sum() {
        let tmp = TempDir::new().unwrap();
        let path = fixture(
            &tmp,
            "p1",
            "s.json",
            r#"{"session_id":"a","messages":[
                {"model":"gemini-2.5-pro","tokens":{"input":10,"output":20}},
                {"model":"gemini-2.5-pro"}]}"#,
        );
        let tokens = parse_now(&path).expect("a session").tokens.unwrap();
        assert_eq!(tokens.input, 10);
        assert_eq!(tokens.output, 20);
    }

    #[test]
    fn the_last_model_seen_is_the_one_reported() {
        let tmp = TempDir::new().unwrap();
        let path = fixture(
            &tmp,
            "p1",
            "s.json",
            r#"{"session_id":"a","messages":[
                {"model":"gemini-2.0-flash"},{"model":"gemini-2.5-pro"}]}"#,
        );
        assert_eq!(
            parse_now(&path).expect("a session").model.as_deref(),
            Some("gemini-2.5-pro")
        );
    }

    #[test]
    fn recency_alone_decides_active_or_idle() {
        // There is no status to read, so the only honest signal is when the
        // conversation was last written. `touched` is the mtime; the chat's own
        // `last_updated` is what the state is judged on, so the two are aligned
        // here and only the `now` under test moves.
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "p1", "s.json", CHAT);
        let changed = crate::quota::parse_iso("2026-09-06T05:30:00.000Z").unwrap();

        let fresh = parse_chat(&path, changed, changed + ACTIVE_GRACE_MS).expect("a session");
        assert_eq!(fresh.state, SessionState::ActiveUnknown);

        let stale = parse_chat(&path, changed, changed + ACTIVE_GRACE_MS + 1).expect("a session");
        assert_eq!(stale.state, SessionState::Idle);
    }

    #[test]
    fn timestamps_come_from_the_chat() {
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "p1", "s.json", CHAT);
        let session = parse_chat(&path, 12_345, now_ms()).expect("a session");
        assert_eq!(
            session.state_changed_at,
            crate::quota::parse_iso("2026-09-06T05:30:00.000Z").unwrap()
        );
        assert_eq!(
            session.started_at,
            crate::quota::parse_iso("2026-09-06T04:00:00.000Z").unwrap()
        );
    }

    #[test]
    fn a_chat_with_no_timestamps_falls_back_to_the_file_mtime() {
        // A zero would render as 1970, which is worse than "whenever the file
        // was last touched".
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "p1", "bare.json", r#"{"messages":[]}"#);
        let session = parse_now(&path).expect("a session");
        assert!(session.state_changed_at > 1_600_000_000_000);
        assert_eq!(session.state_changed_at, session.started_at);
    }

    #[test]
    fn an_unparseable_timestamp_falls_back_rather_than_producing_zero() {
        let tmp = TempDir::new().unwrap();
        let path = fixture(
            &tmp,
            "p1",
            "s.json",
            r#"{"session_id":"a","last_updated":"not a date","messages":[]}"#,
        );
        let session = parse_chat(&path, 12_345, now_ms()).expect("a session");
        assert_eq!(session.state_changed_at, 12_345);
    }

    #[test]
    fn a_chat_with_no_id_falls_back_to_its_path() {
        // An id is the only handle a user has on a row, so a missing one must
        // not produce a blank one.
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "p1", "session-xyz.json", r#"{"messages":[]}"#);
        let session = parse_now(&path).expect("a session");
        assert!(
            session.session_id.contains("session-xyz.json"),
            "got {}",
            session.session_id
        );
    }

    #[test]
    fn the_cwd_is_the_project_the_chat_belongs_to() {
        // Not the chats dir and not the tmp dir: the UI prints this path, and
        // `.../tmp/1a2b3c` is not a project anybody recognises.
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "proj-hash", "s.json", CHAT);
        let session = parse_now(&path).expect("a session");
        assert_eq!(session.cwd, tmp.path().join(".gemini/tmp/proj-hash"));
    }

    #[test]
    fn the_key_is_stable_across_scans() {
        // The key is the process, and here there is no process: the file's path
        // stands in. It still has to be stable, or the UI would show duplicates.
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "p1", "s.json", CHAT);
        let a = parse_now(&path).expect("a session");
        let b = parse_now(&path).expect("a session");
        assert_eq!(a.key, b.key);
    }

    #[test]
    fn an_unparseable_chat_is_skipped_rather_than_faked() {
        let tmp = TempDir::new().unwrap();
        let path = fixture(&tmp, "p1", "s.json", "{ not json");
        assert_eq!(parse_now(&path), None);
    }

    #[test]
    fn a_missing_file_is_skipped() {
        assert_eq!(parse_chat(&PathBuf::from("/nope/never.json"), 0, 0), None);
    }

    /// The live/history split is made on the *file's* mtime, not on any
    /// timestamp inside the conversation: gemini never rewrites an old chat, so
    /// mtime is the only honest "last touched" signal it has.
    #[test]
    fn the_recency_window_splits_live_from_history() {
        let tmp = TempDir::new().unwrap();
        fixture(&tmp, "p1", "recent.json", CHAT);
        let old = fixture(&tmp, "p1", "old.json", CHAT);
        super::super::testutil::backdate(
            &old,
            Duration::from_millis(RECENT_WINDOW_MS as u64 + 60_000),
        );

        let paths = resolver(&tmp);
        let mut adapter = GeminiAdapter;

        let live = adapter.scan(&paths).expect("live scan");
        assert_eq!(live.len(), 1, "only the freshly written chat is live");
        assert_eq!(live[0].session_id, "abc-123");

        let ended = adapter.scan_ended(&paths).expect("ended scan");
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].state, SessionState::Ended);
        assert_eq!(ended[0].liveness, Liveness::Dead);
    }

    #[test]
    fn the_two_lists_never_claim_the_same_conversation() {
        // Structural, not incidental: a row in both lists is the one thing the
        // differ cannot defend against, because it only reads one of them.
        let tmp = TempDir::new().unwrap();
        let old = fixture(
            &tmp,
            "p1",
            "old.json",
            r#"{"session_id":"old","messages":[]}"#,
        );
        fixture(
            &tmp,
            "p1",
            "fresh.json",
            r#"{"session_id":"fresh","messages":[]}"#,
        );
        super::super::testutil::backdate(&old, Duration::from_millis(RECENT_WINDOW_MS as u64 + 1));
        let paths = resolver(&tmp);
        let mut adapter = GeminiAdapter;
        let live = adapter.scan(&paths).expect("live");
        let ended = adapter.scan_ended(&paths).expect("ended");
        let live_paths: Vec<_> = live.iter().map(|s| &s.session_id).collect();
        for session in &ended {
            assert!(
                !live_paths.contains(&&session.session_id),
                "{}",
                session.session_id
            );
        }
    }

    #[test]
    fn a_project_with_no_chats_directory_is_skipped() {
        // gemini creates `tmp/<hash>/` lazily, so a half-made tree is normal.
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".gemini/tmp/half-made")).unwrap();
        let mut adapter = GeminiAdapter;
        assert!(adapter.scan(&resolver(&tmp)).expect("scan").is_empty());
    }

    #[test]
    fn a_missing_gemini_root_is_not_an_error() {
        let tmp = TempDir::new().unwrap();
        let mut adapter = GeminiAdapter;
        assert!(adapter.scan(&resolver(&tmp)).expect("scan").is_empty());
        assert!(adapter
            .scan_ended(&resolver(&tmp))
            .expect("scan")
            .is_empty());
    }

    #[test]
    fn detection_follows_the_tmp_directory_not_the_root() {
        // `~/.gemini` exists for settings long before any chat is written, and
        // "installed and quiet" must not be reported as "not installed".
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".gemini")).unwrap();
        assert!(!GeminiAdapter.detect(&resolver(&tmp)));

        std::fs::create_dir_all(tmp.path().join(".gemini/tmp")).unwrap();
        assert!(GeminiAdapter.detect(&resolver(&tmp)));
    }

    #[test]
    fn only_json_files_are_parsed() {
        // The chats dir also holds editor backups and scratch notes.
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".gemini/tmp/p1/chats");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["notes.md", "s.json.bak", "s.JSON", "s.json~"] {
            std::fs::write(dir.join(name), CHAT).unwrap();
        }
        let mut adapter = GeminiAdapter;
        assert!(adapter.scan(&resolver(&tmp)).expect("scan").is_empty());
    }

    #[test]
    fn history_is_capped_and_newest_first() {
        // A year of chats is thousands of files; the list is a recent-history
        // view, and the rest stays on disk where it already lives.
        let tmp = TempDir::new().unwrap();
        for i in 0..(MAX_ENDED + 20) {
            let path = fixture(
                &tmp,
                "p1",
                &format!("s{i:04}.json"),
                &format!(r#"{{"session_id":"s{i:04}","messages":[]}}"#),
            );
            let age = RECENT_WINDOW_MS as u64 + 60_000 + i as u64 * 1_000;
            super::super::testutil::backdate(&path, Duration::from_millis(age));
        }
        let mut adapter = GeminiAdapter;
        let ended = adapter.scan_ended(&resolver(&tmp)).expect("ended");
        assert_eq!(ended.len(), MAX_ENDED);
        assert_eq!(ended[0].session_id, "s0000", "newest history first");
    }

    #[test]
    fn the_live_list_is_not_capped_by_the_history_limit() {
        // A busy day with 150 live conversations is 150 rows, and silently
        // dropping 50 of them would be a lie about what is running.
        let tmp = TempDir::new().unwrap();
        for i in 0..(MAX_ENDED + 20) {
            fixture(
                &tmp,
                "p1",
                &format!("s{i:04}.json"),
                &format!(r#"{{"session_id":"s{i:04}","messages":[]}}"#),
            );
        }
        let mut adapter = GeminiAdapter;
        assert_eq!(
            adapter.scan(&resolver(&tmp)).expect("scan").len(),
            MAX_ENDED + 20
        );
    }
}
