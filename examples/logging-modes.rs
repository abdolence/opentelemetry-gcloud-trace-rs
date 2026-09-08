//! One blueprint for both logging setups a single application needs:
//! development runs with a human-readable console layer, while GKE, Cloud
//! Run and GCE instances with the Cloud Logging agent run with the JSON
//! layer instead, since that is what the agent parses and nobody reads a
//! JSON stream by eye at a terminal. `APP_MODE` (default `development`)
//! chooses between them at startup, and exactly one of the two layers is
//! active at a time. A host with no logging agent should use
//! `examples/logging-api.rs` instead, which writes through the Cloud
//! Logging API directly rather than to stdout.

use opentelemetry_gcloud_trace::logs::GcpCloudLoggingLayerBuilder;
use opentelemetry_gcloud_trace::GcpCloudTraceExporterBuilder;
use tracing::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Development,
    Production,
}

impl AppMode {
    fn from_env() -> Self {
        match std::env::var("APP_MODE").as_deref() {
            Ok("production") => AppMode::Production,
            _ => AppMode::Development,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let project_id = config_env_var("PROJECT_ID")?;
    let mode = AppMode::from_env();

    let gcp_trace_exporter = GcpCloudTraceExporterBuilder::new(project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_attributes(vec![opentelemetry::KeyValue::new(
                "service.name",
                "logging-modes-example",
            )])
            .build(),
    );
    let tracer_provider = gcp_trace_exporter.create_provider().await?;
    let tracer = gcp_trace_exporter.install(&tracer_provider).await?;
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // `Option<Layer>` implements `Layer`, so both sinks can be built from one
    // `match` and composed unconditionally below - the inactive sink is a
    // `None` that costs nothing on the subscriber.
    let (console_layer, json_layer) = match mode {
        AppMode::Development => (Some(tracing_subscriber::fmt::layer()), None),
        AppMode::Production => (
            None,
            Some(GcpCloudLoggingLayerBuilder::new(project_id).build()),
        ),
    };

    // The OpenTelemetry layer must come first: the log layers read the span
    // context that layer attaches, and only see what is registered ahead of
    // them.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(console_layer)
        .with(json_layer);

    let dispatch = tracing::Dispatch::new(subscriber);

    tracing::dispatcher::with_default(&dispatch, || {
        let root = span!(Level::INFO, "handle_request");
        let _enter = root.enter();

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
