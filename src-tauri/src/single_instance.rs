//! One UI process at a time.
//!
//! Two copies of the pill would fight over the same SQLite store, the same
//! window placement file and the same notification stream, and the user would
//! have no way to tell which one is the real monitor. So a second launch hands
//! the window back to the first and exits.
//!
//! `tauri-plugin-single-instance` does this properly, and it is what we use
//! wherever its transport actually works: a named mutex on Windows, a unix
//! socket in /tmp on macOS, and a D-Bus name on Linux. Its Linux path unwraps
//! both the session connection and the name request, so on a host with no
//! session bus - a stock WSL, which this app is expected to run on - it
//! panics inside plugin setup and takes the window down with it. There we fall
//! back to a pid lock file, which is a weaker guarantee (it cannot hand the
//! window over, so the second launch just exits) but still never runs two
//! monitors.

use std::io::Write;
use std::path::{Path, PathBuf};

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, RunEvent, Runtime, Wry};

use crate::paths::PathResolver;

/// Registered first, so a second instance is turned away before any other
/// plugin or the window itself gets a chance to come up.
pub fn plugin() -> TauriPlugin<Wry> {
    if session_bus_available() {
        tauri_plugin_single_instance::init(surface)
    } else {
        lock_file_plugin()
    }
}

/// The second launch's job is to make the monitor the user already has
/// visible again, not to open a second copy of it. The window is otherwise
/// reachable only through the tray, so a silent exit here would look like a
/// double-click that did nothing.
fn surface<R: Runtime>(app: &AppHandle<R>, _args: Vec<String>, _cwd: String) {
    if let Some(window) = app.get_webview_window(crate::WINDOW) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Non-Linux transports do not depend on a session bus, so they are always
/// available.
#[cfg(not(target_os = "linux"))]
fn session_bus_available() -> bool {
    true
}

/// Probe by connecting rather than by checking `DBUS_SESSION_BUS_ADDRESS`:
/// an address that is set but points at a dead socket fails here, and that is
/// exactly the case the fallback exists for.
#[cfg(target_os = "linux")]
fn session_bus_available() -> bool {
    zbus::blocking::Connection::session().is_ok()
}

fn lock_file_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("single-instance")
        .setup(|app, _api| {
            let Some(path) = claim() else {
                // Same courtesy the real plugin extends: let the app unwind
                // its own state before leaving, rather than tearing down from
                // inside its own setup.
                app.cleanup_before_exit();
                std::process::exit(0);
            };
            // The file's existence is the lock, so the state we manage is just
            // the path to unlink on the way out.
            app.manage(path);
            Ok(())
        })
        .on_event(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(path) = app.try_state::<PathBuf>() {
                    let _ = std::fs::remove_file(&*path);
                }
            }
        })
        .build()
}

fn lock_path() -> PathBuf {
    PathResolver::detect().ui_state_dir().join("instance.lock")
}

/// Take the lock, or report that somebody else holds it.
///
/// Two attempts, because `create_new` is the only atomic part: the second one
/// exists to clear a lock left behind by a crash and try again.
fn claim() -> Option<PathBuf> {
    let path = lock_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    for attempt in 0..2 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                // Best effort: a lock we cannot write a pid into is still a
                // lock, it just cannot be told apart from a stale one.
                let _ = file.write_all(std::process::id().to_string().as_bytes());
                return Some(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if attempt == 1 || holder_alive(&path) {
                    return None;
                }
                let _ = std::fs::remove_file(&path);
            }
            Err(err) => {
                tracing::warn!(%err, "could not create the single-instance lock; \
                    a second copy of the app may start alongside this one");
                return None;
            }
        }
    }
    None
}

/// Whether the process named in the lock file is still running.
#[cfg(target_os = "linux")]
fn holder_alive(path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return true;
    };
    let Ok(pid) = contents.trim().parse::<u32>() else {
        return true;
    };
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Unreachable: the lock file is only used where there is no session bus, and
/// the busless-host case is Linux. Answering "alive" keeps the failure mode
/// the safe one if that ever stops being true.
#[cfg(not(target_os = "linux"))]
fn holder_alive(_path: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_at(path: &Path) -> Option<PathBuf> {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(_) => Some(path.to_path_buf()),
            Err(_) => None,
        }
    }

    #[test]
    fn create_new_refuses_a_lock_already_held() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instance.lock");

        assert!(lock_at(&path).is_some(), "first claim wins");
        assert!(lock_at(&path).is_none(), "second claim is refused");
    }

    #[test]
    fn holder_alive_sees_our_own_pid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instance.lock");
        lock_at(&path).unwrap();
        std::fs::write(&path, std::process::id().to_string()).unwrap();

        assert!(holder_alive(&path), "this process is obviously running");
    }

    #[test]
    fn holder_alive_rejects_a_lock_we_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instance.lock");

        std::fs::write(&path, "not a pid").unwrap();
        assert!(holder_alive(&path), "unparseable means assume the worst");

        std::fs::remove_file(&path).unwrap();
        assert!(holder_alive(&path), "missing means assume the worst");
    }
}
