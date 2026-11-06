use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Hält den Log-Writer am Leben (wird im Tauri-State verwahrt).
pub struct LogGuard(#[allow(dead_code)] Option<WorkerGuard>);

/// Loggt nach stderr und in `<app_log_dir>/framenab.log`. Level über `RUST_LOG` steuerbar.
pub fn init(log_dir: &Path) -> LogGuard {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,framenab=debug"));
    let (file_layer, guard) = match std::fs::create_dir_all(log_dir) {
        Ok(()) => {
            let (writer, guard) =
                tracing_appender::non_blocking(tracing_appender::rolling::daily(log_dir, "framenab.log"));
            (Some(fmt::layer().with_ansi(false).with_writer(writer)), Some(guard))
        }
        Err(_) => (None, None),
    };
    let _ = tracing_subscriber::registry().with(filter).with(fmt::layer()).with(file_layer).try_init();
    LogGuard(guard)
}
