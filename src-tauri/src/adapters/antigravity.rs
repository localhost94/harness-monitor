//! Antigravity adapter - deliberately the thinnest one in the tree.
//!
//! Conversations are binary protobuf (<uuid>.pb) with no published schema, so
//! the only honest signal is "a conversation file was written recently". No
//! token counts, no turn boundaries, and explicitly no derived state: an
//! mtime cannot distinguish running from waiting from finished, and a guess
//! here would produce false "finished" toasts.

use anyhow::Result;
use std::path::PathBuf;

use super::HarnessAdapter;
use crate::liveness::Liveness;
use crate::model::{now_ms, AgentSession, FidelityTier, HarnessId, SessionKey, SessionState};
use crate::paths::PathResolver;

const PRESENCE_WINDOW_MS: i64 = 10 * 60 * 1000;
/// Same cap as the other harnesses. The list is recent history; the full set
/// of conversation files stays where it is.

#[derive(Default)]
pub struct AntigravityAdapter;

impl HarnessAdapter for AntigravityAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Antigravity
    }

    fn detect(&self, paths: &PathResolver) -> bool {
        paths.antigravity_root().is_some()
    }

    fn scan(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        let Some(root) = paths.antigravity_root() else {
            return Ok(Vec::new());
        };
        let now = now_ms();
        let conversations = root.join("conversations");
        let Ok(entries) = std::fs::read_dir(&conversations) else {
            return Ok(Vec::new());
        };

        let newest = entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("pb"))
            .filter_map(|e| super::codex::modified_ms(&e.path()).map(|t| (t, e.path())))
            .max_by_key(|(t, _)| *t);

        let Some((touched, path)) = newest else {
            return Ok(Vec::new());
        };
        if now - touched > PRESENCE_WINDOW_MS {
            return Ok(Vec::new());
        }

        Ok(vec![presence_session(root, touched, path)])
    }

    /// Conversation files older than the presence window.
    ///
    /// Deliberately thinner than the other harnesses' ended lists, and the
    /// reason is worth stating rather than hiding: a `.pb` file is binary
    /// protobuf with no schema, so all that can be said about an old one is
    /// "a conversation with this id was last touched then". No name, no usage,
    /// no state. Listing it is still better than pretending the history does
    /// not exist, as long as the row claims nothing more than that.
    fn scan_ended(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        let Some(root) = paths.antigravity_root() else {
            return Ok(Vec::new());
        };
        let now = now_ms();
        let Ok(entries) = std::fs::read_dir(root.join("conversations")) else {
            return Ok(Vec::new());
        };

        let mut out: Vec<(i64, std::path::PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("pb"))
            .filter_map(|e| super::codex::modified_ms(&e.path()).map(|t| (t, e.path())))
            .filter(|(t, _)| now - *t > PRESENCE_WINDOW_MS)
            .collect();
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out.truncate(super::max_ended());

        Ok(out
            .into_iter()
            .map(|(touched, path)| {
                let mut session = presence_session(root.clone(), touched, path);
                session.state = SessionState::Ended;
                session.liveness = Liveness::Dead;
                session
            })
            .collect())
    }
}

fn presence_session(root: PathBuf, touched: i64, path: PathBuf) -> AgentSession {
    AgentSession {
        key: SessionKey {
            harness: HarnessId::Antigravity,
            pid_domain: "antigravity:presence".into(),
            pid: 0,
            proc_start: "presence".into(),
        },
        session_id: path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        cwd: root,
        name: Some("antigravity".into()),
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
    }
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

    /// antigravity only ever lives in the Windows profile, so the resolver is
    /// pointed at a fixture rather than at the real /mnt/c/Users.
    fn fixture() -> (TempDir, PathResolver) {
        let home = TempDir::new().unwrap();
        let win = TempDir::new().unwrap();
        let root = win.path().join(".gemini/antigravity/conversations");
        std::fs::create_dir_all(&root).unwrap();
        let paths = PathResolver::with_windows_home(
            home.path().to_path_buf(),
            Some(win.path().to_path_buf()),
        );
        (win, paths)
    }

    fn conversation(root: &std::path::Path, name: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::write(&path, b"\x0a\x03binary protobuf, unreadable either way").unwrap();
        path
    }

    #[test]
    fn a_recent_conversation_is_presence_only() {
        // The thinnest row in the app, and deliberately so: an mtime cannot
        // distinguish running from waiting from finished, and a guess here would
        // produce false "finished" toasts.
        let (win, paths) = fixture();
        let root = win.path().join(".gemini/antigravity/conversations");
        conversation(&root, "9f2c-deadbeef.pb");

        let mut adapter = AntigravityAdapter;
        let found = adapter.scan(&paths).expect("scan");
        assert_eq!(found.len(), 1);
        let session = &found[0];
        assert_eq!(session.tier, FidelityTier::PresenceOnly);
        assert_eq!(session.state, SessionState::ActiveUnknown);
        assert_eq!(session.liveness, Liveness::Unknown);
        assert_eq!(
            session.session_id, "9f2c-deadbeef",
            "the file name is the id"
        );
    }

    #[test]
    fn a_presence_row_claims_nothing_it_cannot_know() {
        // No model, no usage, no cost, no name beyond the harness: every one of
        // those would be a guess, and the UI prints them as measurements.
        let (win, paths) = fixture();
        let root = win.path().join(".gemini/antigravity/conversations");
        conversation(&root, "abc.pb");
        let mut adapter = AntigravityAdapter;
        let session = adapter.scan(&paths).expect("scan").remove(0);
        assert_eq!(session.model, None);
        assert_eq!(session.tokens, None);
        assert_eq!(session.cost, None);
        assert_eq!(session.waiting_for, None);
        assert_eq!(session.jump_target, None);
    }

    #[test]
    fn only_the_newest_recent_conversation_is_reported() {
        // One conversation is all that can honestly be described; a list of
        // them would be a list of identical "something happened" rows.
        let (win, paths) = fixture();
        let root = win.path().join(".gemini/antigravity/conversations");
        let older = conversation(&root, "older.pb");
        conversation(&root, "newer.pb");
        std::thread::sleep(Duration::from_millis(20));
        super::super::testutil::backdate(&older, Duration::from_secs(60));

        let mut adapter = AntigravityAdapter;
        let found = adapter.scan(&paths).expect("scan");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].session_id, "newer");
    }

    #[test]
    fn a_conversation_older_than_the_presence_window_is_not_live() {
        // Ten minutes of silence is history, not "something is happening".
        let (win, paths) = fixture();
        let root = win.path().join(".gemini/antigravity/conversations");
        let old = conversation(&root, "old.pb");
        super::super::testutil::backdate(
            &old,
            Duration::from_millis(PRESENCE_WINDOW_MS as u64 + 60_000),
        );
        let mut adapter = AntigravityAdapter;
        assert!(adapter.scan(&paths).expect("scan").is_empty());
    }

    #[test]
    fn an_old_conversation_still_reaches_the_history_list() {
        // Listing it beats pretending the history does not exist, as long as
        // the row claims nothing more than "a conversation with this id was
        // last touched then".
        let (win, paths) = fixture();
        let root = win.path().join(".gemini/antigravity/conversations");
        let old = conversation(&root, "old.pb");
        super::super::testutil::backdate(
            &old,
            Duration::from_millis(PRESENCE_WINDOW_MS as u64 + 60_000),
        );

        let mut adapter = AntigravityAdapter;
        let ended = adapter.scan_ended(&paths).expect("ended");
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].session_id, "old");
        assert_eq!(ended[0].state, SessionState::Ended);
        assert_eq!(ended[0].liveness, Liveness::Dead);
        assert_eq!(ended[0].tier, FidelityTier::PresenceOnly);
    }

    #[test]
    fn a_recent_conversation_is_in_the_live_list_only() {
        let (win, paths) = fixture();
        let root = win.path().join(".gemini/antigravity/conversations");
        conversation(&root, "recent.pb");
        let mut adapter = AntigravityAdapter;
        assert_eq!(adapter.scan(&paths).expect("scan").len(), 1);
        assert!(adapter.scan_ended(&paths).expect("ended").is_empty());
    }

    #[test]
    fn history_is_newest_first_and_capped() {
        let (win, paths) = fixture();
        let root = win.path().join(".gemini/antigravity/conversations");
        for i in 0..(MAX_ENDED + 20) {
            let path = conversation(&root, &format!("c{i:04}.pb"));
            super::super::testutil::backdate(
                &path,
                Duration::from_millis(PRESENCE_WINDOW_MS as u64 + 60_000 + i as u64 * 1_000),
            );
        }
        let mut adapter = AntigravityAdapter;
        let ended = adapter.scan_ended(&paths).expect("ended");
        assert_eq!(ended.len(), MAX_ENDED);
        assert_eq!(ended[0].session_id, "c0000");
    }

    #[test]
    fn only_pb_files_are_considered() {
        let (win, paths) = fixture();
        let root = win.path().join(".gemini/antigravity/conversations");
        for name in ["notes.txt", "c.json", "c.pb.tmp", "c.PB"] {
            conversation(&root, name);
        }
        let mut adapter = AntigravityAdapter;
        assert!(adapter.scan(&paths).expect("scan").is_empty());
        assert!(adapter.scan_ended(&paths).expect("ended").is_empty());
    }

    #[test]
    fn no_conversations_directory_is_not_an_error() {
        let home = TempDir::new().unwrap();
        let win = TempDir::new().unwrap();
        let paths = PathResolver::with_windows_home(
            home.path().to_path_buf(),
            Some(win.path().to_path_buf()),
        );
        let mut adapter = AntigravityAdapter;
        assert!(adapter.scan(&paths).expect("scan").is_empty());
        assert!(adapter.scan_ended(&paths).expect("ended").is_empty());
    }

    #[test]
    fn an_unreachable_windows_profile_is_not_detected() {
        // Not installed is different from installed-and-quiet, and the UI needs
        // to tell those apart.
        let home = TempDir::new().unwrap();
        let paths = PathResolver::with_windows_home(home.path().to_path_buf(), None);
        let mut adapter = AntigravityAdapter;
        assert!(!adapter.detect(&paths));
        assert!(adapter.scan(&paths).expect("scan").is_empty());
    }

    #[test]
    fn detection_follows_the_conversations_directory() {
        let home = TempDir::new().unwrap();
        let win = TempDir::new().unwrap();
        let adapter = AntigravityAdapter;
        let paths = PathResolver::with_windows_home(
            home.path().to_path_buf(),
            Some(win.path().to_path_buf()),
        );
        assert!(!adapter.detect(&paths), "no .gemini/antigravity at all");

        std::fs::create_dir_all(win.path().join(".gemini/antigravity")).unwrap();
        assert!(
            adapter.detect(&paths),
            "the root exists, even with no chats in it"
        );
    }
}
