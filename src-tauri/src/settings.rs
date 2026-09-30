//! User settings: one JSON file, read by the UI process only.
//!
//! Every value here is either a UI preference or a number the scanner was
//! started with. Nothing in this file is read by the `--agent` process, and
//! that is deliberate rather than incidental. On Windows the scanner does not
//! run here at all - it runs inside WSL as a separate process that only ever
//! writes NDJSON to stdout - so a setting the scanner needs cannot be handed
//! to it by re-reading a file the other side does not share. Instead the UI
//! passes those values as command-line arguments when it spawns the agent, and
//! the one-shot `--run-again` process gets its values the same way. See
//! `bridge.rs` and `main.rs`.
//!
//! The consequence is that scanning settings change on the next app start, and
//! the UI says so rather than pretending otherwise. The two notification
//! toggles are exempt: they are read on the UI side and pushed to the running
//! pipeline, so they apply immediately.
//!
//! A file that is missing, unreadable, truncated or full of keys from a future
//! version must never stop the app opening. Every field is
//! `#[serde(default)]` and `load` falls back to the defaults wholesale, which
//! means an unknown key or a bad type costs the user their preferences and
//! nothing else.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::HarnessId;
use crate::paths::PathResolver;

/// Where a resumed session opens.
///
/// Three answers because "where should this open" is genuinely a matter of the
/// machine: with herdr installed a pane is the right answer, on a bare WSL box
/// with no herdr there is no addressable terminal at all and a new window is
/// the only option, and someone whose panes are managed by something else wants
/// us to keep out of the way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchTarget {
    /// herdr when it is installed, a terminal we find ourselves otherwise.
    #[default]
    Auto,
    /// herdr only: fail rather than open a window the user did not ask for.
    Herdr,
    /// Never herdr. Useful where panes are managed by something else.
    Terminal,
}

/// Faster than this and the adapters spend more time reading than the UI does
/// drawing; slower than this and a permission prompt can sit unnoticed for
/// several seconds. Both bounds are the point - the setting is a preference,
/// not a way to brick the machine.
const MIN_INTERVAL_MS: u64 = 500;
const MAX_INTERVAL_MS: u64 = 60_000;
const MIN_ENDED: usize = 10;
const MAX_ENDED: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Persisted so a restart does not hand back a widget that starts shouting
    /// about sessions the user silenced an hour ago.
    pub muted: bool,
    /// Skip toasts while the pill itself has focus. On by default: the row
    /// highlight is the notification when you are already looking at it.
    pub suppress_when_focused: bool,

    // --- read when the scanner starts, so these apply on the next launch ---
    pub interval_ms: u64,
    pub max_ended: usize,
    /// Harnesses to scan and show. A harness that is not installed cannot be
    /// enabled, so this can only narrow what was already detected.
    pub enabled: Vec<HarnessId>,

    // --- read when a session is resumed ---
    pub rerun_target: LaunchTarget,
    pub rerun_focus: bool,
    pub rerun_timeout_ms: u64,
}

/// Appearance is deliberately absent.
///
/// Theme, widget shape and which tab opens first are read by the frontend
/// synchronously while the store is being created, because a floating widget is
/// on screen before any of this has loaded and getting it wrong means a visible
/// flash of the wrong theme. Reading them over an IPC round trip would trade
/// that away to no benefit - nothing on this side of the app cares what colour
/// the panel is. They live in `localStorage`, and the settings popover edits
/// them there.
///
/// What that means in practice is two stores, so the split has to be a
/// deliberate line rather than an accident: a preference the *pipeline* reads
/// belongs here, and a preference the *first render* reads does not.
impl Default for Settings {
    fn default() -> Self {
        Self {
            muted: false,
            suppress_when_focused: true,
            interval_ms: 1_500,
            max_ended: 100,
            enabled: vec![
                HarnessId::ClaudeCode,
                HarnessId::OpenCode,
                HarnessId::Codex,
                HarnessId::Gemini,
                HarnessId::Antigravity,
            ],
            rerun_target: LaunchTarget::Auto,
            rerun_focus: true,
            rerun_timeout_ms: 30_000,
        }
    }
}

impl Settings {
    /// Clamp everything a hand-edited file could set to a hostile value.
    ///
    /// `interval_ms: 0` is the one that would actually hurt: the scanner would
    /// spin a core re-reading every session file with no pause at all.
    pub fn sanitized(mut self) -> Self {
        self.interval_ms = self.interval_ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
        self.max_ended = self.max_ended.clamp(MIN_ENDED, MAX_ENDED);
        self.rerun_timeout_ms = self.rerun_timeout_ms.clamp(3_000, 300_000);

        // A duplicate would make the enabled count read wrong and a filter that
        // disagrees with itself; an unknown id cannot be scanned, so drop it
        // rather than carrying a harness nothing can satisfy.
        let mut seen = Vec::new();
        self.enabled.retain(|h| {
            if seen.contains(h) {
                false
            } else {
                seen.push(*h);
                true
            }
        });
        if self.enabled.is_empty() {
            // An empty list would hide every session, which reads as a broken
            // app rather than a choice. "Show everything" is the safe floor.
            self.enabled = Settings::default().enabled;
        }
        self
    }

    /// The subset that only takes effect on the next launch, so the UI can say
    /// so instead of accepting the click and changing nothing visible.
    pub fn needs_restart(applied: &Settings, edited: &Settings) -> bool {
        applied.interval_ms != edited.interval_ms
            || applied.max_ended != edited.max_ended
            || applied.enabled != edited.enabled
    }
}

/// What the frontend gets back from a settings read or write.
///
/// A wrapper rather than a bare `Settings` because two of the answers are not
/// settings: whether there was a file to read at all, and whether what the user
/// just changed is now visible. Sending `Settings` alone would leave the UI
/// unable to say "applies on restart", which is the difference between a
/// preference and a setting that silently does nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsView {
    pub settings: Settings,
    /// False on a fresh install, and the frontend's cue to migrate the
    /// preferences an older build left in localStorage.
    pub loaded_from_file: bool,
    /// True when the scan settings differ from what the running scanner was
    /// started with.
    pub needs_restart: bool,
    /// Set when the file could not be written. The values are still returned
    /// so the controls show what the user asked for; the panel says it did not
    /// stick.
    #[serde(default)]
    pub error: Option<String>,
}

pub fn file() -> PathBuf {
    PathResolver::detect().ui_state_dir().join("settings.json")
}

/// The settings in force, and whether the file was there to be read.
///
/// The flag is how the UI tells "you have never set anything" from "you set
/// everything back to the default".
pub fn load() -> (Settings, bool) {
    load_from(&file())
}

/// `load` against an explicit path, so the round trip can be tested without
/// writing to the developer's real state directory. Same shape as
/// `placement::load_from`.
pub fn load_from(path: &Path) -> (Settings, bool) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (Settings::default(), false);
    };
    match serde_json::from_str::<Settings>(&raw) {
        Ok(settings) => (settings.sanitized(), true),
        Err(err) => {
            // Worth a log line: the user's preferences are going back to
            // defaults and without this the only symptom is that they
            // mysteriously forgot their shape.
            tracing::warn!(path = %path.display(), %err, "unreadable settings file; using defaults");
            (Settings::default(), false)
        }
    }
}

pub fn save(settings: &Settings) -> Result<(), String> {
    save_to(&file(), settings)
}

/// `save` against an explicit path, for the same reason as `load_from`.
pub fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let body =
        serde_json::to_string_pretty(settings).map_err(|e| format!("encoding settings: {e}"))?;
    std::fs::write(path, body).map_err(|e| format!("writing {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_values_the_app_had_hardcoded() {
        let d = Settings::default();
        assert_eq!(d.interval_ms, 1_500);
        assert_eq!(d.max_ended, 100);
        assert!(d.suppress_when_focused);
        assert!(d.rerun_focus);
        assert_eq!(d.rerun_timeout_ms, 30_000);
        assert_eq!(d.enabled.len(), 5);
    }

    #[test]
    fn an_empty_file_yields_defaults() {
        // What `serde` does with a field it has never heard of, and what a
        // file truncated to nothing must also do.
        let parsed: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, Settings::default());
    }

    #[test]
    fn unknown_keys_are_ignored_not_fatal() {
        let raw = r#"{"muted":true,"fromTheFuture":{"a":1},"interval_ms":3000}"#;
        let parsed: Settings = serde_json::from_str(raw).unwrap();
        assert!(parsed.muted);
        assert_eq!(parsed.interval_ms, 3_000);
    }

    #[test]
    fn a_zero_interval_cannot_spin_the_scanner() {
        let hostile = Settings {
            interval_ms: 0,
            max_ended: 0,
            rerun_timeout_ms: 1,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(hostile.interval_ms, MIN_INTERVAL_MS);
        assert_eq!(hostile.max_ended, MIN_ENDED);
        // herdr refuses anything under 3s, so asking for less is not a faster
        // launch, it is a failed one.
        assert_eq!(hostile.rerun_timeout_ms, 3_000);
    }

    #[test]
    fn disabling_every_harness_falls_back_to_enabling_them() {
        let empty = Settings {
            enabled: Vec::new(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(empty.enabled.len(), 5);
    }

    #[test]
    fn a_repeated_harness_is_written_once() {
        let dupes = Settings {
            enabled: vec![HarnessId::Codex, HarnessId::Codex, HarnessId::Gemini],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(dupes.enabled, vec![HarnessId::Codex, HarnessId::Gemini]);
    }

    #[test]
    fn only_scan_settings_are_restart_gated() {
        let applied = Settings::default();
        // Mute and the resume preferences all apply without a restart, because
        // the pipeline and the launch path both read them on this side of WSL.
        let live_only = Settings {
            muted: true,
            rerun_focus: false,
            rerun_timeout_ms: 60_000,
            suppress_when_focused: false,
            ..Settings::default()
        };
        assert!(!Settings::needs_restart(&applied, &live_only));

        for scan_changed in [
            Settings {
                interval_ms: 5_000,
                ..Settings::default()
            },
            Settings {
                max_ended: 250,
                ..Settings::default()
            },
            Settings {
                enabled: vec![HarnessId::Codex],
                ..Settings::default()
            },
        ] {
            assert!(Settings::needs_restart(&applied, &scan_changed));
        }
    }

    #[test]
    fn settings_survive_a_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join("hm-settings-roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested/settings.json");

        let edited = Settings {
            muted: true,
            suppress_when_focused: false,
            interval_ms: 3_000,
            max_ended: 250,
            enabled: vec![HarnessId::Codex, HarnessId::Gemini],
            rerun_target: LaunchTarget::Herdr,
            rerun_focus: false,
            rerun_timeout_ms: 60_000,
        };
        save_to(&path, &edited).expect("save");
        // A missing file is the one case that means "never configured", so a
        // file that was written has to come back flagged as present.
        let (loaded, existed) = load_from(&path);
        assert!(existed, "a file we just wrote must read back as present");
        assert_eq!(loaded, edited);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_reports_no_file_rather_than_failing_to_start() {
        let dir = std::env::temp_dir().join("hm-settings-corrupt");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{ this is not json").unwrap();

        let (loaded, existed) = load_from(&path);
        assert_eq!(loaded, Settings::default());
        assert!(!existed, "an unreadable file must not read as configured");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let (loaded, existed) = load_from(Path::new("/definitely/not/here/settings.json"));
        assert_eq!(loaded, Settings::default());
        assert!(!existed);
    }

    #[test]
    fn a_hostile_interval_is_clamped_on_the_way_back_in() {
        // The clamp on write is for the UI's benefit; the clamp on read is what
        // stops a hand-edited file from spinning the scanner.
        let dir = std::env::temp_dir().join("hm-settings-clamp");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"interval_ms":0,"max_ended":999999}"#).unwrap();

        let (loaded, existed) = load_from(&path);
        assert!(existed);
        assert_eq!(loaded.interval_ms, MIN_INTERVAL_MS);
        assert_eq!(loaded.max_ended, MAX_ENDED);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
