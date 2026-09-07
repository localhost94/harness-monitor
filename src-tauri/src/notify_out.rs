//! Desktop notification delivery.
//!
//! Windows: tauri-plugin-notification -> native toast.
//! Linux: there is no org.freedesktop.Notifications daemon on a stock WSL box
//! (no dunst/mako/notify-send; only the GTK xdg-portal), and notify-rust talks
//! to that name directly, so the call succeeds and nothing appears. We say so
//! once at startup instead of pretending. Portal delivery via zbus is Phase 4.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::differ::Outgoing;

pub fn startup_check() {
    if !cfg!(target_os = "linux") || linux_daemon_present() {
        return;
    }
    if portal_present() {
        tracing::info!(
            "no notification daemon; falling back to the xdg desktop portal. Verify it renders \
             here with --test-notify - WSLg does not always surface portal notifications"
        );
    } else {
        tracing::warn!(
            "no notification daemon and no desktop portal - toasts will not appear on this host; \
             sessions are still tracked in the window"
        );
    }
}

#[cfg(target_os = "linux")]
fn portal_present() -> bool {
    let probe = || -> zbus::Result<bool> {
        let connection = zbus::blocking::Connection::session()?;
        let has_owner: bool = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &("org.freedesktop.portal.Desktop"),
            )?
            .body()
            .deserialize()?;
        Ok(has_owner)
    };
    probe().unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn portal_present() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn linux_daemon_present() -> bool {
    std::path::Path::new("/usr/share/dbus-1/services/org.freedesktop.Notifications.service").exists()
        || which("notify-send")
}

#[cfg(not(target_os = "linux"))]
fn linux_daemon_present() -> bool {
    true
}

#[cfg(target_os = "linux")]
fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file())
        })
        .unwrap_or(false)
}

/// Linux has no notification daemon on a stock WSL, so try the xdg desktop
/// portal (which does exist there) before the plugin. Returns false if the
/// portal is unavailable or rejects the call, so the caller can fall back.
#[cfg(target_os = "linux")]
fn portal_notify(title: &str, body: &str) -> bool {
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    let attempt = || -> zbus::Result<()> {
        let connection = zbus::blocking::Connection::session()?;
        let id = format!("harness-monitor-{}", crate::model::now_ms());
        let mut payload: HashMap<&str, Value> = HashMap::new();
        payload.insert("title", Value::from(title));
        payload.insert("body", Value::from(body));
        payload.insert("priority", Value::from("normal"));
        connection.call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Notification"),
            "AddNotification",
            &(id.as_str(), payload),
        )?;
        Ok(())
    };

    match attempt() {
        Ok(()) => true,
        Err(err) => {
            tracing::debug!(%err, "portal notification unavailable");
            false
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn portal_notify(_title: &str, _body: &str) -> bool {
    false
}

pub fn send(app: &AppHandle, title: &str, body: &str) {
    if portal_notify(title, body) {
        return;
    }
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(%err, "notification delivery failed");
    }
}

pub fn deliver(app: &AppHandle, out: &Outgoing) {
    let (title, body) = match out {
        Outgoing::One { title, body, .. } => (title.clone(), body.clone()),
        Outgoing::Coalesced { count } => (
            format!("{count} more sessions need attention"),
            "Open HarnessMonitor for details".to_string(),
        ),
    };

    send(app, &title, &body);
}
