//! Snapshot -> transitions -> notifications.
//!
//! Every anti-noise rule lives here, once, for all harnesses. The rules exist
//! because the naive version is genuinely bad: 20+ session files means a
//! restart would fire 20 toasts, tool calls flap busy<->idle several times a
//! turn, and a dead pid whose file lingers would look like a session that just
//! finished.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{AgentSession, SessionKey, SessionState, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    /// Session stopped and wants something from the user.
    Attention,
    /// Session finished a turn.
    Done,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outgoing {
    One {
        kind: NotifyKind,
        key: SessionKey,
        title: String,
        body: String,
    },
    /// Global rate cap tripped: one toast standing in for several.
    Coalesced { count: usize },
}

#[derive(Debug, Clone)]
pub struct DifferConfig {
    /// Per-session silence after any toast. Absorbs busy<->idle flapping
    /// between tool calls, which is the dominant storm source.
    pub cooldown_ms: i64,
    /// Repeat of the same kind for the same session is dropped inside this
    /// window, unless the reason for waiting changed.
    pub same_kind_ms: i64,
    pub cap_count: usize,
    pub cap_window_ms: i64,
    /// Background (`kind:"bg"`) sessions notify by default; user can opt out.
    pub notify_background: bool,
}

impl Default for DifferConfig {
    fn default() -> Self {
        Self {
            cooldown_ms: 30_000,
            same_kind_ms: 300_000,
            cap_count: 3,
            cap_window_ms: 10_000,
            notify_background: true,
        }
    }
}

#[derive(Debug, Clone)]
struct Tracked {
    state: SessionState,
    state_changed_at: i64,
    /// None until this session has ever produced a toast. A sentinel 0 here
    /// would read as "notified at the epoch", i.e. permanently in cooldown.
    last_notified_at: Option<i64>,
    last_notified_kind: Option<NotifyKind>,
    last_waiting_for: Option<String>,
}

pub struct Differ {
    cfg: DifferConfig,
    seeded: bool,
    tracked: HashMap<SessionKey, Tracked>,
    recent: VecDeque<i64>,
}

impl Differ {
    pub fn new(cfg: DifferConfig) -> Self {
        Self {
            cfg,
            seeded: false,
            tracked: HashMap::new(),
            recent: VecDeque::new(),
        }
    }

    pub fn tracked_len(&self) -> usize {
        self.tracked.len()
    }

    /// Feed one snapshot; get back what should be shown to the user.
    ///
    /// `now` is passed in rather than read so the rate limiting is testable.
    pub fn ingest(&mut self, snap: &Snapshot, now: i64) -> Vec<Outgoing> {
        // Rule 1/8: the first snapshot - and the first after a producer
        // restart - establishes the baseline silently.
        let seeding = !self.seeded || snap.reseed;

        let mut candidates: Vec<(SessionKey, NotifyKind, String, String)> = Vec::new();
        let mut seen: HashSet<SessionKey> = HashSet::with_capacity(snap.sessions.len());

        for session in &snap.sessions {
            seen.insert(session.key.clone());
            let previous = self.tracked.get(&session.key).cloned();
            self.remember(session);

            if seeding {
                continue;
            }
            let Some(prev) = previous else {
                // Rule 3: a session appearing is not an event. It may have been
                // running long before this process started.
                continue;
            };
            if let Some(kind) = classify(&prev, session) {
                if session.is_background && !self.cfg.notify_background {
                    continue;
                }
                if self.suppressed(&prev, kind, session, now) {
                    continue;
                }
                let (title, body) = render(kind, session);
                candidates.push((session.key.clone(), kind, title, body));
            }
        }

        // Rule 7: a session that vanished is forgotten, never announced. A
        // deleted state file (or a pid that died mid-turn) is not completion.
        self.tracked.retain(|key, _| seen.contains(key));
        self.seeded = true;

        self.rate_limit(candidates, now)
    }

    fn remember(&mut self, session: &AgentSession) {
        let entry = self.tracked.entry(session.key.clone()).or_insert(Tracked {
            state: session.state,
            state_changed_at: session.state_changed_at,
            last_notified_at: None,
            last_notified_kind: None,
            last_waiting_for: session.waiting_for.clone(),
        });
        entry.state = session.state;
        entry.state_changed_at = entry.state_changed_at.max(session.state_changed_at);
        entry.last_waiting_for = session.waiting_for.clone();
    }

    fn suppressed(
        &self,
        prev: &Tracked,
        kind: NotifyKind,
        session: &AgentSession,
        now: i64,
    ) -> bool {
        let reason_changed = prev.last_waiting_for != session.waiting_for;
        let Some(last) = prev.last_notified_at else {
            return false; // never notified for this session yet
        };
        // Rule 4: per-session cooldown.
        if now - last < self.cfg.cooldown_ms && !reason_changed {
            return true;
        }
        // Rule 5: a repeated "needs you" for the same reason is noise. Applied
        // to Attention only - two completions 40s apart are two real turns, and
        // the cooldown above is the right guard for those.
        if kind == NotifyKind::Attention
            && prev.last_notified_kind == Some(kind)
            && now - last < self.cfg.same_kind_ms
            && !reason_changed
        {
            return true;
        }
        false
    }

    /// Rule 6: at most `cap_count` toasts per `cap_window_ms`; the rest become
    /// a single summary so a resume storm cannot bury the desktop.
    fn rate_limit(
        &mut self,
        candidates: Vec<(SessionKey, NotifyKind, String, String)>,
        now: i64,
    ) -> Vec<Outgoing> {
        let cutoff = now - self.cfg.cap_window_ms;
        while self.recent.front().is_some_and(|t| *t < cutoff) {
            self.recent.pop_front();
        }

        let mut out = Vec::new();
        let mut coalesced = 0usize;
        for (key, kind, title, body) in candidates {
            if let Some(entry) = self.tracked.get_mut(&key) {
                entry.last_notified_at = Some(now);
                entry.last_notified_kind = Some(kind);
            }
            if self.recent.len() < self.cfg.cap_count {
                self.recent.push_back(now);
                out.push(Outgoing::One {
                    kind,
                    key,
                    title,
                    body,
                });
            } else {
                coalesced += 1;
            }
        }
        if coalesced > 0 {
            out.push(Outgoing::Coalesced { count: coalesced });
        }
        out
    }
}

/// Rule 2: fire on the authoritative timestamp advancing AND the state class
/// changing. Comparing values alone would re-fire forever on a re-read;
/// comparing timestamps alone would fire on every heartbeat write.
fn classify(prev: &Tracked, session: &AgentSession) -> Option<NotifyKind> {
    if session.state_changed_at <= prev.state_changed_at {
        return None;
    }
    if session.state.needs_attention() && !prev.state.needs_attention() {
        return Some(NotifyKind::Attention);
    }
    // Reason for waiting changed (input needed -> permission prompt): still
    // worth telling the user, and rule 5 explicitly lets this through.
    if session.state.needs_attention()
        && prev.state.needs_attention()
        && prev.last_waiting_for != session.waiting_for
    {
        return Some(NotifyKind::Attention);
    }
    // Rule 3: only a real turn end counts as done. Idle arrived at from
    // anywhere else (shell, unknown, a fresh idle session) is not an event.
    if session.state == SessionState::Idle && prev.state == SessionState::Running {
        return Some(NotifyKind::Done);
    }
    None
}

fn render(kind: NotifyKind, session: &AgentSession) -> (String, String) {
    let who = session
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| {
            session
                .cwd
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| session.session_id.chars().take(8).collect());
    let harness = session.key.harness.label();
    match kind {
        NotifyKind::Attention => {
            let reason = session.waiting_for.as_deref().unwrap_or("input needed");
            (
                format!("{who} needs you"),
                format!("{harness} - {reason}\n{}", session.cwd.display()),
            )
        }
        NotifyKind::Done => (
            format!("{who} finished"),
            format!("{harness} - turn complete\n{}", session.cwd.display()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FidelityTier, HarnessId};
    use std::path::PathBuf;

    fn session(pid: i64, state: SessionState, changed_at: i64) -> AgentSession {
        AgentSession {
            key: SessionKey {
                harness: HarnessId::ClaudeCode,
                pid_domain: "linux:x".into(),
                pid,
                proc_start: "1519205".into(),
            },
            session_id: "90ba7df5-9c0c-4996-b592-6b86ae15339c".into(),
            cwd: PathBuf::from("/home/you/code/example-project"),
            name: Some(format!("sess-{pid}")),
            state,
            state_changed_at: changed_at,
            started_at: 0,
            waiting_for: match state {
                SessionState::AwaitingPermission => Some("permission prompt".into()),
                SessionState::AwaitingInput => Some("input needed".into()),
                _ => None,
            },
            model: None,
            tokens: None,
            cost: None,
            is_background: false,
            tier: FidelityTier::Full,
            jump_target: None,
            terminal_title: None,
        }
    }

    fn snap(sessions: Vec<AgentSession>) -> Snapshot {
        Snapshot {
            taken_at: 0,
            detected: vec![HarnessId::ClaudeCode],
            sessions,
            quota: None,
            reseed: false,
        }
    }

    #[test]
    fn first_snapshot_is_silent_even_when_sessions_need_attention() {
        let mut d = Differ::new(DifferConfig::default());
        let sessions = (0..20)
            .map(|i| session(i, SessionState::AwaitingInput, 100))
            .collect();
        assert_eq!(d.ingest(&snap(sessions), 1_000), vec![]);
    }

    #[test]
    fn running_to_idle_fires_done_once() {
        let mut d = Differ::new(DifferConfig::default());
        d.ingest(&snap(vec![session(1, SessionState::Running, 100)]), 1_000);

        let out = d.ingest(&snap(vec![session(1, SessionState::Idle, 200)]), 2_000);
        assert!(matches!(
            out.as_slice(),
            [Outgoing::One { kind: NotifyKind::Done, .. }]
        ));

        // Re-reading the same file must not fire again.
        let again = d.ingest(&snap(vec![session(1, SessionState::Idle, 200)]), 3_000);
        assert_eq!(again, vec![]);
    }

    #[test]
    fn waiting_fires_attention_with_reason() {
        let mut d = Differ::new(DifferConfig::default());
        d.ingest(&snap(vec![session(1, SessionState::Running, 100)]), 1_000);
        let out = d.ingest(
            &snap(vec![session(1, SessionState::AwaitingPermission, 200)]),
            2_000,
        );
        match out.as_slice() {
            [Outgoing::One { kind, body, .. }] => {
                assert_eq!(*kind, NotifyKind::Attention);
                assert!(body.contains("permission prompt"), "body was {body}");
            }
            other => panic!("expected one attention toast, got {other:?}"),
        }
    }

    #[test]
    fn stale_timestamp_never_fires() {
        let mut d = Differ::new(DifferConfig::default());
        d.ingest(&snap(vec![session(1, SessionState::Running, 500)]), 1_000);
        // Same state class change but an older statusUpdatedAt: a stale or
        // rewritten file, not a real transition.
        let out = d.ingest(&snap(vec![session(1, SessionState::Idle, 400)]), 2_000);
        assert_eq!(out, vec![]);
    }

    #[test]
    fn vanished_session_never_reports_done() {
        let mut d = Differ::new(DifferConfig::default());
        d.ingest(&snap(vec![session(1, SessionState::Running, 100)]), 1_000);
        let out = d.ingest(&snap(vec![]), 2_000);
        assert_eq!(out, vec![]);
        assert_eq!(d.tracked_len(), 0);
    }

    #[test]
    fn reseed_after_agent_respawn_is_silent() {
        let mut d = Differ::new(DifferConfig::default());
        d.ingest(&snap(vec![session(1, SessionState::Running, 100)]), 1_000);
        let mut s = snap(vec![session(1, SessionState::Idle, 200)]);
        s.reseed = true;
        assert_eq!(d.ingest(&s, 2_000), vec![]);
    }

    #[test]
    fn cooldown_absorbs_flapping() {
        let mut d = Differ::new(DifferConfig::default());
        d.ingest(&snap(vec![session(1, SessionState::Running, 100)]), 1_000);
        let first = d.ingest(&snap(vec![session(1, SessionState::Idle, 200)]), 2_000);
        assert_eq!(first.len(), 1);
        d.ingest(&snap(vec![session(1, SessionState::Running, 300)]), 3_000);
        // Second completion 5s later: inside the 30s cooldown.
        let second = d.ingest(&snap(vec![session(1, SessionState::Idle, 400)]), 7_000);
        assert_eq!(second, vec![]);
        // Well past the cooldown it is allowed through again.
        d.ingest(&snap(vec![session(1, SessionState::Running, 500)]), 40_000);
        let third = d.ingest(&snap(vec![session(1, SessionState::Idle, 600)]), 41_000);
        assert_eq!(third.len(), 1);
    }

    #[test]
    fn global_cap_coalesces_the_rest() {
        let mut d = Differ::new(DifferConfig::default());
        let running: Vec<_> = (0..6).map(|i| session(i, SessionState::Running, 100)).collect();
        d.ingest(&snap(running), 1_000);
        let done: Vec<_> = (0..6).map(|i| session(i, SessionState::Idle, 200)).collect();
        let out = d.ingest(&snap(done), 2_000);
        assert_eq!(out.len(), 4); // 3 individual + 1 summary
        assert_eq!(out[3], Outgoing::Coalesced { count: 3 });
    }

    #[test]
    fn duplicate_session_ids_are_two_distinct_sessions() {
        // Verified on this machine: sessionId 90ba7df5... appears under two pids.
        let mut d = Differ::new(DifferConfig::default());
        d.ingest(
            &snap(vec![
                session(1978, SessionState::Running, 100),
                session(18299, SessionState::Running, 100),
            ]),
            1_000,
        );
        assert_eq!(d.tracked_len(), 2);
        let out = d.ingest(
            &snap(vec![
                session(1978, SessionState::Idle, 200),
                session(18299, SessionState::Running, 100),
            ]),
            2_000,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn background_sessions_can_be_muted() {
        let cfg = DifferConfig {
            notify_background: false,
            ..Default::default()
        };
        let mut d = Differ::new(cfg);
        let mut bg = session(1, SessionState::Running, 100);
        bg.is_background = true;
        d.ingest(&snap(vec![bg.clone()]), 1_000);
        bg.state = SessionState::Idle;
        bg.state_changed_at = 200;
        assert_eq!(d.ingest(&snap(vec![bg]), 2_000), vec![]);
    }
}
