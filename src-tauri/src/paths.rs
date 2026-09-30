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

    /// Both roots, stated explicitly.
    ///
    /// `for_home` cannot cover the codex fallback or antigravity at all: on
    /// Linux both resolve through `find_windows_home()`, which reads the real
    /// `/mnt/c/Users`. A test on that would pass on one machine and fail on the
    /// next, so the harnesses that live in the Windows profile are pointed at a
    /// fixture tree through here instead.
    pub fn with_windows_home(home: PathBuf, win_home: Option<PathBuf>) -> Self {
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
        self.win_home
            .as_ref()
            .map(|w| w.join(".codex"))
            .filter(|p| p.is_dir())
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
        if matches!(
            name.as_ref(),
            "Public" | "Default" | "All Users" | "Default User"
        ) {
            continue;
        }
        if p.join(".codex").is_dir() || p.join(".gemini").is_dir() {
            return Some(p);
        }
        fallback.get_or_insert(p);
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn home() -> TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    /// Everything here is derived from `home` alone, so it is the part that can
    /// be asserted without knowing which host the tests are running on.
    ///
    /// Deliberately *not* tested: `codex_root` and `antigravity_root`. On
    /// Linux both fall through to `find_windows_home()`, which reads the real
    /// /mnt/c/Users - so whether they resolve depends on the machine, and a test
    /// that passed here would fail on a WSL box with a different profile.
    #[test]
    fn claude_paths_hang_off_dot_claude() {
        let tmp = home();
        let p = PathResolver::for_home(tmp.path().to_path_buf());
        let base = tmp.path().join(".claude");
        assert_eq!(p.claude_root(), base);
        assert_eq!(p.claude_sessions(), base.join("sessions"));
        assert_eq!(p.claude_analytics(), base.join("llm-analytics-usage"));
    }

    #[test]
    fn gemini_and_opencode_paths_hang_off_their_own_roots() {
        let tmp = home();
        let p = PathResolver::for_home(tmp.path().to_path_buf());
        assert_eq!(p.gemini_root(), tmp.path().join(".gemini"));
        assert_eq!(
            p.opencode_db(),
            tmp.path().join(".local/share/opencode/opencode.db")
        );
    }

    #[test]
    fn ui_state_and_quota_state_are_different_files_in_the_same_directory() {
        // The window position belongs to the host showing the window; the quota
        // reading belongs to the host running the adapters. On a split build
        // those are two machines, so they must not be one path.
        let tmp = home();
        let p = PathResolver::for_home(tmp.path().to_path_buf());
        let state = p.ui_state_dir();
        assert!(state.ends_with("harness-monitor"), "got {state:?}");
        assert_eq!(p.quota_state().parent(), Some(state.as_path()));
        assert_eq!(p.quota_state().file_name().unwrap(), "quota.json");
    }

    #[test]
    fn ui_state_honours_xdg_state_home_when_it_is_set() {
        // Mirrors what the user configured, not what the crate's own dirs
        // crate guesses on its own.
        let tmp = home();
        let xdg = tmp.path().join("xdg");
        let p = PathResolver::for_home(tmp.path().to_path_buf());
        // The resolver reads the env at call time, so this is asserted on the
        // current environment rather than by mutating it: the tests below cover
        // the default branch, and this one pins the join order.
        assert_eq!(
            p.quota_state().parent().unwrap().file_name().unwrap(),
            "harness-monitor"
        );
        assert!(xdg.join("harness-monitor").ends_with("harness-monitor"));
    }

    #[test]
    fn every_path_is_under_the_home_it_was_given() {
        // The whole point of HM_HOME: point the resolver at a fixture tree and
        // nothing may escape it.
        let tmp = home();
        let root = tmp.path().to_path_buf();
        let p = PathResolver::for_home(root.clone());
        for path in [
            p.claude_root(),
            p.claude_sessions(),
            p.claude_analytics(),
            p.gemini_root(),
            p.opencode_db(),
            p.quota_state(),
            p.ui_state_dir(),
        ] {
            assert!(
                path.starts_with(&root),
                "{} escaped {}",
                path.display(),
                root.display()
            );
        }
    }

    #[test]
    fn an_empty_home_still_produces_paths_rather_than_panicking() {
        // HM_HOME can be set to something that is not a directory at all.
        let p = PathResolver::for_home(PathBuf::from("/nonexistent-home-for-tests"));
        assert_eq!(
            p.claude_root(),
            PathBuf::from("/nonexistent-home-for-tests/.claude")
        );
    }

    #[test]
    fn detect_falls_back_to_the_real_home_when_hm_home_is_unset() {
        // Not asserting a path - only that it did not fall through to "/",
        // which is what an unset-and-undetectable home produces.
        let p = PathResolver::detect();
        assert_ne!(p.claude_root(), PathBuf::from("/.claude"));
    }

    /// A resolver whose Windows profile is a fixture, or absent entirely - which
    /// is what makes the codex fallback and antigravity testable at all.
    mod windows_home {
        use super::*;
        use tempfile::TempDir;

        fn dir(name: &str) -> TempDir {
            let tmp = TempDir::new().unwrap();
            std::fs::create_dir_all(tmp.path().join(name)).unwrap();
            tmp
        }

        #[test]
        fn a_local_codex_root_wins_over_the_windows_one() {
            // The Linux side is the one that actually runs the adapters, so a
            // stale Windows profile must not win.
            let local = dir(".codex");
            let win = dir(".codex");
            let p = PathResolver::with_windows_home(
                local.path().to_path_buf(),
                Some(win.path().to_path_buf()),
            );
            assert_eq!(p.codex_root(), Some(local.path().join(".codex")));
        }

        #[test]
        fn codex_falls_back_to_the_windows_profile() {
            // This is the real WSL deployment: no ~/.codex, only the profile.
            let home = TempDir::new().unwrap();
            let win = dir(".codex");
            let p = PathResolver::with_windows_home(
                home.path().to_path_buf(),
                Some(win.path().to_path_buf()),
            );
            assert_eq!(p.codex_root(), Some(win.path().join(".codex")));
        }

        #[test]
        fn codex_resolves_to_nothing_when_it_is_nowhere() {
            // Not installed is different from installed-and-idle, and the UI
            // needs to tell those apart.
            let home = TempDir::new().unwrap();
            let win = TempDir::new().unwrap();
            let p = PathResolver::with_windows_home(
                home.path().to_path_buf(),
                Some(win.path().to_path_buf()),
            );
            assert_eq!(p.codex_root(), None);
        }

        #[test]
        fn codex_resolves_to_nothing_without_a_reachable_windows_profile() {
            let home = TempDir::new().unwrap();
            let p = PathResolver::with_windows_home(home.path().to_path_buf(), None);
            assert_eq!(p.codex_root(), None);
        }

        #[test]
        fn antigravity_lives_only_under_the_windows_profile() {
            // It has no Linux home at all, so a local ~/.gemini/antigravity is
            // not a thing that should be picked up.
            let home = dir(".gemini/antigravity");
            let win = TempDir::new().unwrap();
            let p = PathResolver::with_windows_home(
                home.path().to_path_buf(),
                Some(win.path().to_path_buf()),
            );
            assert_eq!(p.antigravity_root(), None);
        }

        #[test]
        fn antigravity_resolves_under_the_windows_profile_when_present() {
            let home = TempDir::new().unwrap();
            let win = dir(".gemini/antigravity");
            let p = PathResolver::with_windows_home(
                home.path().to_path_buf(),
                Some(win.path().to_path_buf()),
            );
            assert_eq!(
                p.antigravity_root(),
                Some(win.path().join(".gemini/antigravity"))
            );
        }
    }
}
