//! Reports errors to Cloud Error Reporting through the Cloud Logging API sink. Every event at
//! ERROR severity or above becomes an error event with the service, the code location, the
//! error's `source()` chain and a backtrace, and Error Reporting groups the repeats.
//!
//! The backtrace is empty unless `RUST_BACKTRACE=1` or `RUST_LIB_BACKTRACE=1` is set. The
//! error must be passed to the event as a `&dyn Error` for its sources to be kept.
//!
//! Run with:
//!
//! ```sh
//! RUST_BACKTRACE=1 PROJECT_ID=your-project cargo run --example errors-error-reporting --features logs-api
//! ```

use gcloud_observability::ids::LogId;
use gcloud_observability::logs::{
    GcpCloudLoggingApiConfig, GcpCloudLoggingLayerBuilder, ServiceContext,
};
use std::error::Error;
use std::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

#[derive(Debug)]
struct CardExpired;

impl fmt::Display for CardExpired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("card expired")
    }
}

impl Error for CardExpired {}

#[derive(Debug)]
struct PaymentDeclined(CardExpired);

impl fmt::Display for PaymentDeclined {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("payment declined")
    }
}

impl Error for PaymentDeclined {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

fn charge(order_id: &str) {
    let failure = PaymentDeclined(CardExpired);
    tracing::error!(
        error = &failure as &dyn Error,
        order_id,
        "http_request.request_method" = "POST",
        "http_request.request_url" = "https://example.test/orders/checkout",
        "http_request.status" = 500,
        "checkout failed"
    );
}

fn reserve_stock(sku: &str) {
    tracing::error!(sku, "stock reservation failed");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;

    // ANCHOR: layer
    let (log_layer, log_handle) = GcpCloudLoggingLayerBuilder::new(google_project_id)
        .with_cloud_logging_api(GcpCloudLoggingApiConfig::new(LogId::new(
            "gcloud-observability-example",
        )?))
        .with_error_reporting(
            ServiceContext::new("gcloud-observability-error-reporting-example")
                .with_version(env!("CARGO_PKG_VERSION")),
        )
        .build_async()
        .await?;

    // The filter keeps the Cloud Logging client's own debug events out of the log it writes.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(log_layer);
    // ANCHOR_END: layer
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("not an error, so left untouched");
        for order_id in ["order-1", "order-2", "order-3"] {
            charge(order_id);
        }
        reserve_stock("sku-42");
    });

    log_handle.shutdown().await;

    Ok(())
}
