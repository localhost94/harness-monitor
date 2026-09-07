//! Shared data model. These types cross the process boundary between the
//! WSL-side agent (`--agent`) and the UI process, so every field is serde.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub sessions: Vec<AgentSession>,
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
