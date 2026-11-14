//! Exports spans to Google Cloud Trace through the Cloud Trace API v2
//! (`cloudtrace.googleapis.com`). Spans come from the OpenTelemetry API directly, with
//! attributes and events, and from `tracing` through the `tracing-opentelemetry` layer, which
//! is how most Rust code creates them.
//!
//! Span events are exported as annotations only with `with_span_events(true)`, which is off
//! by default.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example traces-cloud-trace-api --features trace-api
//! ```

use gcloud_observability::trace_api::GcpCloudTraceExporterBuilder;
use opentelemetry::trace::{TraceContextExt, Tracer};
use opentelemetry::KeyValue;
use tracing::{error, info, span, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;

    // ANCHOR: setup
    let exporter = GcpCloudTraceExporterBuilder::new(google_project_id)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("traces-cloud-trace-api-example")
                .build(),
        )
        .with_span_events(true);
    let tracer_provider = exporter.create_provider().await?;
    let tracer = exporter.install(&tracer_provider);
    // ANCHOR_END: setup

    // ANCHOR: opentelemetry_api
    tracer.in_span("my_parent_work", |cx| {
        let span = cx.span();
        span.set_attribute(KeyValue::new("http.client_ip", "42.42.42.42"));
        span.add_event(
            "test-event",
            vec![KeyValue::new("test_event_attr", "test-event-value")],
        );
        tracer.in_span("my_child_work", |cx| {
            cx.span().add_event(
                "test-child-event",
                vec![KeyValue::new("test_event_attr", "test-event-value")],
            );
        })
    });
    // ANCHOR_END: opentelemetry_api

    // ANCHOR: tracing
    let subscriber = Registry::default().with(tracing_opentelemetry::layer().with_tracer(tracer));
    tracing::subscriber::with_default(subscriber, || {
        let root = span!(Level::INFO, "my_app", work_units = 2);
        let _enter = root.enter();

        span!(
            Level::INFO,
            "my_child",
            work_units = 2,
            "http.client_ip" = "42.42.42.42"
        )
        .in_scope(|| info!("Doing the child work."));

        error!("This event is added to the root span.");
    });
    // ANCHOR_END: tracing

    // Exports the spans still queued; blocks, so it needs the multi-threaded runtime
    // `#[tokio::main]` starts.
    tracer_provider.shutdown()?;

    Ok(())
}
