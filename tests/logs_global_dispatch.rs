#![cfg(feature = "logs")]

mod common;

use common::SharedBuffer;
use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::{
    InMemorySpanExporter, Sampler, SdkTracerProvider, SimpleSpanProcessor,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;

// A dedicated process (each integration test binary gets one) so that
// installing the process-wide global default subscriber cannot collide with
// any other test.
#[test]
fn log_line_correlates_under_global_default_dispatch() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::AlwaysOn)
        .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
        .build();
    let tracer = provider.tracer("logs-global-dispatch-test");

    let buffer = SharedBuffer::default();
    let log_layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();

    // Context activation off so a pass here cannot be explained by the
    // current-OpenTelemetry-context fallback; only the global-default lookup
    // can supply the span context.
    let otel_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_context_activation(false);

    let subscriber = Registry::default().with(otel_layer).with(Some(log_layer));

    tracing::subscriber::set_global_default(subscriber)
        .expect("this test installs the only global subscriber in its process");

    {
        let span = tracing::info_span!("traced-work-global");
        let _guard = span.enter();
        tracing::info!("correlated line via global dispatch");
    }

    let exported = exporter
        .get_finished_spans()
        .expect("exporter is not shut down")
        .into_iter()
        .find(|span| span.name == "traced-work-global")
        .expect("the span was exported once its guard dropped");

    let lines = buffer.lines();
    let log_line = lines
        .into_iter()
        .find(|line| line["message"] == "correlated line via global dispatch")
        .expect("the log line was written");

    assert_eq!(
        log_line["logging.googleapis.com/trace"],
        format!(
            "projects/test-project/traces/{}",
            exported.span_context.trace_id()
        )
    );
    assert_eq!(
        log_line["logging.googleapis.com/spanId"],
        exported.span_context.span_id().to_string()
    );
    assert_eq!(log_line["logging.googleapis.com/trace_sampled"], true);
}
