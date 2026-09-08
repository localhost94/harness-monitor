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
pub mod scanner;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WindowEvent};

use differ::{Differ, DifferConfig};
use model::{now_ms, Snapshot};

/// Window geometry per (orientation, expanded). The vertical strip is meant to
/// live along a screen edge; expanding either shape shows the same session
/// list, which needs the wider frame to be readable at all.
const PILL_H: (f64, f64) = (440.0, 80.0);
const PILL_V: (f64, f64) = (118.0, 300.0);
const EXPANDED: (f64, f64) = (440.0, 580.0);
const WINDOW: &str = "pill";

#[derive(Default)]
pub struct AppState {
    latest: Mutex<Option<Snapshot>>,
    muted: AtomicBool,
    /// Logged once, so a shipped build can prove the webview actually loaded.
    /// A release binary built without the `custom-protocol` feature points at
    /// devUrl and renders "localhost failed to connect" instead - this line is
    /// how you tell that apart from a backend problem.
    frontend_seen: AtomicBool,
    last_move_saved: Mutex<i64>,
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

/// The pill grows into a list in place; a second window would lose the
/// user's chosen position.
#[tauri::command]
fn set_shape(app: AppHandle, vertical: bool, expanded: bool) -> Result<(), String> {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return Ok(());
    };
    let (w, h) = match (vertical, expanded) {
        (_, true) => EXPANDED,
        (true, false) => PILL_V,
        (false, false) => PILL_H,
    };
    window
        .set_size(LogicalSize::new(w, h))
        .map_err(|e| e.to_string())
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

pub fn run_ui(test_notify: bool) {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            is_muted,
            set_muted,
            set_shape,
            quit_app,
            focus_session
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
                    let _ = if visible { window.hide() } else { window.show() };
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
    let (tx, rx) = std::sync::mpsc::channel::<Snapshot>();
    bridge::spawn_source(tx, 1_500);

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
            // Rule 9: if the user is already looking at the pill, the row
            // highlight is enough - a toast on top of it is noise.
            let focused = app
                .get_webview_window(WINDOW)
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false);
            if focused {
                continue;
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

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct Placement {
        pub x: i32,
        pub y: i32,
    }

    fn file() -> std::path::PathBuf {
        PathResolver::detect().ui_state_dir().join("window.json")
    }

    pub fn load() -> Option<Placement> {
        let raw = std::fs::read_to_string(file()).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save(position: PhysicalPosition<i32>) {
        let path = file();
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
}
