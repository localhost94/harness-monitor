//! Harness data roots per host OS.
//!
//! Deliberately small: the Windows UI process never reads WSL paths over
//! `\\wsl.localhost` - it spawns the same binary inside WSL instead (see
//! `bridge.rs`). What is left here is the genuinely cross-mounted stuff
//! (codex and antigravity live in the Windows user profile, reachable from
//! Linux at /mnt/c/Users/<user>).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PathResolver {
    home: PathBuf,
    /// Windows user profile, as seen from this host. None if not reachable.
    win_home: Option<PathBuf>,
}

impl PathResolver {
    pub fn detect() -> Self {
        // HM_HOME lets tests point the whole resolver at a fixture tree.
        let home = std::env::var_os("HM_HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        let win_home = if cfg!(windows) {
            Some(home.clone())
        } else {
            find_windows_home()
        };
        Self { home, win_home }
    }

    /// Explicit root, for tests and for pointing at another user's tree.
    pub fn for_home(home: PathBuf) -> Self {
        let win_home = if cfg!(windows) {
            Some(home.clone())
        } else {
            find_windows_home()
        };
        Self { home, win_home }
    }

    pub fn claude_root(&self) -> PathBuf {
        self.home.join(".claude")
    }

    pub fn claude_sessions(&self) -> PathBuf {
        self.claude_root().join("sessions")
    }

    pub fn claude_analytics(&self) -> PathBuf {
        self.claude_root().join("llm-analytics-usage")
    }

    /// Where the UI process keeps its own state (window position). Distinct
    /// from `quota_state`: that one is read on whichever host runs the
    /// adapters (Linux), while this belongs to the host showing the window.
    pub fn ui_state_dir(&self) -> PathBuf {
        if cfg!(windows) {
            dirs::data_local_dir()
                .unwrap_or_else(|| self.home.join("AppData/Local"))
                .join("harness-monitor")
        } else {
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| self.home.join(".local/state"))
                .join("harness-monitor")
        }
    }

    /// State file written by the statusline shim (installer/statusline-shim.sh).
    pub fn quota_state(&self) -> PathBuf {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(".local/state"))
            .join("harness-monitor/quota.json")
    }

    pub fn opencode_db(&self) -> PathBuf {
        self.home.join(".local/share/opencode/opencode.db")
    }

    pub fn gemini_root(&self) -> PathBuf {
        self.home.join(".gemini")
    }

    pub fn codex_root(&self) -> Option<PathBuf> {
        let local = self.home.join(".codex");
        if local.is_dir() {
            return Some(local);
        }
        self.win_home.as_ref().map(|w| w.join(".codex")).filter(|p| p.is_dir())
    }

    pub fn antigravity_root(&self) -> Option<PathBuf> {
        self.win_home
            .as_ref()
            .map(|w| w.join(".gemini/antigravity"))
            .filter(|p| p.is_dir())
    }
}

/// Cheapest reliable way to find the Windows profile from WSL: pick the
/// /mnt/c/Users entry that owns a .codex or .gemini dir. Avoids parsing
/// wslpath output or shelling out.
fn find_windows_home() -> Option<PathBuf> {
    let users = Path::new("/mnt/c/Users");
    let entries = std::fs::read_dir(users).ok()?;
    let mut fallback = None;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), "Public" | "Default" | "All Users" | "Default User") {
            continue;
        }
        if p.join(".codex").is_dir() || p.join(".gemini").is_dir() {
            return Some(p);
        }
        fallback.get_or_insert(p);
    }
    fallback
}
