//! `--agent` role: no GUI, one NDJSON snapshot per line on stdout.
//!
//! The Windows UI process runs this inside WSL because path translation alone
//! is not enough there - a Linux pid means nothing to a Windows process (2 of
//! 23 session files are live, and only /proc can say which), and opencode's
//! WAL database cannot be opened safely over a 9p share.

use std::io::Write;
use std::time::Duration;

use crate::scanner::Scanner;

pub fn run(interval_ms: u64) -> anyhow::Result<()> {
    let mut scanner = Scanner::new();
    let stdout = std::io::stdout();
    loop {
        let snapshot = scanner.tick();
        let line = serde_json::to_string(&snapshot)?;
        let mut handle = stdout.lock();
        // A broken pipe means the UI went away; exit quietly rather than spin.
        if writeln!(handle, "{line}").is_err() || handle.flush().is_err() {
            return Ok(());
        }
        drop(handle);
        std::thread::sleep(Duration::from_millis(interval_ms));
    }
}
