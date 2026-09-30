//! Shared data model. These types cross the process boundary between the
//! WSL-side agent (`--agent`) and the UI process, so every field is serde.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::liveness::Liveness;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessId {
    ClaudeCode,
    OpenCode,
    Codex,
    Gemini,
    Antigravity,
}

impl HarnessId {
    pub fn label(self) -> &'static str {
        match self {
            HarnessId::ClaudeCode => "Claude Code",
            HarnessId::OpenCode => "opencode",
            HarnessId::Codex => "codex",
            HarnessId::Gemini => "gemini-cli",
            HarnessId::Antigravity => "antigravity",
        }
    }
}

/// How much a harness can actually tell us. Rendered in the UI so a
/// presence-only harness is never mistaken for a fully tracked one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FidelityTier {
    /// Authoritative state + usage (Claude Code).
    Full,
    /// Usage is exact, state is inferred from recency (opencode, codex).
    UsageOnly,
    /// Only "something changed recently" (gemini, antigravity).
    PresenceOnly,
}

/// `sessionId` is NOT unique - a resumed session reuses it under a new pid.
/// Identity is the process itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey {
    pub harness: HarnessId,
    pub pid_domain: String,
    pub pid: i64,
    pub proc_start: String,
}

impl SessionKey {
    pub fn ui_id(&self) -> String {
        format!(
            "{:?}:{}:{}:{}",
            self.harness, self.pid_domain, self.pid, self.proc_start
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionState {
    /// Agent is working on a turn.
    Running,
    /// Agent stopped and is waiting for the user to type something.
    AwaitingInput,
    /// Agent stopped on a permission prompt.
    AwaitingPermission,
    /// Turn finished, nothing pending.
    Idle,
    /// User dropped into a shell from the harness.
    Shell,
    /// Harness only exposes "recently active" (presence tier).
    ActiveUnknown,
    /// The session is over: its process is gone, or its row fell outside the
    /// recency window its harness uses to mean "still around".
    ///
    /// A distinct state rather than Idle, because a dead process frozen at
    /// `status:"busy"` is not idle - it is nothing. Rendering it as idle would
    /// tell the user a session is calmly waiting for them when it died months
    /// ago.
    ///
    /// Ended sessions only ever appear in `Snapshot::ended`, which the differ
    /// does not read - so this can never fire a notification.
    Ended,
}

impl SessionState {
    pub fn needs_attention(self) -> bool {
        matches!(
            self,
            SessionState::AwaitingInput | SessionState::AwaitingPermission
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSession {
    pub key: SessionKey,
    pub session_id: String,
    pub cwd: PathBuf,
    pub name: Option<String>,
    pub state: SessionState,
    /// Authoritative, monotonic-per-harness moment the state last changed
    /// (epoch ms). The differ fires on this advancing, not on value equality,
    /// so transitions that happen between two polls are still caught.
    pub state_changed_at: i64,
    pub started_at: i64,
    pub waiting_for: Option<String>,
    pub model: Option<String>,
    pub tokens: Option<TokenCounts>,
    pub cost: Option<f64>,
    pub is_background: bool,
    pub tier: FidelityTier,
    /// Opaque handle for "take me to this session" - a herdr pane id. None
    /// when nothing on this host knows where the session lives.
    #[serde(default)]
    pub jump_target: Option<String>,
    /// Tab/pane title of the terminal hosting it, when known - the fastest way
    /// for a human to recognise the right window.
    #[serde(default)]
    pub terminal_title: Option<String>,
    /// Only ever `Alive` or `Unknown` in `Snapshot::sessions`: anything the
    /// process table says is dead goes to `Snapshot::ended` instead. Carried so
    /// the UI can say why an ended row cannot be trusted to be idle.
    #[serde(default)]
    pub liveness: Liveness,
}

/// Plan-level rate limit window. Mirrored verbatim from Claude Code - never
/// derived from token counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaSnapshot {
    /// Epoch ms the reading was produced. Used to render staleness.
    pub at: i64,
    pub source: String,
    pub tier: Option<String>,
    pub five_hour_pct: Option<f64>,
    pub five_hour_resets_at: Option<String>,
    pub seven_day_pct: Option<f64>,
    pub seven_day_resets_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub taken_at: i64,
    /// Harnesses whose data root was found on this host. Adapters that did not
    /// detect are absent from `sessions` because they are not installed, not
    /// because they are idle - the UI needs to tell those apart.
    pub detected: Vec<HarnessId>,
    /// Sessions whose process is confirmed alive, or whose liveness cannot be
    /// determined on this host. This is the only list the differ reads, and
    /// the only one the pill counts - so a ghost can never reach either.
    pub sessions: Vec<AgentSession>,
    /// Sessions this machine can see but that are no longer running: a dead
    /// pid whose state file was never cleaned up, or a database row older than
    /// the harness's own recency window.
    ///
    /// Display-only by construction. They are here because "what have I run"
    /// is a question the live list cannot answer - it is empty the moment the
    /// work is over, which is exactly when you want to look.
    #[serde(default)]
    pub ended: Vec<AgentSession>,
    pub quota: Option<QuotaSnapshot>,
    /// Set when the producer restarted. Tells the differ to reseed instead of
    /// diffing, so an agent respawn does not fire a toast per session.
    #[serde(default)]
    pub reseed: bool,
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_ids_serialise_as_kebab_case() {
        // These strings are the wire format the TypeScript `HarnessId` union is
        // typed against. A rename here is a silent runtime mismatch, not a
        // compile error, so it is pinned.
        let wire = [
            (HarnessId::ClaudeCode, "\"claude-code\""),
            (HarnessId::OpenCode, "\"open-code\""),
            (HarnessId::Codex, "\"codex\""),
            (HarnessId::Gemini, "\"gemini\""),
            (HarnessId::Antigravity, "\"antigravity\""),
        ];
        for (id, expected) in wire {
            assert_eq!(serde_json::to_string(&id).unwrap(), expected);
            let back: HarnessId = serde_json::from_str(expected).unwrap();
            assert_eq!(back, id);
        }
    }

    #[test]
    fn session_states_serialise_as_kebab_case() {
        let wire = [
            (SessionState::Running, "\"running\""),
            (SessionState::AwaitingInput, "\"awaiting-input\""),
            (SessionState::AwaitingPermission, "\"awaiting-permission\""),
            (SessionState::Idle, "\"idle\""),
            (SessionState::Shell, "\"shell\""),
            (SessionState::ActiveUnknown, "\"active-unknown\""),
            (SessionState::Ended, "\"ended\""),
        ];
        for (state, expected) in wire {
            assert_eq!(serde_json::to_string(&state).unwrap(), expected);
            let back: SessionState = serde_json::from_str(expected).unwrap();
            assert_eq!(back, state);
        }
    }

    #[test]
    fn fidelity_tiers_serialise_as_kebab_case() {
        let wire = [
            (FidelityTier::Full, "\"full\""),
            (FidelityTier::UsageOnly, "\"usage-only\""),
            (FidelityTier::PresenceOnly, "\"presence-only\""),
        ];
        for (tier, expected) in wire {
            assert_eq!(serde_json::to_string(&tier).unwrap(), expected);
            let back: FidelityTier = serde_json::from_str(expected).unwrap();
            assert_eq!(back, tier);
        }
    }

    #[test]
    fn only_the_two_blocking_states_need_attention() {
        // The differ fires on exactly this predicate, so a state added here by
        // accident would start notifying.
        for state in [
            SessionState::Running,
            SessionState::AwaitingInput,
            SessionState::AwaitingPermission,
            SessionState::Idle,
            SessionState::Shell,
            SessionState::ActiveUnknown,
            SessionState::Ended,
        ] {
            let expected = matches!(
                state,
                SessionState::AwaitingInput | SessionState::AwaitingPermission
            );
            assert_eq!(state.needs_attention(), expected, "{state:?}");
        }
    }

    #[test]
    fn ended_never_needs_attention() {
        // Structural, not incidental: nothing in `Snapshot::ended` is read by
        // the differ, so this is belt and braces on a safety argument.
        assert!(!SessionState::Ended.needs_attention());
    }

    #[test]
    fn ui_id_is_unique_per_process_not_per_session() {
        // Resuming a session reuses its sessionId under a new pid, so identity
        // has to include the pid and its start time.
        let base = SessionKey {
            harness: HarnessId::ClaudeCode,
            pid_domain: "linux:x".into(),
            pid: 100,
            proc_start: "555".into(),
        };
        let same = base.clone();
        assert_eq!(base.ui_id(), same.ui_id());

        let mut new_pid = base.clone();
        new_pid.pid = 101;
        assert_ne!(base.ui_id(), new_pid.ui_id());

        let mut recycled = base.clone();
        recycled.proc_start = "556".into();
        assert_ne!(base.ui_id(), recycled.ui_id());

        let mut other_domain = base.clone();
        other_domain.pid_domain = "windows:y".into();
        assert_ne!(base.ui_id(), other_domain.ui_id());
    }

    #[test]
    fn ui_id_uses_the_debug_form_of_the_harness() {
        let key = SessionKey {
            harness: HarnessId::OpenCode,
            pid_domain: "linux:x".into(),
            pid: 7,
            proc_start: "1".into(),
        };
        assert_eq!(key.ui_id(), "OpenCode:linux:x:7:1");
    }

    #[test]
    fn every_harness_has_a_human_label() {
        for (id, expected) in [
            (HarnessId::ClaudeCode, "Claude Code"),
            (HarnessId::OpenCode, "opencode"),
            (HarnessId::Codex, "codex"),
            (HarnessId::Gemini, "gemini-cli"),
            (HarnessId::Antigravity, "antigravity"),
        ] {
            assert_eq!(id.label(), expected);
        }
    }

    /// A snapshot as the UI half expects to receive it, with only the fields
    /// that have no sensible default omitted.
    const MINIMAL_SESSION: &str = r#"{
        "key": {"harness":"claude-code","pid_domain":"linux:x","pid":1,"proc_start":"9"},
        "session_id":"a","cwd":"/tmp","name":null,"state":"running",
        "state_changed_at":1000,"started_at":900,"waiting_for":null,
        "model":null,"tokens":null,"cost":null,"is_background":false,"tier":"full"
    }"#;

    #[test]
    fn optional_session_fields_default_when_absent() {
        // A field added to AgentSession must not break an older agent binary
        // still streaming the shape it was built with.
        let parsed: AgentSession = serde_json::from_str(MINIMAL_SESSION).unwrap();
        assert_eq!(parsed.jump_target, None);
        assert_eq!(parsed.terminal_title, None);
        assert_eq!(parsed.liveness, Liveness::Unknown);
    }

    #[test]
    fn liveness_defaults_to_unknown_not_alive() {
        // `unknown` is not a synonym for alive: a host that cannot check must
        // not have its silence read as a confirmation.
        assert_eq!(Liveness::default(), Liveness::Unknown);
        let parsed: AgentSession = serde_json::from_str(MINIMAL_SESSION).unwrap();
        assert_eq!(parsed.liveness, Liveness::Unknown);
    }

    #[test]
    fn a_snapshot_without_ended_or_reseed_still_parses() {
        let body = format!(
            r#"{{"taken_at":1,"detected":["claude-code"],"sessions":[{MINIMAL_SESSION}],
                "quota":null}}"#
        );
        let snap: Snapshot = serde_json::from_str(&body).unwrap();
        assert!(snap.ended.is_empty(), "ended defaults to empty");
        assert!(!snap.reseed, "reseed defaults to false");
        assert_eq!(snap.sessions.len(), 1);
    }

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let snap = Snapshot {
            taken_at: 1_700_000_000_000,
            detected: vec![HarnessId::ClaudeCode, HarnessId::Antigravity],
            sessions: vec![serde_json::from_str(MINIMAL_SESSION).unwrap()],
            ended: Vec::new(),
            quota: Some(QuotaSnapshot {
                at: 1_700_000_000_000,
                source: "statusline".into(),
                tier: Some("default_claude_max_5x".into()),
                five_hour_pct: Some(64.0),
                five_hour_resets_at: Some("2026-09-06T13:00:00Z".into()),
                seven_day_pct: Some(34.0),
                seven_day_resets_at: None,
            }),
            reseed: true,
        };
        let body = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&body).unwrap();
        assert_eq!(back.taken_at, snap.taken_at);
        assert_eq!(back.detected, snap.detected);
        assert_eq!(back.sessions, snap.sessions);
        assert!(back.reseed);
        assert_eq!(back.quota.unwrap().five_hour_pct, Some(64.0));
    }

    #[test]
    fn a_cwd_survives_the_boundary_as_a_path() {
        // WSL paths reach a Windows UI process as text; it has to come back as
        // the same path, not a mangled one.
        let body = MINIMAL_SESSION.replace("/tmp", "C:\\\\Users\\\\you\\\\code");
        let parsed: AgentSession = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.cwd, PathBuf::from("C:\\Users\\you\\code"));
    }

    #[test]
    fn token_counts_default_to_zero() {
        let counts = TokenCounts::default();
        assert_eq!(counts.input, 0);
        assert_eq!(counts.output, 0);
        assert_eq!(counts.reasoning, 0);
        assert_eq!(counts.cache_read, 0);
        assert_eq!(counts.cache_write, 0);
    }

    #[test]
    fn now_ms_is_a_plausible_epoch_millisecond_stamp() {
        // Guards against a unit slip: seconds would be ~1.7e9, not ~1.7e12.
        let now = now_ms();
        assert!(now > 1_600_000_000_000, "got {now}");
        assert!(now < 4_000_000_000_000, "got {now}");
    }
}
