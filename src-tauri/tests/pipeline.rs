//! End-to-end over the real filesystem: session files in, notifications out.
//!
//! Covers the three things unit tests cannot: that a dead pid's file cannot be
//! diffed, that a live session finishing a turn produces exactly one toast,
//! and that the ghosts a real ~/.claude/sessions accumulates are still
//! reachable - just in a list that cannot notify.

use std::path::Path;

use harness_monitor_lib::differ::{Differ, DifferConfig, NotifyKind, Outgoing};
use harness_monitor_lib::liveness::Liveness;
use harness_monitor_lib::model::{now_ms, AgentSession, HarnessId, SessionState};
use harness_monitor_lib::paths::PathResolver;
use harness_monitor_lib::scanner::Scanner;

/// The fixture home only controls `~/.claude`; the other adapters resolve their
/// own roots and will happily find this machine's real data. Narrow every
/// assertion to the harness under test.
fn of_harness(sessions: &[AgentSession], harness: HarnessId) -> Vec<&AgentSession> {
    sessions
        .iter()
        .filter(|s| s.key.harness == harness)
        .collect()
}

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
fn own_proc_start() -> String {
    let raw = std::fs::read_to_string(format!("/proc/{}/stat", std::process::id())).unwrap();
    harness_monitor_lib::liveness::parse_start_time(&raw).unwrap()
}

fn temp_home(tag: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("harness-monitor-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".claude/sessions")).unwrap();
    dir
}

#[test]
fn ghosts_are_listed_but_cannot_notify() {
    let home = temp_home("pipeline");
    let sessions = home.join(".claude/sessions");
    let live_pid = std::process::id();
    let live_start = own_proc_start();

    // One live session mid-turn, plus two ghosts of the kind that accumulate
    // for months in a real ~/.claude/sessions (56 of 61 files on this box).
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
    assert_eq!(
        of_harness(&first.sessions, HarnessId::ClaudeCode).len(),
        1,
        "only the live pid survives liveness"
    );
    assert_eq!(first.sessions[0].key.pid, live_pid as i64);
    assert_eq!(
        differ.ingest(&first, now_ms()),
        vec![],
        "baseline is silent"
    );

    // The ghosts are not gone, they are filed: the same two files, minus the
    // one that is alive. This is the "what did I run" list, and it is
    // reachable without ever entering the differ.
    let ended = of_harness(&first.ended, HarnessId::ClaudeCode);
    assert_eq!(ended.len(), 2, "both ghosts are listed as ended");
    for session in ended {
        assert_eq!(session.state, SessionState::Ended);
        assert_ne!(session.key.pid, live_pid as i64);
    }

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

    // The ghosts, sitting in the ended list across the transition, add nothing
    // to that toast. They carry timestamps older than the live one, so a naive
    // merge would have fired two extra completions here.
    assert_eq!(of_harness(&second.ended, HarnessId::ClaudeCode).len(), 2);

    // Re-reading the unchanged file must stay quiet.
    let third = scanner.tick();
    assert_eq!(differ.ingest(&third, now_ms()), vec![]);

    // The whole session directory being wiped is not "everything finished".
    std::fs::remove_dir_all(&sessions).unwrap();
    let fourth = scanner.tick();
    assert_eq!(of_harness(&fourth.sessions, HarnessId::ClaudeCode).len(), 0);
    assert_eq!(differ.ingest(&fourth, now_ms()), vec![]);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_session_with_no_proc_start_is_listed_not_shown_as_live() {
    // macOS takes this path today: no procfs, so nothing can be verified, and
    // the session must not be claimed as live just because it cannot be
    // disproved.
    let home = temp_home("unverifiable");
    let sessions = home.join(".claude/sessions");
    std::fs::write(
        sessions.join("4242.json"),
        r#"{"pid":4242,"sessionId":"s","cwd":"/tmp","name":"no-procstart",
            "status":"busy","statusUpdatedAt":1000}"#,
    )
    .unwrap();

    let mut scanner = Scanner::with_paths(PathResolver::for_home(home.clone()));
    let snapshot = scanner.tick();
    assert!(
        of_harness(&snapshot.sessions, HarnessId::ClaudeCode).is_empty(),
        "an unverifiable pid is not live"
    );
    let ended = of_harness(&snapshot.ended, HarnessId::ClaudeCode);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].liveness, Liveness::Unknown);
    assert_eq!(ended[0].state, SessionState::Ended);

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
