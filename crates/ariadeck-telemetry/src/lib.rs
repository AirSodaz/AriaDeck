//! Structured diagnostics setup for AriaDeck binaries.

use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

/// Installs the process-wide tracing subscriber.
///
/// `RUST_LOG` takes precedence over the supplied default filter. Calling this
/// function more than once is harmless, which keeps test and preview binaries
/// composable.
pub fn init(default_filter: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true),
        )
        .try_init();
}
