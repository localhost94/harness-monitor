//! Runs every detected adapter once and assembles a snapshot. Shared by the
//! in-process loop (Linux build) and the `--agent` role (spawned inside WSL by
//! the Windows build), so both produce byte-identical snapshots.

use crate::adapters::{self, HarnessAdapter};
use crate::herdr;
use crate::model::{now_ms, HarnessId, Snapshot};
use crate::paths::PathResolver;
use crate::quota;

pub struct Scanner {
    paths: PathResolver,
    adapters: Vec<Box<dyn HarnessAdapter>>,
    detected: Vec<HarnessId>,
}

impl Scanner {
    pub fn new() -> Self {
        Self::with_paths(PathResolver::detect())
    }

    pub fn with_paths(paths: PathResolver) -> Self {
        let adapters = adapters::all();
        let detected = adapters
            .iter()
            .filter(|a| a.detect(&paths))
            .map(|a| a.id())
            .collect();
        Self {
            paths,
            adapters,
            detected,
        }
    }

    pub fn detected(&self) -> &[HarnessId] {
        &self.detected
    }

    pub fn tick(&mut self) -> Snapshot {
        let mut sessions = Vec::new();
        for adapter in self.adapters.iter_mut() {
            if !self.detected.contains(&adapter.id()) {
                continue;
            }
            match adapter.scan(&self.paths) {
                Ok(mut found) => sessions.append(&mut found),
                // A parse failure must degrade one harness, never kill the loop.
                Err(err) => tracing::warn!(harness = ?adapter.id(), %err, "adapter scan failed"),
            }
        }
        // Adapters know what a session is doing; herdr knows where it is.
        let locations = herdr::locations();
        if !locations.is_empty() {
            for session in sessions.iter_mut() {
                if let Some(found) = locations.get(&session.session_id) {
                    session.jump_target = Some(found.pane_id.clone());
                    session.terminal_title = found.terminal_title.clone();
                }
            }
        }

        Snapshot {
            taken_at: now_ms(),
            detected: self.detected.clone(),
            sessions,
            quota: quota::read(&self.paths),
            reseed: false,
        }
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}
