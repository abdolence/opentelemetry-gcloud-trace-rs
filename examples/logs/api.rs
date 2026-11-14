use gcloud_observability::ids::{GoogleProjectId, LogId};
use gcloud_observability::logs::{GcpCloudLoggingApiConfig, GcpCloudLoggingLayerBuilder};
use gcloud_observability::trace_api::GcpCloudTraceExporterBuilder;
use tracing::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id: GoogleProjectId = config_env_var("PROJECT_ID")?.parse()?;

    let gcp_trace_exporter = GcpCloudTraceExporterBuilder::new(google_project_id.clone())
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    "logging-only-api-example",
                )])
                .build(),
        );
    let tracer_provider = gcp_trace_exporter.create_provider().await?;
    let tracer = gcp_trace_exporter.install(&tracer_provider);
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    let (log_layer, log_handle) = GcpCloudLoggingLayerBuilder::new(google_project_id)
        .with_cloud_logging_api(GcpCloudLoggingApiConfig::new(LogId::from_static(
            "gcloud-observability-example",
        )))
        .build_async()
        .await?;

    // The OpenTelemetry layer must come first: the log layer reads the span
    // context that layer attaches, and only sees what is registered ahead of
    // it.
    let subscriber = Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(log_layer);

    tracing::subscriber::with_default(subscriber, || {
        let root = span!(Level::INFO, "handle_request");
        let _enter = root.enter();

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

    log_handle.shutdown().await;
    tracer_provider.shutdown()?;

    Ok(())
}
