//! The shortest useful setup: `tracing` spans exported to Cloud Trace through OTLP, and every
//! event written to stdout as a Cloud Logging JSON line carrying the trace and span it
//! happened in.
//!
//! The JSON lines are what the Cloud Logging agent of GKE, Cloud Run and GCE collects from
//! stdout. Run on a laptop they only print; the traces reach the Telemetry API either way.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example quickstart --features logs,otlp
//! ```

use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use tracing::{info, info_span};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;

    // ANCHOR: setup
    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_service_name("quickstart-example")
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);

    let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id).build()?;

    // The OpenTelemetry layer goes before the logging layer: the logging layer reads the span
    // context the OpenTelemetry layer attaches, and sees only what is registered ahead of it.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(log_layer);
    tracing::subscriber::set_global_default(subscriber)?;
    // ANCHOR_END: setup

    info_span!("handle_request", order_id = "order-42").in_scope(|| {
        info!("labels.tenant" = "acme", "Serving a request.");
        info_span!("load_order").in_scope(|| info!(rows = 3, "Loaded the order rows."));
    });

    // Flushes the spans still queued; blocks, so it needs the multi-threaded runtime
    // `#[tokio::main]` starts.
    tracer_provider.shutdown()?;

    Ok(())
}
