//! End-to-end over the real filesystem: session files in, notifications out.
//!
//! Covers the three things unit tests cannot: that a dead pid's file is dropped
//! before it can be diffed, that a live session finishing a turn produces
//! exactly one toast, and that two watched accounts never merge into one.

use std::path::Path;

use harness_monitor_lib::accounts::Account;
use harness_monitor_lib::differ::{Differ, DifferConfig, NotifyKind, Outgoing};
use harness_monitor_lib::model::now_ms;
use harness_monitor_lib::paths::PathResolver;
use harness_monitor_lib::scanner::Scanner;

fn write_session(dir: &Path, pid: u32, proc_start: &str, status: &str, updated: i64) {
    let body = format!(
        r#"{{"pid":{pid},"sessionId":"90ba7df5-9c0c-4996-b592-6b86ae15339c",
            "cwd":"/home/you/code/example-project","startedAt":1786422487968,
            "procStart":"{proc_start}","version":"2.1.261","kind":"interactive",
            "name":"fixture-{pid}","status":"{status}","statusUpdatedAt":{updated},
            "updatedAt":{updated},"pidDomain":"linux:test:pid:[1]"}}"#
    );
    std::fs::write(dir.join(format!("{pid}.json")), body).unwrap();
}

/// starttime of a process we know is alive: this test process.
#[cfg(target_os = "linux")]
fn own_proc_start() -> String {
    let raw = std::fs::read_to_string(format!("/proc/{}/stat", std::process::id())).unwrap();
    harness_monitor_lib::liveness::parse_start_time(&raw).unwrap()
}

/// Off Linux there is no procfs, so `liveness::check` answers Unknown for
/// every pid and the value here is never compared against anything.
#[cfg(not(target_os = "linux"))]
fn own_proc_start() -> String {
    "0".into()
}

fn temp_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("harness-monitor-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".claude/sessions")).unwrap();
    dir
}

/// Liveness is a /proc read; without one every ghost file survives the filter,
/// so this assertion only means anything on Linux (see docs/ARCHITECTURE.md).
#[cfg(target_os = "linux")]
#[test]
fn ghosts_are_dropped_and_a_finished_turn_fires_once() {
    let home = temp_home("pipeline");
    let sessions = home.join(".claude/sessions");
    let live_pid = std::process::id();
    let live_start = own_proc_start();

    // One live session mid-turn, plus two ghosts of the kind that accumulate
    // for months in a real ~/.claude/sessions (21 of 23 files on this box).
    write_session(&sessions, live_pid, &live_start, "busy", 1_000);
    write_session(&sessions, 999_998, "12345", "busy", 900);
    write_session(&sessions, 999_999, "12345", "waiting", 900);
    // A sibling key file must not be parsed as a session.
    std::fs::write(sessions.join("999999.abc.key"), "not json").unwrap();

    // Explicit root, not an env var: these tests run in parallel in one
    // process and would otherwise fight over HM_HOME.
    let mut scanner = Scanner::with_paths(PathResolver::for_home(home.clone()));
    let mut differ = Differ::new(DifferConfig::default());

    let first = scanner.tick();
    assert_eq!(first.sessions.len(), 1, "only the live pid survives liveness");
    assert_eq!(first.sessions[0].key.pid, live_pid as i64);
    assert_eq!(differ.ingest(&first, now_ms()), vec![], "baseline is silent");

    // Turn ends.
    write_session(&sessions, live_pid, &live_start, "idle", 2_000);
    let second = scanner.tick();
    let events = differ.ingest(&second, now_ms());
    match events.as_slice() {
        [Outgoing::One { kind, title, .. }] => {
            assert_eq!(*kind, NotifyKind::Done);
            assert!(title.contains("fixture"), "title was {title}");
        }
        other => panic!("expected exactly one done toast, got {other:?}"),
    }

    // Re-reading the unchanged file must stay quiet.
    let third = scanner.tick();
    assert_eq!(differ.ingest(&third, now_ms()), vec![]);

    // The whole session directory being wiped is not "everything finished".
    std::fs::remove_dir_all(&sessions).unwrap();
    let fourth = scanner.tick();
    assert_eq!(fourth.sessions.len(), 0);
    assert_eq!(differ.ingest(&fourth, now_ms()), vec![]);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn permission_prompt_reports_its_reason() {
    let home = temp_home("permission");
    let sessions = home.join(".claude/sessions");
    let live_pid = std::process::id();
    let live_start = own_proc_start();
    write_session(&sessions, live_pid, &live_start, "busy", 1_000);

    let mut scanner = Scanner::with_paths(PathResolver::for_home(home.clone()));
    let mut differ = Differ::new(DifferConfig::default());
    differ.ingest(&scanner.tick(), now_ms());

    let body = format!(
        r#"{{"pid":{live_pid},"sessionId":"s","cwd":"/tmp","procStart":"{live_start}",
            "name":"fixture-perm","status":"waiting","waitingFor":"permission prompt",
            "statusUpdatedAt":2000,"pidDomain":"linux:test:pid:[1]"}}"#
    );
    std::fs::write(sessions.join(format!("{live_pid}.json")), body).unwrap();

    let events = differ.ingest(&scanner.tick(), now_ms());
    match events.as_slice() {
        [Outgoing::One { kind, body, .. }] => {
            assert_eq!(*kind, NotifyKind::Attention);
            assert!(body.contains("permission prompt"), "body was {body}");
        }
        other => panic!("expected one attention toast, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&home);
}

fn write_quota(home: &Path, five_hour_pct: f64) {
    let dir = home.join(".local/state/harness-monitor");
    std::fs::create_dir_all(&dir).unwrap();
    // The shape the statusline shim writes.
    std::fs::write(
        dir.join("quota.json"),
        format!(r#"{{"at":1786422487.5,"rate_limits":{{"five_hour":{{"used_percentage":{five_hour_pct}}}}}}}"#),
    )
    .unwrap();
}

/// Two divisions, two homes, and deliberately the same pid, the same
/// `sessionId` and the same `pidDomain` in both - which is what happens when
/// one machine watches two account trees. Without the account in the identity
/// these collapse into one row and cross-fire each other's notifications.
#[test]
fn two_accounts_never_merge() {
    let marketing = temp_home("acct-marketing");
    let engineering = temp_home("acct-engineering");
    let pid = std::process::id();
    let start = own_proc_start();

    write_session(&marketing.join(".claude/sessions"), pid, &start, "busy", 1_000);
    write_session(&engineering.join(".claude/sessions"), pid, &start, "waiting", 1_000);
    write_quota(&marketing, 12.0);
    write_quota(&engineering, 88.0);

    let mut scanner = Scanner::for_accounts(vec![
        Account {
            label: "claude-rina".into(),
            division: "marketing".into(),
            home: marketing.clone(),
        },
        Account {
            label: "claude-core".into(),
            division: "engineering".into(),
            home: engineering.clone(),
        },
    ]);
    let snap = scanner.tick();

    assert_eq!(snap.accounts.len(), 2);
    assert_eq!(snap.sessions.len(), 2, "one row per account, not one merged row");
    assert_ne!(snap.sessions[0].key, snap.sessions[1].key, "identity must differ");
    assert_eq!(snap.sessions[0].key.account, "claude-rina");
    assert_eq!(snap.sessions[1].key.account, "claude-core");

    // Two subscriptions, two independent windows - never averaged, never
    // attributed to whichever account was scanned last.
    let pct = |label: &str| {
        snap.quotas
            .iter()
            .find(|q| q.account == label)
            .unwrap_or_else(|| panic!("no quota for {label}"))
            .five_hour_pct
    };
    assert_eq!(pct("claude-rina"), Some(12.0));
    assert_eq!(pct("claude-core"), Some(88.0));

    let _ = std::fs::remove_dir_all(&marketing);
    let _ = std::fs::remove_dir_all(&engineering);
}
