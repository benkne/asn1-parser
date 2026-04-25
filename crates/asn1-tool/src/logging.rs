//! Tracing subscriber + panic hook.
//!
//! Logs go to `asn1-tool.log` inside the data directory, rotated daily. A
//! small number of past logs are kept so a crash report can reference the
//! last run. The panic hook writes a one-shot `crash-<timestamp>.log`
//! containing the panic message and backtrace — independent of the tracing
//! subscriber because that may itself be mid-shutdown when panic fires.

use std::panic;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Install a rolling-file tracing subscriber. The returned guard flushes the
/// background writer on drop — keep it alive until `main` returns.
pub(crate) fn init(data_dir: &Path) -> WorkerGuard {
    let file_appender = rolling::daily(data_dir, "asn1-tool.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_env("ASN1_TOOL_LOG")
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("static filter is valid");

    let file_layer =
        fmt::layer().with_writer(non_blocking).with_ansi(false).with_target(false).with_level(true);

    // Debug builds also mirror to stderr so `cargo run` shows logs live.
    // Release builds on Windows use `windows_subsystem = "windows"`, so
    // stderr is detached anyway — the file is the only reachable sink.
    #[cfg(debug_assertions)]
    {
        let stderr_layer =
            fmt::layer().with_writer(std::io::stderr).with_target(false).with_level(true);
        tracing_subscriber::registry().with(filter).with(file_layer).with(stderr_layer).init();
    }
    #[cfg(not(debug_assertions))]
    {
        tracing_subscriber::registry().with(filter).with(file_layer).init();
    }

    guard
}

/// Install a panic hook that appends a crash log next to the rolling trace
/// file and preserves the default hook's terminal output. Safe to call
/// exactly once; subsequent panics would still fire through the default hook.
pub(crate) fn install_panic_hook(data_dir: &Path) {
    let dir = data_dir.to_path_buf();
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Best-effort crash report; never panic from inside the hook.
        let _ = write_crash_log(&dir, info);
        default_hook(info);
    }));
}

fn write_crash_log(data_dir: &Path, info: &panic::PanicHookInfo<'_>) -> std::io::Result<()> {
    use std::io::Write;

    let path = crash_log_path(data_dir);
    let mut f = std::fs::File::create(&path)?;
    writeln!(f, "asn1-tool crash report")?;
    writeln!(f, "version:   {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(f, "timestamp: {}", timestamp())?;
    writeln!(f, "os:        {}", std::env::consts::OS)?;
    writeln!(f, "arch:      {}", std::env::consts::ARCH)?;
    writeln!(f)?;
    writeln!(f, "panic: {info}")?;
    writeln!(f)?;
    writeln!(f, "backtrace:")?;
    writeln!(f, "{}", std::backtrace::Backtrace::force_capture())?;
    Ok(())
}

fn crash_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join(format!("crash-{}.log", timestamp_for_filename()))
}

fn timestamp() -> String {
    // Deliberately avoid a `chrono`/`time` dep for a single timestamp —
    // system time as unix seconds is enough to correlate with the trace log.
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("unix={}s", d.as_secs()),
        Err(_) => String::from("unknown"),
    }
}

fn timestamp_for_filename() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| format!("pid{}", std::process::id()))
}
