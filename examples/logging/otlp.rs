//! Sends traces and log entries to the Google Cloud Telemetry API through OTLP, with each log
//! entry correlated to the span that emitted it. Open the trace in Trace Explorer and its
//! "Logs" panel lists the entries under their spans, or open an entry in Logs Explorer and
//! follow its trace link back to the waterfall.
//!
//! The logging layer sends through the same OTLP builder that exports the spans, so one set of
//! credentials and one endpoint serve both. The layer needs the logger provider's shutdown
//! to flush: the provider is owned by the caller.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example logging-otlp --features logs-otlp
//! ```

use gcloud_observability::ids::LogId;
use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use opentelemetry::trace::TraceContextExt;
use std::time::Duration;
use tracing::*;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[instrument(skip_all, fields(order_id = %order_id))]
async fn fetch_from_db(order_id: &str) -> u64 {
    tokio::time::sleep(Duration::from_millis(15)).await;
    let rows = 3;
    info!(order_id, rows, "loaded order rows");
    rows
}

#[instrument(skip_all, fields(order_id = %order_id))]
async fn send_notification(order_id: &str) -> Result<(), String> {
    tokio::time::sleep(Duration::from_millis(10)).await;
    let err = format!("no notification channel configured for order {order_id}");
    error!(error = %err, "failed to notify customer of checkout");
    Err(err)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = config_env_var("PROJECT_ID")?;

    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_attributes(vec![opentelemetry::KeyValue::new(
                "service.name",
                "logging-otlp-example",
            )])
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);
    let logger_provider = otlp.create_logger_provider().await?;
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id.clone())
        .with_otel_logger_provider(
            &logger_provider,
            LogId::from_static("gcloud-observability-example"),
        )
        .build()?;

    // The OpenTelemetry layer must come first: the log layer reads the span context that layer
    // attaches, and only sees what is registered ahead of it.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(log_layer);
    tracing::subscriber::set_global_default(subscriber)?;

    let root = info_span!("handle_request", order_id = "order-42", tenant = "acme");
    let trace_id = {
        let _enter = root.enter();

        info!(
            "labels.tenant" = "acme",
            "http_request.request_method" = "POST",
            "http_request.request_url" = "https://example.test/orders/order-42/checkout",
            "http_request.status" = 200,
            "http_request.latency" = "0.125s",
            "checkout request received"
        );

        let rows = fetch_from_db("order-42").await;
        info!(rows, "order ready for fulfilment");

        if send_notification("order-42").await.is_err() {
            warn!("checkout completed without customer notification");
        }

        root.context().span().span_context().trace_id()
    };
    drop(root);

    info!(
        trace_id = %trace_id,
        trace_explorer = %format!(
            "https://console.cloud.google.com/traces/list?project={google_project_id}&tid={trace_id}"
        ),
        "request handled; open the trace in the console"
    );

    tracer_provider.shutdown()?;
    logger_provider.shutdown()?;

    Ok(())
}
