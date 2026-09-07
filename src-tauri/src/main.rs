// Windows release builds must not pop a console window behind the pill.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "harness-monitor", about = "Floating monitor for local AI coding agents")]
struct Cli {
    /// Headless snapshot producer: writes NDJSON to stdout. The Windows UI
    /// spawns this same binary inside WSL.
    #[arg(long)]
    agent: bool,

    #[arg(long, default_value_t = 1500)]
    interval_ms: u64,

    /// Focus the terminal pane hosting a session, then exit. Used by the
    /// Windows build, which cannot reach herdr directly.
    #[arg(long, value_name = "PANE")]
    focus: Option<String>,

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

    if cli.agent {
        if let Err(err) = harness_monitor_lib::agent::run(cli.interval_ms) {
            tracing::error!(%err, "agent exited");
            std::process::exit(1);
        }
        return;
    }

    harness_monitor_lib::run_ui(cli.test_notify);
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
