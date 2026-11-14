//! Reports a panic to Cloud Error Reporting through the Cloud Logging API sink. The panic hook
//! emits one error event per panic and then runs the previous hook, so the default message on
//! stderr is kept.
//!
//! The backtrace is empty unless `RUST_BACKTRACE=1` or `RUST_LIB_BACKTRACE=1` is set.
//!
//! Run with:
//!
//! ```sh
//! RUST_BACKTRACE=1 PROJECT_ID=your-project cargo run --example errors-panic-hook --features logs-api
//! ```

use gcloud_observability::logs::{
    install_panic_hook, GcpCloudLoggingApiConfig, GcpCloudLoggingLayerBuilder, ServiceContext,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

fn adjust_inventory(stock: u32, sold: u32) -> u32 {
    assert!(
        sold <= stock,
        "inventory went negative: {stock} in stock, {sold} sold"
    );
    stock - sold
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let project_id = config_env_var("PROJECT_ID")?;

    let (log_layer, log_handle) = GcpCloudLoggingLayerBuilder::new(project_id)
        .with_cloud_logging_api(GcpCloudLoggingApiConfig::new(
            "gcloud-observability-example",
        ))
        .with_error_reporting(
            ServiceContext::new("gcloud-observability-panic-hook-example")
                .with_version(env!("CARGO_PKG_VERSION")),
        )
        .build_async()
        .await?;
    // The filter keeps the Cloud Logging client's own debug events out of the log it writes.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(log_layer);
    tracing::subscriber::set_global_default(subscriber)?;

    install_panic_hook();

    let outcome = std::panic::catch_unwind(|| adjust_inventory(1, std::hint::black_box(2)));
    tracing::info!(panicked = outcome.is_err(), "the panic was caught");

    log_handle.shutdown().await;

    Ok(())
}
