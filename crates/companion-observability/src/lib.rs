pub mod audit;
pub mod metrics;

pub use audit::*;
pub use metrics::*;

use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize the observability stack.
///
/// Sets up `tracing` with structured JSON output and env-filter support.
/// Use `RUST_LOG=companion=debug` for development logging.
pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("companion=info,tower_http=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_level(true)
                .with_file(true)
                .with_line_number(true),
        )
        .init();
}

/// Initialize with JSON output (for production).
pub fn init_json() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("companion=info,tower_http=info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().json())
        .init();
}
