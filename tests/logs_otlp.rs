#![cfg(feature = "logs-otlp")]

use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use opentelemetry::logs::{AnyValue, Severity};
use opentelemetry::trace::TracerProvider;
use opentelemetry::Key;
use opentelemetry_sdk::logs::{
    InMemoryLogExporter, SdkLogRecord, SdkLoggerProvider, SimpleLogProcessor,
};
use opentelemetry_sdk::trace::{
    InMemorySpanExporter, Sampler, SdkTracerProvider, SimpleSpanProcessor,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;

const LOG_ID: &str = "orders-log";

fn logger_provider() -> (SdkLoggerProvider, InMemoryLogExporter) {
    let exporter = InMemoryLogExporter::default();
    let provider = SdkLoggerProvider::builder()
        .with_log_processor(SimpleLogProcessor::new(exporter.clone()))
        .build();
    (provider, exporter)
}

fn emitted(exporter: &InMemoryLogExporter) -> Vec<SdkLogRecord> {
    exporter
        .get_emitted_logs()
        .expect("the in-memory exporter is not shut down")
        .into_iter()
        .map(|data| data.record)
        .collect()
}

fn attribute<'a>(record: &'a SdkLogRecord, key: &str) -> Option<&'a AnyValue> {
    let key = Key::from(key.to_string());
    record
        .attributes_iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

fn map_entry<'a>(value: &'a AnyValue, key: &str) -> Option<&'a AnyValue> {
    match value {
        AnyValue::Map(entries) => entries.get(&Key::from(key.to_string())),
        _ => None,
    }
}

fn string(value: &str) -> AnyValue {
    AnyValue::from(value.to_string())
}

#[test]
fn severity_is_sent_as_the_number_google_maps_to_it() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_otel_logger_provider(&provider, LOG_ID)
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        tracing::debug!("debug level");
        tracing::info!("info level");
        tracing::warn!("warn level");
        tracing::error!("error level");
        tracing::info!(severity = "notice", "notice override");
        tracing::info!(severity = "critical", "critical override");
        tracing::info!(severity = "alert", "alert override");
        tracing::info!(severity = "emergency", "emergency override");
    });

    let severities: Vec<(Option<Severity>, Option<&str>)> = emitted(&exporter)
        .iter()
        .map(|record| (record.severity_number(), record.severity_text()))
        .collect();
    assert_eq!(
        severities,
        vec![
            (Some(Severity::Debug), Some("DEBUG")),
            (Some(Severity::Info), Some("INFO")),
            (Some(Severity::Warn), Some("WARNING")),
            (Some(Severity::Error), Some("ERROR")),
            (Some(Severity::Info3), Some("NOTICE")),
            (Some(Severity::Fatal), Some("CRITICAL")),
            (Some(Severity::Fatal3), Some("ALERT")),
            (Some(Severity::Fatal4), Some("EMERGENCY")),
        ]
    );
}

#[test]
fn trace_context_is_taken_from_the_correlated_span() {
    let span_exporter = InMemorySpanExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::AlwaysOn)
        .with_span_processor(SimpleSpanProcessor::new(span_exporter.clone()))
        .build();
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_otel_logger_provider(&provider, LOG_ID)
        .build();
    let subscriber = Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer("otlp-sink-test")))
        .with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("traced-work");
        let _guard = span.enter();
        tracing::info!("correlated line");
    });

    let exported = span_exporter
        .get_finished_spans()
        .expect("the span exporter is not shut down")
        .into_iter()
        .find(|span| span.name == "traced-work")
        .expect("the span ends when its guard drops");
    let records = emitted(&exporter);
    let context = records[0]
        .trace_context()
        .expect("an event inside a span carries its trace context");
    assert_eq!(context.trace_id, exported.span_context.trace_id());
    assert_eq!(context.span_id, exported.span_context.span_id());
    assert_eq!(
        context.trace_flags,
        Some(exported.span_context.trace_flags())
    );
}

#[test]
fn http_request_operation_source_location_labels_and_insert_id_use_the_attributes_google_reads() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_otel_logger_provider(&provider, LOG_ID)
        .build();

    let event_line = line!() + 2;
    tracing::subscriber::with_default(Registry::default().with(layer), || {
        tracing::info!(
            "http_request.request_method" = "GET",
            "http_request.status" = 200,
            "operation.id" = "op-1",
            "operation.first" = true,
            "labels.tenant" = "acme",
            insert_id = "unique-1",
            "request served"
        );
    });

    let records = emitted(&exporter);
    let record = &records[0];

    assert_eq!(attribute(record, "gcp.log.name"), Some(&string(LOG_ID)));

    let http_request = attribute(record, "gcp.http_request").expect("httpRequest is set");
    assert_eq!(
        map_entry(http_request, "requestMethod"),
        Some(&string("GET"))
    );
    assert_eq!(map_entry(http_request, "status"), Some(&AnyValue::Int(200)));

    let operation = attribute(record, "gcp.operation").expect("operation is set");
    assert_eq!(map_entry(operation, "id"), Some(&string("op-1")));
    assert_eq!(
        map_entry(operation, "first"),
        Some(&AnyValue::Boolean(true))
    );

    assert_eq!(attribute(record, "code.file.path"), Some(&string(file!())));
    assert_eq!(
        attribute(record, "code.line.number"),
        Some(&AnyValue::Int(i64::from(event_line)))
    );
    assert_eq!(attribute(record, "tenant"), Some(&string("acme")));
    assert_eq!(
        attribute(record, "gcp.insert_id"),
        Some(&string("unique-1"))
    );
}

#[test]
fn body_carries_the_message_target_and_user_fields() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_otel_logger_provider(&provider, LOG_ID)
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        tracing::info!(order_id = 42, paid = true, "order placed");
    });

    let records = emitted(&exporter);
    let body = records[0].body().expect("the body is set");
    assert_eq!(map_entry(body, "message"), Some(&string("order placed")));
    assert_eq!(map_entry(body, "target"), Some(&string("logs_otlp")));
    assert_eq!(map_entry(body, "order_id"), Some(&AnyValue::Int(42)));
    assert_eq!(map_entry(body, "paid"), Some(&AnyValue::Boolean(true)));
}

#[test]
fn nothing_is_emitted_inside_a_telemetry_suppressed_scope() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new("test-project")
        .with_otel_logger_provider(&provider, LOG_ID)
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        {
            let _suppressed = opentelemetry::Context::enter_telemetry_suppressed_scope();
            tracing::info!("emitted by the exporter itself");
        }
        tracing::info!("emitted by the application");
    });

    let records = emitted(&exporter);
    assert_eq!(records.len(), 1);
    assert_eq!(
        map_entry(records[0].body().expect("the body is set"), "message"),
        Some(&string("emitted by the application"))
    );
}
