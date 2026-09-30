pub mod adapters;
pub mod agent;
pub mod bridge;
pub mod differ;
pub mod herdr;
pub mod liveness;
pub mod model;
mod notify_out;
pub mod paths;
pub mod quota;
pub mod rerun;
pub mod scanner;
pub mod settings;
pub mod single_instance;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WindowEvent};

use differ::{Differ, DifferConfig};
use model::{now_ms, Snapshot};

/// Window geometry per (shape, expanded).
///
/// Three collapsed shapes, because the useful footprint depends on where the
/// screen is free: the wide pill reads at a glance from the bottom, the one
/// line is a strip you can park anywhere without giving up a row of screen,
/// and the vertical stack is for a side edge. Expanding any of them shows the
/// same session list, which needs the wider frame to be readable at all.
const PILL_H: (f64, f64) = (440.0, 80.0);
const PILL_LINE: (f64, f64) = (440.0, 44.0);
const PILL_V: (f64, f64) = (118.0, 300.0);
const EXPANDED: (f64, f64) = (440.0, 580.0);
const WINDOW: &str = "pill";

#[derive(Default)]
pub struct AppState {
    latest: Mutex<Option<Snapshot>>,
    muted: AtomicBool,
    /// Live, unlike the scan settings: the pipeline reads this on every event,
    /// so a change takes effect without restarting the scanner.
    suppress_when_focused: AtomicBool,
    /// Logged once, so a shipped build can prove the webview actually loaded.
    /// A release binary built without the `custom-protocol` feature points at
    /// devUrl and renders "localhost failed to connect" instead - this line is
    /// how you tell that apart from a backend problem.
    frontend_seen: AtomicBool,
    last_move_saved: Mutex<i64>,
    /// What the settings file said when the app started. Kept so the UI can be
    /// told which of the user's edits will not be visible until a restart,
    /// rather than accepting them and appearing to do nothing.
    applied: Mutex<settings::Settings>,
}

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> Option<Snapshot> {
    if !state.frontend_seen.swap(true, Ordering::Relaxed) {
        tracing::info!("frontend connected");
    }
    state.latest.lock().ok().and_then(|s| s.clone())
}

#[tauri::command]
fn is_muted(state: State<'_, AppState>) -> bool {
    state.muted.load(Ordering::Relaxed)
}

#[tauri::command]
fn set_muted(state: State<'_, AppState>, muted: bool) {
    state.muted.store(muted, Ordering::Relaxed);
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> settings::SettingsView {
    let stored = settings::load();
    if let Ok(mut applied) = state.applied.lock() {
        *applied = stored.0.clone();
    }
    // A fresh install carries no file, which is the frontend's cue to migrate
    // preferences an older build left in localStorage.
    settings::SettingsView {
        settings: stored.0,
        loaded_from_file: stored.1,
        needs_restart: false,
        error: None,
    }
}

/// Save settings and push the ones the running pipeline can act on.
///
/// The scan settings are written but not applied: the scanner takes them as
/// command-line arguments when it starts, and on Windows it is not even in this
/// process. `needs_restart` is the honest answer to "I changed the interval and
/// nothing happened".
#[tauri::command]
fn set_settings(
    state: State<'_, AppState>,
    settings: settings::Settings,
) -> settings::SettingsView {
    let clean = settings.sanitized();
    if let Err(err) = settings::save(&clean) {
        tracing::warn!(%err, "could not write settings.json");
        return settings::SettingsView {
            settings: clean,
            loaded_from_file: true,
            needs_restart: false,
            error: Some(err),
        };
    }
    // These two are read on this side of the WSL boundary, so they take effect
    // immediately. Everything else waits. No event is emitted: the frontend
    // made the change, so it already knows, and `set_muted` has never emitted
    // one either.
    state.muted.store(clean.muted, Ordering::Relaxed);
    state
        .suppress_when_focused
        .store(clean.suppress_when_focused, Ordering::Relaxed);

    let needs_restart = state
        .applied
        .lock()
        .map(|applied| settings::Settings::needs_restart(&applied, &clean))
        .unwrap_or(false);
    settings::SettingsView {
        settings: clean,
        loaded_from_file: true,
        needs_restart,
        error: None,
    }
}

/// The pill grows into a list in place; a second window would lose the
/// user's chosen position.
///
/// `shape` is one of `pill` | `line` | `vertical`. A string rather than two
/// booleans, because "vertical and one line at once" is not a thing and two
/// booleans would happily accept it.
#[tauri::command]
fn set_shape(app: AppHandle, shape: String, expanded: bool) -> Result<(), String> {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return Ok(());
    };
    let (w, h) = match (shape.as_str(), expanded) {
        (_, true) => EXPANDED,
        ("vertical", false) => PILL_V,
        ("line", false) => PILL_LINE,
        _ => PILL_H,
    };
    window.set_size(LogicalSize::new(w, h)).map_err(|e| {
        // Logged as well as returned: the frontend treats this as cosmetic,
        // so a persistent failure would otherwise leave no trace anywhere.
        tracing::warn!(%e, shape, expanded, "set_shape failed");
        e.to_string()
    })
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Take the user to the terminal pane running a session.
///
/// herdr lives inside WSL, so on Windows this re-invokes our own binary there
/// in one-shot `--focus` mode rather than hunting for herdr on PATH (a
/// non-interactive `wsl.exe --` shell does not have ~/.local/bin).
#[tauri::command]
fn focus_session(target: String) -> Result<(), String> {
    if cfg!(windows) {
        bridge::focus_via_wsl(&target)
    } else {
        herdr::focus(&target)
    }
}

/// Reopen a finished session's conversation in a terminal.
///
/// Async, and on a blocking thread, because the underlying work is a subprocess
/// that may sit there for as long as the readiness timeout: `herdr agent start`
/// waits for the agent to accept input, and the WSL one-shot is a full
/// `wsl.exe` round trip on top of that. A synchronous command would park the
/// command thread and freeze every other invoke for the duration - including
/// `get_snapshot`, so the pill would stop counting while a resume was pending.
#[tauri::command]
async fn rerun_session(
    state: State<'_, AppState>,
    harness: String,
    session_id: String,
    cwd: String,
) -> Result<(), String> {
    let harness = parse_harness(&harness)?;
    let prefs = state
        .applied
        .lock()
        .map(|s| (s.rerun_target, s.rerun_focus, s.rerun_timeout_ms))
        .unwrap_or((rerun::LaunchTarget::Auto, true, 30_000));
    let (target, focus, timeout_ms) = prefs;
    let opts = rerun::RerunOptions {
        target,
        focus,
        timeout_ms,
    };
    tauri::async_runtime::spawn_blocking(move || {
        if cfg!(windows) {
            bridge::run_again_via_wsl(
                harness_wire_name(harness),
                &session_id,
                &cwd,
                match opts.target {
                    rerun::LaunchTarget::Auto => "auto",
                    rerun::LaunchTarget::Herdr => "herdr",
                    rerun::LaunchTarget::Terminal => "terminal",
                },
                opts.focus,
                opts.timeout_ms,
            )
        } else {
            rerun::run_again(harness, &session_id, &cwd, opts)
        }
    })
    .await
    .map_err(|e| format!("the resume task did not finish: {e}"))?
}

/// The serde name of a harness, which is what the WSL one-shot parses.
fn harness_wire_name(harness: model::HarnessId) -> &'static str {
    match harness {
        model::HarnessId::ClaudeCode => "claude-code",
        model::HarnessId::OpenCode => "open-code",
        model::HarnessId::Codex => "codex",
        model::HarnessId::Gemini => "gemini",
        model::HarnessId::Antigravity => "antigravity",
    }
}

fn parse_harness(name: &str) -> Result<model::HarnessId, String> {
    match name {
        "claude-code" => Ok(model::HarnessId::ClaudeCode),
        "open-code" => Ok(model::HarnessId::OpenCode),
        "codex" => Ok(model::HarnessId::Codex),
        "gemini" => Ok(model::HarnessId::Gemini),
        "antigravity" => Ok(model::HarnessId::Antigravity),
        other => Err(format!("unknown harness {other}")),
    }
}

pub fn run_ui(test_notify: bool) {
    let mut builder = tauri::Builder::default();
    // Must come before every other plugin: a second copy has to be turned
    // away before anything else takes a lock, spawns a thread or opens a
    // window. --test-notify is exempt - it is a diagnostic the user runs
    // *while* the app is up, and blocking it would make the one host that
    // needs checking the one host that cannot check it.
    if !test_notify {
        builder = builder.plugin(single_instance::plugin());
    }
    builder
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            is_muted,
            set_muted,
            get_settings,
            set_settings,
            set_shape,
            quit_app,
            focus_session,
            rerun_session
        ])
        .setup(move |app| {
            // A floating pill lives on the tray, not the Dock. Accessory drops
            // the Dock icon and the app menu bar, which is also what carries the
            // window's skipTaskbar intent on macOS - that flag is a no-op there.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            notify_out::startup_check();
            if test_notify {
                notify_out::send(
                    app.handle(),
                    "HarnessMonitor test",
                    "If you can read this, desktop notifications work here.",
                );
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    handle.exit(0);
                });
                return Ok(());
            }
            // A missing tray implementation (common on bare WSL, which has no
            // appindicator host) must not take the window down with it.
            if let Err(err) = build_tray(app.handle()) {
                tracing::warn!(%err, "tray unavailable; continuing without it");
            }
            placement::restore(app.handle());
            start_pipeline(app.handle().clone());
            tracing::info!("pipeline started");
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the pill hides it; the tray icon is the real lifetime.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            // A drag fires this per frame, so only persist once things settle.
            WindowEvent::Moved(position) => {
                let state = window.state::<AppState>();
                let now = now_ms();
                let mut last = match state.last_move_saved.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
                if now - *last < 400 {
                    return;
                }
                *last = now;
                placement::save(*position);
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running HarnessMonitor");
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / hide", true, None::<&str>)?;
    let mute = MenuItem::with_id(app, "mute", "Mute notifications", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &mute, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().unwrap())
        .tooltip("HarnessMonitor")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "toggle" => {
                if let Some(window) = app.get_webview_window(WINDOW) {
                    let visible = window.is_visible().unwrap_or(false);
                    let _ = if visible {
                        window.hide()
                    } else {
                        window.show()
                    };
                }
            }
            "mute" => {
                let state = app.state::<AppState>();
                let next = !state.muted.load(Ordering::Relaxed);
                state.muted.store(next, Ordering::Relaxed);
                let _ = app.emit("muted", next);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// Source -> differ -> (frontend event, desktop toast).
fn start_pipeline(app: AppHandle) {
    // Read once, here, because these are the values the scanner starts with.
    // The file is the only place they exist, and on Windows the process that
    // needs them is a different one - so they are handed over as arguments
    // rather than read twice in two places that could disagree.
    let (loaded, from_file) = settings::load();
    {
        let state = app.state::<AppState>();
        state.muted.store(loaded.muted, Ordering::Relaxed);
        state
            .suppress_when_focused
            .store(loaded.suppress_when_focused, Ordering::Relaxed);
        let mut applied = state.applied.lock().ok();
        if let Some(slot) = applied.as_deref_mut() {
            *slot = loaded.clone();
        }
    }
    tracing::info!(
        interval_ms = loaded.interval_ms,
        max_ended = loaded.max_ended,
        harnesses = loaded.enabled.len(),
        muted = loaded.muted,
        from_file,
        "settings loaded"
    );

    let (tx, rx) = std::sync::mpsc::channel::<Snapshot>();
    bridge::spawn_source(tx, loaded.interval_ms, &loaded.enabled, loaded.max_ended);

    std::thread::spawn(move || {
        let mut differ = Differ::new(DifferConfig::default());
        for snapshot in rx {
            let events = differ.ingest(&snapshot, now_ms());
            tracing::debug!(
                sessions = snapshot.sessions.len(),
                notifications = events.len(),
                quota_pct = ?snapshot.quota.as_ref().and_then(|q| q.five_hour_pct),
                quota_source = ?snapshot.quota.as_ref().map(|q| q.source.as_str()),
                "snapshot"
            );

            if let Ok(mut slot) = app.state::<AppState>().latest.lock() {
                *slot = Some(snapshot.clone());
            }
            let _ = app.emit("snapshot", &snapshot);

            if app.state::<AppState>().muted.load(Ordering::Relaxed) {
                continue;
            }
            // The original rule was "if the user is already looking at the pill,
            // the row highlight is enough - a toast on top of it is noise". That
            // is now a setting rather than a constant, because it is a matter
            // of taste: someone watching a long list expand may well want the
            // toast even with the panel open.
            if app
                .state::<AppState>()
                .suppress_when_focused
                .load(Ordering::Relaxed)
            {
                let focused = app
                    .get_webview_window(WINDOW)
                    .and_then(|w| w.is_focused().ok())
                    .unwrap_or(false);
                if focused {
                    continue;
                }
            }
            for event in &events {
                notify_out::deliver(&app, event);
            }
        }
    });
}

/// Remembering where the user parked the pill.
///
/// A floating widget that jumps back to a corner on every restart is a widget
/// people close. Saved on move (throttled - a drag emits a Moved per frame)
/// and restored before the window is shown.
mod placement {
    use serde::{Deserialize, Serialize};
    use tauri::{AppHandle, PhysicalPosition};

    use crate::paths::PathResolver;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Placement {
        pub x: i32,
        pub y: i32,
    }

    fn file() -> std::path::PathBuf {
        PathResolver::detect().ui_state_dir().join("window.json")
    }

    pub fn load() -> Option<Placement> {
        load_from(&file())
    }

    pub fn save(position: PhysicalPosition<i32>) {
        save_to(&file(), position);
    }

    /// The file half of the pair, with the path supplied.
    ///
    /// `load`/`save` resolve the state directory from the environment, which a
    /// test cannot pin without mutating global state that every test in the
    /// process shares. Everything except the one line of path resolution is
    /// here, and that is what the tests exercise.
    fn load_from(path: &std::path::Path) -> Option<Placement> {
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn save_to(path: &std::path::Path, position: PhysicalPosition<i32>) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::to_string(&Placement {
            x: position.x,
            y: position.y,
        });
        if let Ok(body) = body {
            let _ = std::fs::write(path, body);
        }
    }

    pub fn restore(app: &AppHandle) {
        use tauri::Manager;
        let Some(saved) = load() else { return };
        if let Some(window) = app.get_webview_window(super::WINDOW) {
            let _ = window.set_position(PhysicalPosition::new(saved.x, saved.y));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        fn at(x: i32, y: i32) -> PhysicalPosition<i32> {
            PhysicalPosition::new(x, y)
        }

        #[test]
        fn a_placement_survives_a_restart() {
            // The whole point: a widget that jumps back to a corner every
            // launch is a widget people close.
            let tmp = TempDir::new().unwrap();
            let file = tmp.path().join("window.json");
            save_to(&file, at(1_920, 1_080));
            assert_eq!(load_from(&file), Some(Placement { x: 1_920, y: 1_080 }));
        }

        #[test]
        fn negative_coordinates_survive_a_multi_monitor_layout() {
            // A monitor to the left of the primary one puts the widget at a
            // negative x, and a u32 coordinate would wrap it to the far right.
            let tmp = TempDir::new().unwrap();
            let file = tmp.path().join("window.json");
            save_to(&file, at(-1_920, -40));
            assert_eq!(load_from(&file), Some(Placement { x: -1_920, y: -40 }));
        }

        #[test]
        fn the_origin_is_a_real_position_not_a_missing_one() {
            let tmp = TempDir::new().unwrap();
            let file = tmp.path().join("window.json");
            save_to(&file, at(0, 0));
            assert_eq!(load_from(&file), Some(Placement { x: 0, y: 0 }));
        }

        #[test]
        fn a_first_run_has_no_placement_rather_than_the_origin() {
            // Jumping to 0,0 on first launch would put the pill in the
            // top-left corner, over the user's editor.
            let tmp = TempDir::new().unwrap();
            assert_eq!(load_from(&tmp.path().join("window.json")), None);
        }

        #[test]
        fn a_corrupt_file_falls_back_to_the_default_position() {
            // Truncated by a crash mid-write, most likely. Refusing to launch
            // would be worse than putting the widget somewhere visible.
            let tmp = TempDir::new().unwrap();
            let file = tmp.path().join("window.json");
            std::fs::write(&file, b"{\"x\": 1, \"y\":").unwrap();
            assert_eq!(load_from(&file), None);
        }

        #[test]
        fn a_file_with_the_wrong_shape_falls_back_too() {
            let tmp = TempDir::new().unwrap();
            let file = tmp.path().join("window.json");
            // Both of the shapes a half-written or foreign file can take.
            std::fs::write(&file, br#"{"a":1,"b":2}"#).unwrap();
            assert_eq!(load_from(&file), None);
            std::fs::write(&file, br#"{"x":"far left","y":2}"#).unwrap();
            assert_eq!(load_from(&file), None);
        }

        #[test]
        fn saving_creates_the_state_directory() {
            // A fresh install has no ~/.local/state/harness-monitor yet, and
            // the first move is the thing that has to work.
            let tmp = TempDir::new().unwrap();
            let file = tmp.path().join("nested/state/window.json");
            assert!(!file.parent().unwrap().exists());
            save_to(&file, at(10, 20));
            assert_eq!(load_from(&file), Some(Placement { x: 10, y: 20 }));
        }

        #[test]
        fn the_last_save_wins() {
            // A drag emits a move per frame, throttled upstream but not here.
            let tmp = TempDir::new().unwrap();
            let file = tmp.path().join("window.json");
            save_to(&file, at(1, 1));
            save_to(&file, at(2, 2));
            save_to(&file, at(3, 3));
            assert_eq!(load_from(&file), Some(Placement { x: 3, y: 3 }));
        }
    }
}
