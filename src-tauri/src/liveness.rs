//! Liveness for harness-reported pids.
//!
//! ~/.claude/sessions holds one file per CLI process and nothing ever cleans
//! them up: on this machine 23 files, 2 live, with dead ones frozen in
//! `status:"busy"` or `waitingFor:"permission prompt"` for months. Showing
//! those - or worse, diffing them - is the single largest correctness risk in
//! the app, so dead entries are dropped before they ever reach the differ.
//!
//! The check is `procStart` (the kernel's starttime for that pid) against
//! /proc/<pid>/stat field 22. Comparing pid alone is not enough: pids are
//! recycled. Matching on process *name* is wrong too - a live Claude Code
//! process has comm "2.1.259", not "claude".

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Dead,
    /// Cannot be determined on this host (e.g. a Linux pid seen from Windows,
    /// which lives in a different pid namespace entirely).
    Unknown,
}

#[cfg(target_os = "linux")]
pub fn check(pid: i64, proc_start: &str) -> Liveness {
    match read_start_time(pid) {
        Some(actual) if actual == proc_start.trim() => Liveness::Alive,
        Some(_) => Liveness::Dead,
        None => Liveness::Dead,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn check(_pid: i64, _proc_start: &str) -> Liveness {
    Liveness::Unknown
}

/// Field 22 of /proc/<pid>/stat. The comm field (2) is wrapped in parens and
/// may itself contain spaces and parens, so split after the LAST ')'.
#[cfg(target_os = "linux")]
pub fn read_start_time(pid: i64) -> Option<String> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_start_time(&raw)
}

pub fn parse_start_time(raw: &str) -> Option<String> {
    let close = raw.rfind(')')?;
    let rest = raw.get(close + 1..)?;
    // rest starts at field 3 (state), so starttime is the 20th field here.
    rest.split_whitespace().nth(19).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_starttime_field() {
        let raw = "1028 (claude) S 1 1028 1028 0 -1 4194304 1 2 3 4 5 6 7 8 20 0 1 0 1519205 123 456";
        assert_eq!(parse_start_time(raw).as_deref(), Some("1519205"));
    }

    #[test]
    fn survives_comm_containing_spaces_and_parens() {
        let raw = "42 (weird (name) here) S 1 42 42 0 -1 0 1 2 3 4 5 6 7 8 20 0 1 0 99887 1 2";
        assert_eq!(parse_start_time(raw).as_deref(), Some("99887"));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_start_time("no parens here"), None);
    }
}
