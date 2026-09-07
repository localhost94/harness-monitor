//! herdr integration - optional, and the only thing that makes "jump to that
//! session" possible on this machine.
//!
//! herdr is a terminal workspace manager for AI coding agents. Its panes
//! already know which agent session they host (Claude Code's own hook reports
//! `agent_session_id` to it), so it can answer the question our own adapters
//! cannot: WHERE is this session? A session id is not a valid focus target, so
//! we map session id -> pane id through `herdr agent list` and then call
//! `herdr agent focus <pane>`.
//!
//! Everything here degrades to None: without herdr the app simply offers no
//! jump.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Envelope {
    result: Option<AgentList>,
}

#[derive(Debug, Deserialize)]
struct AgentList {
    #[serde(default)]
    agents: Vec<Agent>,
}

#[derive(Debug, Deserialize)]
struct Agent {
    #[serde(default)]
    pane_id: Option<String>,
    #[serde(default)]
    terminal_title_stripped: Option<String>,
    #[serde(default)]
    terminal_title: Option<String>,
    #[serde(default)]
    agent_session: Option<AgentSessionRef>,
}

#[derive(Debug, Deserialize)]
struct AgentSessionRef {
    /// The harness's own session id - Claude Code's uuid, opencode's `ses_...`.
    #[serde(default)]
    value: Option<String>,
}

/// What herdr can tell us about one session.
#[derive(Debug, Clone)]
pub struct Location {
    pub pane_id: String,
    pub terminal_title: Option<String>,
}

/// Path to the herdr binary. `~/.local/bin` is not on PATH for a
/// non-interactive `wsl.exe --` invocation, so resolve it explicitly rather
/// than trusting the environment.
pub fn binary() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/herdr"));
    }
    candidates.push(PathBuf::from("/usr/local/bin/herdr"));
    candidates.push(PathBuf::from("/usr/bin/herdr"));
    candidates.into_iter().find(|p| p.is_file()).or_else(|| {
        // Last resort: let the OS resolve it from PATH.
        Command::new("herdr")
            .arg("--version")
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|_| PathBuf::from("herdr"))
    })
}

/// session id -> where it lives. Empty map when herdr is absent or unhappy.
pub fn locations() -> HashMap<String, Location> {
    let mut out = HashMap::new();
    let Some(bin) = binary() else {
        return out;
    };
    let Ok(result) = Command::new(bin).args(["agent", "list"]).output() else {
        return out;
    };
    if !result.status.success() {
        return out;
    }
    let Ok(envelope) = serde_json::from_slice::<Envelope>(&result.stdout) else {
        return out;
    };
    let Some(list) = envelope.result else {
        return out;
    };

    for agent in list.agents {
        let Some(pane_id) = agent.pane_id else { continue };
        let Some(session) = agent.agent_session.and_then(|s| s.value) else {
            continue;
        };
        out.insert(
            session,
            Location {
                pane_id,
                terminal_title: agent.terminal_title_stripped.or(agent.terminal_title),
            },
        );
    }
    out
}

/// Focus a pane. Runs inside WSL, where herdr lives.
pub fn focus(target: &str) -> Result<(), String> {
    let bin = binary().ok_or_else(|| "herdr not found on this host".to_string())?;
    let output = Command::new(bin)
        .args(["agent", "focus", target])
        .output()
        .map_err(|e| format!("running herdr: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr)
        .trim()
        .chars()
        .take(200)
        .collect())
}
