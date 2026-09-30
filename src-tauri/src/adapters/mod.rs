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

/// How many finished rows one harness contributes, when the user has not said.
///
/// Was a `const MAX_ENDED = 100` in each of the five adapters, each with its own
/// version of the same reason: a year of work is a few thousand files, the
/// database behind opencode and codex keeps every session forever, and the list
/// is a "what did I run recently" view rather than an archive browser. The full
/// history stays on disk where it already lives, so nothing is lost by capping.
///
/// It is one number and it is now a setting, so it lives here rather than in
/// five places that `settings.rs` has to clamp and the WSL agent has to be told
/// about. Set once at startup; fixed for the life of the process, which is the
/// same contract the constant had.
static MAX_ENDED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(100);

pub fn configure(max_ended: usize, enabled: &[String]) {
    MAX_ENDED.store(max_ended.max(1), std::sync::atomic::Ordering::Relaxed);
    ENABLED
        .lock()
        .map(|mut set| {
            *set = enabled
                .iter()
                .filter_map(|name| parse_harness(name))
                .collect();
        })
        .ok();
}

/// The cap, as a `usize` for the `truncate` and SQL `LIMIT` call sites.
pub fn max_ended() -> usize {
    MAX_ENDED.load(std::sync::atomic::Ordering::Relaxed)
}

static ENABLED: std::sync::Mutex<Vec<HarnessId>> = std::sync::Mutex::new(Vec::new());

/// Whether the user has this harness switched on. Empty means "no opinion
/// yet", which is treated as yes: `configure` runs before the first tick, and a
/// scanner that hid everything because nobody had called it yet would be a very
/// confusing empty pill.
pub fn is_enabled(harness: HarnessId) -> bool {
    match ENABLED.lock() {
        Ok(set) if set.is_empty() => true,
        Ok(set) => set.contains(&harness),
        Err(_) => true,
    }
}

fn parse_harness(name: &str) -> Option<HarnessId> {
    match name {
        "claude-code" => Some(HarnessId::ClaudeCode),
        "open-code" => Some(HarnessId::OpenCode),
        "codex" => Some(HarnessId::Codex),
        "gemini" => Some(HarnessId::Gemini),
        "antigravity" => Some(HarnessId::Antigravity),
        _ => None,
    }
}

pub trait HarnessAdapter: Send {
    fn id(&self) -> HarnessId;

    /// Whether this harness's data root exists on this host. A harness that is
    /// not installed must be distinguishable from one that is merely idle.
    fn detect(&self, paths: &PathResolver) -> bool;

    fn scan(&mut self, paths: &PathResolver) -> anyhow::Result<Vec<AgentSession>>;

    /// Sessions this harness can see that are no longer running.
    ///
    /// Separate from `scan` on purpose, not merely by convention: the caller
    /// feeds `scan`'s output to the differ, and a dead pid in there would fire
    /// a "turn complete" toast for a process that vanished weeks ago. An
    /// adapter with nothing to add returns an empty list, which is the common
    /// case - only Claude Code keeps per-process state files around.
    fn scan_ended(&mut self, _paths: &PathResolver) -> anyhow::Result<Vec<AgentSession>> {
        Ok(Vec::new())
    }
}

pub fn all() -> Vec<Box<dyn HarnessAdapter>> {
    vec![
        Box::new(claude_code::ClaudeCodeAdapter::default()),
        Box::new(opencode::OpenCodeAdapter::default()),
        Box::new(codex::CodexAdapter::default()),
        Box::new(gemini::GeminiAdapter),
        Box::new(antigravity::AntigravityAdapter),
    ]
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Backdate a file, so a recency window can be crossed in a test.
    //!
    //! Three of the five harnesses decide live-versus-history by the mtime of a
    //! file they never rewrite, which makes a fresh fixture always "live". There
    //! is no way to arrange that by writing different content, so the tests set
    //! the timestamp instead.

    use std::path::Path;
    use std::time::{Duration, SystemTime};

    pub fn backdate(path: &Path, age: Duration) {
        let when = SystemTime::now() - age;
        let times = std::fs::FileTimes::new().set_modified(when);
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open to set mtime")
            .set_times(times)
            .expect("set mtime");
    }
}
