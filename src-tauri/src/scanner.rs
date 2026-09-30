//! Runs every detected adapter once and assembles a snapshot. Shared by the
//! in-process loop (Linux build) and the `--agent` role (spawned inside WSL by
//! the Windows build), so both produce byte-identical snapshots.

use crate::adapters::{self, HarnessAdapter};
use crate::herdr;
use crate::model::{now_ms, AgentSession, HarnessId, Snapshot};
use crate::paths::PathResolver;
use crate::quota;

/// How often the ended list is rebuilt, in ticks. It is history: it changes
/// when you start and stop work, not every 1.5s, and rebuilding it on the live
/// cadence would add a per-row message query and a full scan of a database
/// that has grown to thousands of rows.
const ENDED_REFRESH_TICKS: u32 = 20;

pub struct Scanner {
    paths: PathResolver,
    adapters: Vec<Box<dyn HarnessAdapter>>,
    detected: Vec<HarnessId>,
    /// Cached between refreshes - see `ENDED_REFRESH_TICKS`.
    ended: Vec<AgentSession>,
    ticks: u32,
}

impl Scanner {
    pub fn new() -> Self {
        Self::with_paths(PathResolver::detect())
    }

    pub fn with_paths(paths: PathResolver) -> Self {
        let adapters = adapters::all();
        // A harness the user switched off is dropped here rather than filtered
        // per tick, so `detected` means "what this snapshot can contain" and the
        // UI does not have to be told about a harness whose rows will never
        // arrive. `detect()` still comes first: a harness that is not installed
        // cannot be enabled.
        let detected = adapters
            .iter()
            .filter(|a| a.detect(&paths) && adapters::is_enabled(a.id()))
            .map(|a| a.id())
            .collect();
        Self {
            paths,
            adapters,
            detected,
            ended: Vec::new(),
            ticks: 0,
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

        // The first tick populates it, then every ENDED_REFRESH_TICKS after -
        // so the very first snapshot the UI sees is already complete.
        if self.ended.is_empty() || self.ticks % ENDED_REFRESH_TICKS == 0 {
            let mut ended = Vec::new();
            for adapter in self.adapters.iter_mut() {
                if !self.detected.contains(&adapter.id()) {
                    continue;
                }
                match adapter.scan_ended(&self.paths) {
                    Ok(mut found) => ended.append(&mut found),
                    Err(err) => {
                        tracing::warn!(harness = ?adapter.id(), %err, "ended scan failed")
                    }
                }
            }
            ended.sort_by(|a, b| b.state_changed_at.cmp(&a.state_changed_at));
            self.ended = ended;
        }
        self.ticks = self.ticks.wrapping_add(1);

        // A session that is live must not also appear as history. This runs on
        // every tick rather than only on a refresh, because a session that just
        // started is in the fresh live scan while the cached ended list still
        // remembers the dead pid it was two refreshes ago - and a row that says
        // both "running" and "ended" at once is worse than either.
        self.ended = without_live(std::mem::take(&mut self.ended), &sessions);

        Snapshot {
            taken_at: now_ms(),
            detected: self.detected.clone(),
            sessions,
            ended: self.ended.clone(),
            quota: quota::read(&self.paths),
            reseed: false,
        }
    }
}

/// Drop every history row that the live scan just produced.
///
/// Compared on the key, not the whole row: identity is the process, and the
/// two passes can legitimately disagree about tokens.
///
/// The cache is the whole point of this running every tick rather than only on
/// a refresh. A session whose liveness check failed transiently - `/proc` briefly
/// unreadable, so `liveness::check` reports `Dead` - is cached as history, and
/// the next tick's live scan finds the very same process. The keys are equal
/// because the key *is* `(harness, pidDomain, pid, procStart)`, so the filter
/// can recognise it.
fn without_live(mut ended: Vec<AgentSession>, live: &[AgentSession]) -> Vec<AgentSession> {
    let live: std::collections::HashSet<&crate::model::SessionKey> =
        live.iter().map(|s| &s.key).collect();
    ended.retain(|s| !live.contains(&s.key));
    ended
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SessionState, TokenCounts};
    use crate::paths::PathResolver;
    use std::path::Path;
    use tempfile::TempDir;

    /// A fixture home. `Scanner::with_paths` is used rather than the `HM_HOME`
    /// env var: these tests run in parallel in one process and would otherwise
    /// fight over it.
    fn home() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude/sessions")).unwrap();
        tmp
    }

    fn scanner(tmp: &TempDir) -> Scanner {
        Scanner::with_paths(PathResolver::for_home(tmp.path().to_path_buf()))
    }

    fn of_claude(sessions: &[AgentSession]) -> Vec<&AgentSession> {
        sessions
            .iter()
            .filter(|s| s.key.harness == HarnessId::ClaudeCode)
            .collect()
    }

    /// A state file for a pid that is not alive, so it is filed as history.
    fn write_ghost(dir: &Path, pid: u32, name: &str, updated: i64) {
        std::fs::write(
            dir.join(format!("{pid}.json")),
            format!(
                r#"{{"pid":{pid},"sessionId":"s-{pid}","cwd":"/tmp","name":"{name}",
                    "procStart":"1","status":"idle","statusUpdatedAt":{updated},
                    "startedAt":{updated},"pidDomain":"linux:test:pid:[1]"}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn the_very_first_tick_already_has_history() {
        // The first snapshot the UI sees has to be complete: an empty ended
        // list for the first 30 seconds would read as "you have run nothing".
        let tmp = home();
        write_ghost(&tmp.path().join(".claude/sessions"), 900_001, "old", 1_000);
        let mut s = scanner(&tmp);
        let snap = s.tick();
        assert_eq!(of_claude(&snap.ended).len(), 1);
    }

    #[test]
    fn the_ended_list_is_cached_between_refreshes() {
        let tmp = home();
        let dir = tmp.path().join(".claude/sessions");
        write_ghost(&dir, 900_001, "old", 1_000);
        let mut s = scanner(&tmp);
        assert_eq!(of_claude(&s.tick().ended).len(), 1);

        // A new ghost appears. The cached list should not know about it yet.
        write_ghost(&dir, 900_002, "new", 2_000);
        let snap = s.tick();
        assert_eq!(
            of_claude(&snap.ended).len(),
            1,
            "tick 2 should still be the cached list"
        );
    }

    #[test]
    fn the_ended_list_refreshes_every_twenty_ticks() {
        // It is history: it changes when work starts and stops, not 1.5 times a
        // second, and rebuilding it means a full table scan of opencode's
        // session table.
        let tmp = home();
        let dir = tmp.path().join(".claude/sessions");
        write_ghost(&dir, 900_001, "old", 1_000);
        let mut s = scanner(&tmp);
        s.tick();
        write_ghost(&dir, 900_002, "new", 2_000);

        // Ticks 2..=20 reuse the cache, so the new ghost is still invisible.
        for tick in 2..=ENDED_REFRESH_TICKS {
            assert_eq!(
                of_claude(&s.tick().ended).len(),
                1,
                "tick {tick} should not have refreshed yet"
            );
        }
        // Tick 21 rebuilds.
        let snap = s.tick();
        assert_eq!(
            of_claude(&snap.ended).len(),
            2,
            "tick 21 should have picked up the new ghost"
        );
    }

    #[test]
    fn history_is_newest_first() {
        let tmp = home();
        let dir = tmp.path().join(".claude/sessions");
        write_ghost(&dir, 900_001, "older", 1_000);
        write_ghost(&dir, 900_002, "newer", 5_000);
        let mut s = scanner(&tmp);
        let snap = s.tick();
        let ended = of_claude(&snap.ended);
        assert_eq!(ended[0].name.as_deref(), Some("newer"));
        assert_eq!(ended[1].name.as_deref(), Some("older"));
    }

    #[test]
    fn a_session_that_just_went_live_leaves_the_history_list_on_the_same_tick() {
        // A transient `/proc` read failure makes `liveness::check` report Dead
        // for a process that is still running, so it gets cached as history.
        // The next tick's live scan finds the very same process, and the cache
        // has to let it go - otherwise a row says both "running" and "ended".
        //
        // Not reachable through the filesystem: the key contains `procStart`,
        // which is the very field that decided liveness, so a fixture cannot
        // be a ghost and a live process at the same time. Hence `without_live`
        // is exercised directly below.
        let ghost = session_row(101, "1", SessionState::Ended);
        let mut live = ghost.clone();
        live.state = SessionState::Running;
        let other = session_row(102, "1", SessionState::Ended);

        let cached = vec![ghost, other];
        let filtered = without_live(cached, std::slice::from_ref(&live));
        assert_eq!(
            filtered.iter().map(|s| s.key.pid).collect::<Vec<_>>(),
            vec![102],
            "only the still-dead process should remain in history"
        );
    }

    #[test]
    fn the_filter_ignores_fields_other_than_identity() {
        // The two passes can legitimately disagree about tokens and state; a
        // row that differs only in usage is still the same process.
        let cached = session_row(101, "1", SessionState::Ended);
        let mut live = cached.clone();
        live.state = SessionState::Running;
        live.tokens = Some(TokenCounts {
            input: 5,
            output: 6,
            reasoning: 0,
            cache_read: 0,
            cache_write: 0,
        });
        assert!(without_live(vec![cached], std::slice::from_ref(&live)).is_empty());
    }

    #[test]
    fn a_recycled_pid_is_a_different_process_and_stays_in_history() {
        // Same pid, different start time: a new process that happens to have
        // inherited the number. The old row is history, not a duplicate.
        let cached = session_row(101, "1", SessionState::Ended);
        let live = session_row(101, "2", SessionState::Running);
        assert_eq!(
            without_live(vec![cached], std::slice::from_ref(&live)).len(),
            1
        );
    }

    #[test]
    fn a_different_harness_under_the_same_pid_is_a_different_process() {
        let mut cached = session_row(101, "1", SessionState::Ended);
        cached.key.harness = HarnessId::ClaudeCode;
        let mut live = session_row(101, "1", SessionState::Running);
        live.key.harness = HarnessId::Codex;
        assert_eq!(
            without_live(vec![cached], std::slice::from_ref(&live)).len(),
            1
        );
    }

    #[test]
    fn no_live_sessions_keeps_the_whole_history() {
        let cached = vec![session_row(101, "1", SessionState::Ended)];
        assert_eq!(without_live(cached, &[]).len(), 1);
    }

    /// Minimal row for the filter tests, which care only about identity.
    fn session_row(pid: i64, proc_start: &str, state: SessionState) -> AgentSession {
        AgentSession {
            key: crate::model::SessionKey {
                harness: HarnessId::ClaudeCode,
                pid_domain: "linux:x".into(),
                pid,
                proc_start: proc_start.into(),
            },
            session_id: format!("s-{pid}"),
            cwd: "/tmp".into(),
            name: None,
            state,
            state_changed_at: 1_000,
            started_at: 900,
            waiting_for: None,
            model: None,
            tokens: None,
            cost: None,
            is_background: false,
            tier: crate::model::FidelityTier::Full,
            jump_target: None,
            terminal_title: None,
            liveness: crate::liveness::Liveness::Dead,
        }
    }

    #[test]
    fn an_ended_row_is_never_live() {
        // The state file still says whatever the harness last wrote; presenting
        // that as a live state would be a lie about a process that is gone.
        let tmp = home();
        write_ghost(&tmp.path().join(".claude/sessions"), 900_001, "old", 1_000);
        let mut s = scanner(&tmp);
        for session in s.tick().ended {
            assert_eq!(session.state, SessionState::Ended);
        }
    }

    #[test]
    fn a_home_with_no_harnesses_produces_an_empty_snapshot_rather_than_an_error() {
        let tmp = home();
        let mut s = scanner(&tmp);
        let snap = s.tick();
        assert_eq!(of_claude(&snap.sessions).len(), 0);
        assert_eq!(of_claude(&snap.ended).len(), 0);
        assert!(snap.taken_at > 0);
        assert!(!snap.reseed, "a normal tick is not a respawn");
    }

    #[test]
    fn every_tick_advances_the_clock_the_differ_uses() {
        // A non-advancing `taken_at` would make the UI's "nothing arrived"
        // hint fire while snapshots keep flowing.
        let tmp = home();
        let mut s = scanner(&tmp);
        let first = s.tick().taken_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(s.tick().taken_at > first);
    }

    #[test]
    fn a_wiped_session_directory_is_not_everything_finishing() {
        // The whole directory disappearing is a user deleting files, not 60
        // turns completing. The live list empties at once; the history list
        // holds what it last cached, which is the same bargain it makes
        // everywhere else.
        let tmp = home();
        let dir = tmp.path().join(".claude/sessions");
        write_ghost(&dir, 900_001, "old", 1_000);
        let mut s = scanner(&tmp);
        assert_eq!(of_claude(&s.tick().ended).len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
        let snap = s.tick();
        assert_eq!(of_claude(&snap.sessions).len(), 0);
        assert_eq!(
            of_claude(&snap.ended).len(),
            1,
            "the cache has not refreshed yet"
        );
    }

    #[test]
    fn one_broken_harness_does_not_take_the_loop_down() {
        // A parse failure must degrade one harness, never kill the loop.
        let tmp = home();
        let dir = tmp.path().join(".claude/sessions");
        std::fs::write(dir.join("not-a-session.txt"), "garbage").unwrap();
        std::fs::write(dir.join("999999.abc.key"), "not json").unwrap();
        write_ghost(&dir, 900_001, "old", 1_000);
        let mut s = scanner(&tmp);
        let snap = s.tick();
        assert_eq!(of_claude(&snap.ended).len(), 1);
    }
}
