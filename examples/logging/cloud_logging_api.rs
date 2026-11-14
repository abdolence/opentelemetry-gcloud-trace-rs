//! Writes log entries through the Cloud Logging API, for hosts with no logging agent to collect
//! stdout: a laptop, a VM without the Ops Agent, a batch job. Entries are queued and written in
//! batches by a background task, so the handle the builder returns must be shut down to flush
//! them.
//!
//! The layer is registered after the OpenTelemetry layer, so each entry carries the trace and
//! span it was logged in.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example logging-cloud-logging-api --features logs-api,otlp
//! ```

use gcloud_observability::ids::LogId;
use gcloud_observability::logs::{GcpCloudLoggingApiConfig, GcpCloudLoggingLayerBuilder};
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use tracing::{info, info_span, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;

    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_service_name("logging-cloud-logging-api-example")
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);

    // ANCHOR: layer
    let (log_layer, log_handle) = GcpCloudLoggingLayerBuilder::new(google_project_id)
        .with_cloud_logging_api(GcpCloudLoggingApiConfig::new(LogId::new(
            "gcloud-observability-example",
        )?))
        .build_async()
        .await?;

    // The OpenTelemetry layer goes before the logging layer: the logging layer reads the span
    // context the OpenTelemetry layer attaches, and sees only what is registered ahead of it.
    let subscriber = Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(log_layer);
    tracing::subscriber::set_global_default(subscriber)?;
    // ANCHOR_END: layer

    info_span!("handle_request").in_scope(|| {
        info!(
            "labels.tenant" = "acme",
            "labels.region" = "europe-west1",
            "http_request.request_method" = "GET",
            "http_request.request_url" = "https://example.test/orders",
            "http_request.status" = 200,
            "http_request.response_size" = "1024",
            "http_request.latency" = "250ms",
            "http_request.remote_ip" = "10.0.0.1",
            "Serving a request through the Cloud Logging API sink."
        );

        warn!(
            "labels.tenant" = "acme",
            retries = 2,
            "Downstream call was retried."
        );
    });

    // Writes the entries still queued; without it they are lost.
    log_handle.shutdown().await;
    tracer_provider.shutdown()?;

    Ok(())
}
