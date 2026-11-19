use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Keeps the log writer alive (stored in Tauri state).
pub struct LogGuard(#[allow(dead_code)] Option<WorkerGuard>);

/// Logs to stderr and to `<app_log_dir>/framenapper.log`. The level can be set via `RUST_LOG`.
pub fn init(log_dir: &Path) -> LogGuard {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,framenapper=debug"));
    let (file_layer, guard) = match std::fs::create_dir_all(log_dir) {
        Ok(()) => {
            let (writer, guard) =
                tracing_appender::non_blocking(tracing_appender::rolling::daily(log_dir, "framenapper.log"));
            (Some(fmt::layer().with_ansi(false).with_writer(writer)), Some(guard))
        }
        Err(_) => (None, None),
    };
    let _ = tracing_subscriber::registry().with(filter).with(fmt::layer()).with(file_layer).try_init();
    LogGuard(guard)
}
