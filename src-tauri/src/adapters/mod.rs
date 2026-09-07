//! Adapters return a full snapshot each tick. There is deliberately no
//! streaming trait: every source except opencode is poll-shaped (state files,
//! JSON chats, mtimes), and an adapter that does have an event log keeps its
//! cursor internally behind `&mut self`.
//!
//! Adapters parse. They do not decide liveness, transitions, or notifications
//! - that is `differ.rs`, once, for every harness.

use crate::model::{AgentSession, HarnessId};
use crate::paths::PathResolver;

pub mod antigravity;
pub mod claude_code;
pub mod codex;
pub mod gemini;
pub mod opencode;

pub trait HarnessAdapter: Send {
    fn id(&self) -> HarnessId;

    /// Whether this harness's data root exists on this host. A harness that is
    /// not installed must be distinguishable from one that is merely idle.
    fn detect(&self, paths: &PathResolver) -> bool;

    fn scan(&mut self, paths: &PathResolver) -> anyhow::Result<Vec<AgentSession>>;
}

pub fn all() -> Vec<Box<dyn HarnessAdapter>> {
    vec![
        Box::new(claude_code::ClaudeCodeAdapter),
        Box::new(opencode::OpenCodeAdapter::default()),
        Box::new(codex::CodexAdapter::default()),
        Box::new(gemini::GeminiAdapter),
        Box::new(antigravity::AntigravityAdapter),
    ]
}
