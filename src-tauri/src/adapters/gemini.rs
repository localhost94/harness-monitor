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
use crate::model::{now_ms, AgentSession, FidelityTier, HarnessId, SessionKey, SessionState, TokenCounts};
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
        session_id: chat.session_id.unwrap_or_else(|| path.to_string_lossy().to_string()),
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
    })
}
