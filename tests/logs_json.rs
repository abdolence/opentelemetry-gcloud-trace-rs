#![cfg(feature = "logs")]

use opentelemetry::trace::TracerProvider;
use opentelemetry_gcloud_trace::logs::GcpCloudLoggingLayerBuilder;
use opentelemetry_sdk::trace::{
    InMemorySpanExporter, Sampler, SdkTracerProvider, SimpleSpanProcessor,
};
use serde_json::Value;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;

#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn lines(&self) -> Vec<Value> {
        let buffer = self
            .0
            .lock()
            .expect("buffer mutex is never poisoned in these tests");
        String::from_utf8_lossy(&buffer)
            .lines()
            .map(|line| serde_json::from_str(line).expect("sink writes one JSON object per line"))
            .collect()
    }
}

struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBufferWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("buffer mutex is never poisoned in these tests")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    type Writer = SharedBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferWriter(self.0.clone())
    }
}

#[test]
fn severity_matches_level_for_each_tracing_level() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::trace!("t");
        tracing::debug!("d");
        tracing::info!("i");
        tracing::warn!("w");
        tracing::error!("e");
    });

    let lines = buffer.lines();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0]["severity"], "DEBUG");
    assert_eq!(lines[1]["severity"], "DEBUG");
    assert_eq!(lines[2]["severity"], "INFO");
    assert_eq!(lines[3]["severity"], "WARNING");
    assert_eq!(lines[4]["severity"], "ERROR");
}

#[test]
fn severity_field_overrides_level_case_insensitively() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(severity = "critical", "overridden");
    });

    let lines = buffer.lines();
    assert_eq!(lines[0]["severity"], "CRITICAL");
}

#[test]
fn severity_field_falls_back_to_level_when_unrecognised() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::error!(severity = "not-a-real-severity", "still an error");
    });

    let lines = buffer.lines();
    assert_eq!(lines[0]["severity"], "ERROR");
}

#[test]
fn message_falls_back_to_event_name_when_no_message_field() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::event!(tracing::Level::INFO, counter = 1);
    });

    let lines = buffer.lines();
    let message = lines[0]["message"].as_str().expect("message is a string");
    assert!(
        message.starts_with("event "),
        "expected the default tracing event name, got {message:?}"
    );
}

#[test]
fn http_request_fields_become_camel_cased_http_request_object() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            http_request.request_method = "GET",
            http_request.status = 200,
            "served"
        );
    });

    let lines = buffer.lines();
    assert_eq!(lines[0]["httpRequest"]["requestMethod"], "GET");
    assert_eq!(lines[0]["httpRequest"]["status"], 200);
}

#[test]
fn labels_fields_become_stringified_labels_object() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(labels.retry_count = 3, labels.tenant = "acme", "labelled");
    });

    let lines = buffer.lines();
    assert_eq!(
        lines[0]["logging.googleapis.com/labels"]["retry_count"],
        "3"
    );
    assert_eq!(lines[0]["logging.googleapis.com/labels"]["tenant"], "acme");
}

#[test]
fn operation_fields_populate_operation_object() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            operation.id = "op-1",
            operation.producer = "svc",
            operation.first = true,
            operation.last = false,
            "operation event"
        );
    });

    let lines = buffer.lines();
    let operation = &lines[0]["logging.googleapis.com/operation"];
    assert_eq!(operation["id"], "op-1");
    assert_eq!(operation["producer"], "svc");
    assert_eq!(operation["first"], true);
    assert_eq!(operation["last"], false);
}

#[test]
fn insert_id_field_populates_insert_id() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(insert_id = "abc-123", "deduplicated");
    });

    let lines = buffer.lines();
    assert_eq!(lines[0]["logging.googleapis.com/insertId"], "abc-123");
}

#[test]
fn arbitrary_and_dotted_field_names_are_kept_verbatim() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(user.id = 42, plain_field = "x", "verbatim");
    });

    let lines = buffer.lines();
    assert_eq!(lines[0]["user.id"], 42);
    assert_eq!(lines[0]["plain_field"], "x");
}

#[test]
fn source_location_present_when_enabled() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("with location");
    });

    let lines = buffer.lines();
    let source_location = &lines[0]["logging.googleapis.com/sourceLocation"];
    assert!(source_location["file"]
        .as_str()
        .unwrap()
        .ends_with("logs_json.rs"));
    assert!(source_location["line"].is_string());
}

#[test]
fn source_location_absent_when_disabled() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_source_location(false)
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("without location");
    });

    let lines = buffer.lines();
    assert!(lines[0]
        .get("logging.googleapis.com/sourceLocation")
        .is_none());
}

#[test]
fn event_outside_any_span_carries_no_trace_fields() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("no span here");
    });

    let lines = buffer.lines();
    assert!(lines[0].get("logging.googleapis.com/trace").is_none());
    assert!(lines[0].get("logging.googleapis.com/spanId").is_none());
}

#[test]
fn event_in_span_without_otel_layer_carries_no_trace_fields() {
    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();
    let subscriber = Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("no-otel-layer");
        let _guard = span.enter();
        tracing::info!("inside a span, but nothing correlates it");
    });

    let lines = buffer.lines();
    assert!(lines[0].get("logging.googleapis.com/trace").is_none());
    assert!(lines[0].get("logging.googleapis.com/spanId").is_none());
}

#[test]
fn log_line_correlates_with_the_exported_span() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::AlwaysOn)
        .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
        .build();
    let tracer = provider.tracer("logs-correlation-test");

    let buffer = SharedBuffer::default();
    let log_layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .build();

    let subscriber = Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(log_layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("traced-work");
        let _guard = span.enter();
        tracing::info!("correlated line");
    });

    let exported = exporter
        .get_finished_spans()
        .expect("exporter is not shut down")
        .into_iter()
        .find(|span| span.name == "traced-work")
        .expect("the span was exported once its guard dropped");

    let lines = buffer.lines();
    let log_line = lines
        .into_iter()
        .find(|line| line["message"] == "correlated line")
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
