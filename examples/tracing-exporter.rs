use tracing::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

use opentelemetry_gcloud_trace::*;

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let gcp_trace_exporter = GcpCloudTraceExporterBuilder::for_default_project_id()
        .await?
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    "simple-app",
                )])
                .build(),
        ); // or GcpCloudTraceExporterBuilder::new(config_env_var("PROJECT_ID")?)
    let tracer_provider = gcp_trace_exporter.create_provider().await?;
    let tracer: opentelemetry_sdk::trace::Tracer =
        gcp_trace_exporter.install(&tracer_provider).await?;

    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // Create a tracing layer with the configured tracer
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    // Use the tracing subscriber `Registry`, or any other subscriber
    // that impls `LookupSpan`
    let subscriber = Registry::default().with(telemetry);

    // Trace executed code
    tracing::subscriber::with_default(subscriber, || {
        // Spans will be sent to the configured OpenTelemetry exporter
        let root = span!(tracing::Level::TRACE, "my_app", work_units = 2);
        let _enter = root.enter();

        let child_span = span!(
            tracing::Level::TRACE,
            "my_child",
            work_units = 2,
            "http.client_ip" = "42.42.42.42"
        );
        child_span.in_scope(|| {
            info!(
                "Do printing, nothing more here. Please check your Google Cloud Trace dashboard."
            );
        });

        error!("This event will be logged in the root span.");
    });

    tracer_provider.shutdown()?;

    Ok(())
}
