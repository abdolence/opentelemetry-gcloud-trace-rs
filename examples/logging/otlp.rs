//! Sends log entries to the Google Cloud Telemetry API through OTLP, from the same builder that
//! exports the spans, so one set of credentials and one endpoint serve both. Each entry is
//! correlated to the span that emitted it.
//!
//! The logger provider is owned by the caller, which flushes it by shutting it down.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example logging-otlp --features logs-otlp
//! ```

use gcloud_observability::ids::LogId;
use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use tracing::{info, info_span, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;

    // ANCHOR: setup
    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_service_name("logging-otlp-example")
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);
    let logger_provider = otlp.create_logger_provider().await?;

    let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id)
        .with_otel_logger_provider(
            &logger_provider,
            LogId::new("gcloud-observability-example")?,
        )
        .build()?;

    // The OpenTelemetry layer goes before the logging layer: the logging layer reads the span
    // context the OpenTelemetry layer attaches, and sees only what is registered ahead of it.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(log_layer);
    tracing::subscriber::set_global_default(subscriber)?;
    // ANCHOR_END: setup

    info_span!("handle_request").in_scope(|| {
        info!(
            "labels.tenant" = "acme",
            "http_request.request_method" = "POST",
            "http_request.request_url" = "https://example.test/orders/order-42/checkout",
            "http_request.status" = 200,
            "http_request.latency" = "0.125s",
            "Checkout request received."
        );
        warn!(retries = 2, "Downstream call was retried.");
    });

    // Both block until the batches are exported, so they need the multi-threaded runtime
    // `#[tokio::main]` starts.
    tracer_provider.shutdown()?;
    logger_provider.shutdown()?;

    Ok(())
}
