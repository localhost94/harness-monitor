// Windows release builds must not pop a console window behind the pill.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "harness-monitor",
    about = "Floating monitor for local AI coding agents"
)]
struct Cli {
    /// Headless snapshot producer: writes NDJSON to stdout. The Windows UI
    /// spawns this same binary inside WSL.
    #[arg(long)]
    agent: bool,

    #[arg(long, default_value_t = 1500)]
    interval_ms: u64,

    /// Cap on finished rows per harness. Passed in by the Windows UI process,
    /// which owns the settings file this one cannot read.
    #[arg(long, default_value_t = 100)]
    max_ended: usize,

    /// Comma-separated harness ids to scan. Empty means all of them, so a
    /// hand-run `--agent` with no flags behaves exactly as it did before.
    #[arg(long, value_name = "IDS", default_value = "")]
    harnesses: String,

    /// Focus the terminal pane hosting a session, then exit. Used by the
    /// Windows build, which cannot reach herdr directly.
    #[arg(long, value_name = "PANE")]
    focus: Option<String>,

    /// Reopen a finished session's conversation in a terminal, then exit:
    /// `--run-again <HARNESS> <SESSION_ID> <CWD>`.
    ///
    /// The options are arguments rather than a file this process reads, because
    /// on Windows the caller is a UI process on the other side of WSL and the
    /// two do not share a settings directory. Same reason `--focus` re-invokes
    /// us instead of the UI hunting for herdr on PATH.
    #[arg(long, num_args = 3, value_names = ["HARNESS", "SESSION_ID", "CWD"])]
    run_again: Option<Vec<String>>,

    /// Where to open it: `auto` (herdr if present), `herdr`, or `terminal`.
    #[arg(long, default_value = "auto")]
    rerun_target: String,

    /// Bring the new pane to the front. `--no-focus` is the negating form, so
    /// the flag defaults to on the way herdr's own `--focus` does not.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    rerun_focus: bool,

    /// How long to wait for the agent to become interactive.
    #[arg(long, default_value_t = 30_000)]
    rerun_timeout_ms: u64,

    /// Send one test notification and exit. Use it to check whether toasts
    /// actually appear on this host before trusting them.
    #[arg(long)]
    test_notify: bool,
}

fn main() {
    let cli = Cli::parse();
    init_logging();

    if let Some(target) = cli.focus {
        if let Err(err) = harness_monitor_lib::herdr::focus(&target) {
            eprintln!("focus failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    if let Some(args) = cli.run_again.clone() {
        match run_again(&args, &cli) {
            Ok(()) => return,
            Err(err) => {
                // stderr, not only tracing: this process has no console on a
                // Windows release build, and the UI reads the exit code. The
                // log file is where a human finds the detail.
                eprintln!("run-again failed: {err}");
                tracing::error!(%err, "run-again failed");
                std::process::exit(1);
            }
        }
    }

    if cli.agent {
        if let Err(err) =
            harness_monitor_lib::agent::run(cli.interval_ms, cli.max_ended, &cli.harnesses)
        {
            tracing::error!(%err, "agent exited");
            std::process::exit(1);
        }
        return;
    }

    harness_monitor_lib::run_ui(cli.test_notify);
}

fn run_again(args: &[String], cli: &Cli) -> Result<(), String> {
    let [harness, session_id, cwd] = args else {
        return Err("--run-again needs a harness, a session id and a directory".into());
    };
    let harness: harness_monitor_lib::model::HarnessId =
        serde_json::from_value(harness_id_value(harness)?)
            .map_err(|e| format!("unknown harness {harness}: {e}"))?;
    let opts = harness_monitor_lib::rerun::RerunOptions {
        target: match cli.rerun_target.as_str() {
            "auto" => harness_monitor_lib::rerun::LaunchTarget::Auto,
            "herdr" => harness_monitor_lib::rerun::LaunchTarget::Herdr,
            "terminal" => harness_monitor_lib::rerun::LaunchTarget::Terminal,
            other => return Err(format!("unknown --rerun-target {other}")),
        },
        focus: cli.rerun_focus,
        timeout_ms: cli.rerun_timeout_ms,
    };
    harness_monitor_lib::rerun::run_again(harness, session_id, cwd, opts)
}

/// The serde name for a harness id, which is kebab-case on the wire
/// (`claude-code`) rather than the Rust variant (`ClaudeCode`).
fn harness_id_value(name: &str) -> Result<serde_json::Value, String> {
    match name {
        "claude-code" | "open-code" | "codex" | "gemini" | "antigravity" => {
            Ok(serde_json::Value::String(name.to_string()))
        }
        other => Err(other.to_string()),
    }
}

/// The Windows release build has no console, so HM_LOG_FILE is the only way to
/// see why (say) the WSL agent failed to start. stdout is never used for logs -
/// it is the NDJSON channel.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "harness_monitor_lib=info".into());

    let log_file = std::env::var_os("HM_LOG_FILE").and_then(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()
            .map(|_| std::path::PathBuf::from(path))
    });

    match log_file {
        Some(path) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(move || {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map(WriterSink::File)
                    .unwrap_or(WriterSink::Stderr)
            })
            .init(),
        None => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init(),
    }
}

enum WriterSink {
    File(std::fs::File),
    Stderr,
}

impl std::io::Write for WriterSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            WriterSink::File(f) => f.write(buf),
            WriterSink::Stderr => std::io::stderr().write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            WriterSink::File(f) => f.flush(),
            WriterSink::Stderr => std::io::stderr().flush(),
        }
    }
}
