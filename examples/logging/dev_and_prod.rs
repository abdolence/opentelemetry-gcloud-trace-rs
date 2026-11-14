//! One application with two logging setups: a human-readable console layer for development,
//! and the Cloud Logging JSON layer for GKE, Cloud Run and GCE instances with the Cloud
//! Logging agent, which parses the JSON lines the layer writes to stdout. `APP_MODE` (default
//! `development`) picks one at startup, and exactly one of the two layers is active.
//!
//! A host with no logging agent should write through the Cloud Logging API instead, as
//! `examples/logging/cloud_logging_api.rs` does.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example logging-dev-and-prod --features logs,otlp
//! PROJECT_ID=your-project APP_MODE=production cargo run --example logging-dev-and-prod --features logs,otlp
//! ```

use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use tracing::{info, info_span, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;
    let production = std::env::var("APP_MODE").is_ok_and(|mode| mode == "production");

    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_service_name("logging-dev-and-prod-example")
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);

    // ANCHOR: layers
    // `Option<Layer>` implements `Layer`, so both sinks are built from one `match` and composed
    // unconditionally below. The inactive one is a `None` that costs nothing.
    let (console_layer, json_layer) = if production {
        (
            None,
            Some(GcpCloudLoggingLayerBuilder::new(google_project_id).build()?),
        )
    } else {
        (Some(tracing_subscriber::fmt::layer()), None)
    };

    // The OpenTelemetry layer goes before the logging layer: the logging layer reads the span
    // context the OpenTelemetry layer attaches, and sees only what is registered ahead of it.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(console_layer)
        .with(json_layer);
    tracing::subscriber::set_global_default(subscriber)?;
    // ANCHOR_END: layers

    info_span!("handle_request").in_scope(|| {
        // `labels.*` become Cloud Logging labels, `http_request.*` the entry's `httpRequest`.
        info!(
            "labels.tenant" = "acme",
            "http_request.request_method" = "GET",
            "http_request.request_url" = "https://example.test/orders",
            "http_request.status" = 200,
            "Serving a request."
        );

        warn!(retries = 2, "Downstream call was retried.");
    });

    tracer_provider.shutdown()?;

    Ok(())
}
