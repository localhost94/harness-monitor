//! Antigravity adapter - deliberately the thinnest one in the tree.
//!
//! Conversations are binary protobuf (<uuid>.pb) with no published schema, so
//! the only honest signal is "a conversation file was written recently". No
//! token counts, no turn boundaries, and explicitly no derived state: an
//! mtime cannot distinguish running from waiting from finished, and a guess
//! here would produce false "finished" toasts.

use anyhow::Result;

use super::HarnessAdapter;
use crate::model::{now_ms, AgentSession, FidelityTier, HarnessId, SessionKey, SessionState};
use crate::paths::PathResolver;

const PRESENCE_WINDOW_MS: i64 = 10 * 60 * 1000;

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

        Ok(vec![AgentSession {
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
        }])
    }
}
