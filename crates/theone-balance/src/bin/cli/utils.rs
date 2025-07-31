use tracing_subscriber::fmt;

/// Initializes tracing for the CLI, separate from the worker's tracing.
pub fn init_tracing() {
    fmt()
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned()))
        .init();
}
