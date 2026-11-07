use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initializes the tracing subscriber.
/// Returns a `WorkerGuard` that MUST be kept alive in `main()` to ensure logs are flushed.
pub fn init(suffix: &str) -> WorkerGuard {
    let file_appender = tracing_appender::rolling::never("/tmp", format!("just_sync-{suffix}.log"));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false) // No colors in file
                .with_target(true)
                .with_thread_ids(true),
        )
        .init();

    tracing::info!("--- JustSync Tracing Started ({}) ---", suffix);
    guard
}
