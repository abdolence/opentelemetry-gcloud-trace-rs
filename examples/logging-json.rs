use opentelemetry_gcloud_trace::logs::GcpCloudLoggingLayerBuilder;
use opentelemetry_gcloud_trace::GcpCloudTraceExporterBuilder;
use tracing::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let project_id = config_env_var("PROJECT_ID")?;

    let gcp_trace_exporter = GcpCloudTraceExporterBuilder::new(project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_attributes(vec![opentelemetry::KeyValue::new(
                "service.name",
                "logging-json-example",
            )])
            .build(),
    );
    let tracer_provider = gcp_trace_exporter.create_provider().await?;
    let tracer = gcp_trace_exporter.install(&tracer_provider).await?;
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    let log_layer = GcpCloudLoggingLayerBuilder::new(project_id).build();

    // The OpenTelemetry layer must come first: the log layer reads the span
    // context that layer attaches, and only sees what is registered ahead of
    // it.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(log_layer);

    tracing::subscriber::with_default(subscriber, || {
        let root = span!(Level::INFO, "handle_request");
        let _enter = root.enter();

        info!(
            "labels.tenant" = "acme",
            "http_request.request_method" = "GET",
            "http_request.request_url" = "https://example.test/orders",
            "http_request.status" = 200,
            "Serving a request through the JSON sink."
        );

        warn!(retries = 2, "Downstream call was retried.");
    });

    tracer_provider.shutdown()?;

    Ok(())
}
