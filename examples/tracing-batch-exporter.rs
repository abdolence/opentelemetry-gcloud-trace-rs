use tracing::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

use opentelemetry_gcloud_trace::*;

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tracer: opentelemetry::sdk::trace::Tracer =
        GcpCloudTraceExporterBuilder::for_default_project_id()
            .await? // or GcpCloudTraceExporterBuilder::new(config_env_var("PROJECT_ID")?)
            .install_batch(opentelemetry::runtime::Tokio)
            .await?;

    // Create a tracing layer with the configured tracer
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    // Use the tracing subscriber `Registry`, or any other subscriber
    // that impls `LookupSpan`
    let subscriber = Registry::default().with(telemetry);

    // Trace executed code
    tracing::subscriber::with_default(subscriber, || {
        // Spans will be sent to the configured OpenTelemetry exporter
        let root = span!(tracing::Level::TRACE, "app_start", work_units = 2);
        let _enter = root.enter();

        error!("This event will be logged in the root span.");
    });

    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}
