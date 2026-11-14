#![cfg(feature = "logs-otlp")]

use gcloud_observability::ids::{GoogleProjectId, LogId};
use gcloud_observability::logs::{GcpCloudLoggingLayerBuilder, ServiceContext};
use opentelemetry::logs::{AnyValue, Severity};
use opentelemetry::trace::TracerProvider;
use opentelemetry::Key;
use opentelemetry_sdk::logs::{
    InMemoryLogExporter, SdkLogRecord, SdkLoggerProvider, SimpleLogProcessor,
};
use opentelemetry_sdk::trace::{
    InMemorySpanExporter, Sampler, SdkTracerProvider, SimpleSpanProcessor,
};
use std::error::Error;
use std::fmt;
use std::sync::Once;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;

const TEST_PROJECT: GoogleProjectId = GoogleProjectId::from_static("test-project");
const ORDERS_LOG: LogId = LogId::from_static("orders-log");

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
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
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
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
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
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
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

    assert_eq!(
        attribute(record, "gcp.log.name"),
        Some(&string(&ORDERS_LOG.to_string()))
    );

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
fn http_request_latency_is_sent_as_seconds() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        tracing::info!("http_request.latency" = "125ms", "slow request");
        tracing::info!("http_request.latency" = "not a duration", "odd request");
    });

    let records = emitted(&exporter);
    let http_request = attribute(&records[0], "gcp.http_request").expect("httpRequest is set");
    assert_eq!(
        map_entry(http_request, "latency"),
        Some(&AnyValue::Double(0.125))
    );
    // A value the typed field cannot hold stays readable in the body rather than getting the
    // whole record rejected.
    assert!(attribute(&records[1], "gcp.http_request").is_none());
    let body = records[1].body().expect("the body is set");
    assert_eq!(
        map_entry(
            map_entry(body, "httpRequest").expect("kept in the body"),
            "latency"
        ),
        Some(&string("not a duration"))
    );
}

#[test]
fn http_request_members_that_do_not_fit_stay_in_the_body() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        tracing::info!(
            "http_request.request_method" = "GET",
            "http_request.status" = "200 OK",
            "http_request.something_else" = 3,
            "request served"
        );
    });

    let records = emitted(&exporter);
    let http_request = attribute(&records[0], "gcp.http_request").expect("httpRequest is set");
    assert_eq!(
        map_entry(http_request, "requestMethod"),
        Some(&string("GET"))
    );
    assert_eq!(map_entry(http_request, "status"), None);
    assert_eq!(map_entry(http_request, "somethingElse"), None);
    let kept = map_entry(records[0].body().expect("the body is set"), "httpRequest")
        .expect("the unmapped members are kept in the body");
    assert_eq!(map_entry(kept, "status"), Some(&string("200 OK")));
    assert_eq!(map_entry(kept, "somethingElse"), Some(&AnyValue::Int(3)));
}

#[test]
fn operation_members_that_do_not_fit_stay_in_the_body() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        tracing::info!(
            "operation.id" = "op-1",
            "operation.first" = "yes",
            "operation.extra" = 1,
            "step done"
        );
    });

    let records = emitted(&exporter);
    let operation = attribute(&records[0], "gcp.operation").expect("operation is set");
    assert_eq!(map_entry(operation, "id"), Some(&string("op-1")));
    assert_eq!(map_entry(operation, "first"), None);
    let kept = map_entry(
        records[0].body().expect("the body is set"),
        "logging.googleapis.com/operation",
    )
    .expect("the unmapped members are kept in the body");
    assert_eq!(map_entry(kept, "first"), Some(&string("yes")));
    assert_eq!(map_entry(kept, "extra"), Some(&AnyValue::Int(1)));
}

#[test]
fn values_that_fit_no_typed_field_stay_in_the_body() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        tracing::info!(
            insert_id = 42,
            time = "yesterday",
            httpRequest = "GET /orders",
            "odd fields"
        );
    });

    let records = emitted(&exporter);
    let record = &records[0];
    assert!(attribute(record, "gcp.insert_id").is_none());
    assert!(attribute(record, "gcp.http_request").is_none());
    let body = record.body().expect("the body is set");
    assert_eq!(
        map_entry(body, "logging.googleapis.com/insertId"),
        Some(&AnyValue::Int(42))
    );
    assert_eq!(map_entry(body, "time"), Some(&string("yesterday")));
    assert_eq!(map_entry(body, "httpRequest"), Some(&string("GET /orders")));
}

#[test]
fn body_carries_the_message_target_and_user_fields() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
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
fn events_of_the_transport_crates_are_not_exported() {
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        tracing::debug!(target: "h2::codec::framed_write", "send frame");
        tracing::debug!(target: "hyper_util::client::legacy::connect::http", "connecting");
        tracing::debug!(target: "opentelemetry_sdk", "export failed");
        tracing::info!(target: "orders::h2", "emitted by the application");
    });

    let records = emitted(&exporter);
    assert_eq!(records.len(), 1);
    assert_eq!(
        map_entry(records[0].body().expect("the body is set"), "message"),
        Some(&string("emitted by the application"))
    );
}

#[derive(Debug)]
struct RootCause;

impl fmt::Display for RootCause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("card expired")
    }
}

impl Error for RootCause {}

#[derive(Debug)]
struct CheckoutFailure(RootCause);

impl fmt::Display for CheckoutFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("payment declined")
    }
}

impl Error for CheckoutFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

/// `Backtrace::capture` reads the environment once per process, so it is set before any test
/// reaches it.
fn enable_backtraces() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("RUST_BACKTRACE", "1"));
}

fn as_str(value: &AnyValue) -> &str {
    match value {
        AnyValue::String(value) => value.as_str(),
        other => panic!("expected a string, got {other:?}"),
    }
}

#[test]
fn error_reporting_keys_and_exception_attributes_are_set_on_error_events() {
    enable_backtraces();
    let (provider, exporter) = logger_provider();
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_otel_logger_provider(&provider, ORDERS_LOG)
        .with_error_reporting(ServiceContext::new("orders").with_version("1.4.2"))
        .build();

    tracing::subscriber::with_default(Registry::default().with(layer), || {
        let failure = CheckoutFailure(RootCause);
        tracing::error!(
            "exception.type" = "CheckoutFailure",
            error = &failure as &dyn Error,
            "checkout failed"
        );
    });

    let records = emitted(&exporter);
    assert_eq!(records.len(), 1);

    let record = &records[0];
    assert!(record.severity_number().expect("severity is set") >= Severity::Error);
    let body = record.body().expect("the body is set");
    assert_eq!(
        map_entry(body, "@type").map(as_str),
        Some("type.googleapis.com/google.devtools.clouderrorreporting.v1beta1.ReportedErrorEvent")
    );

    assert_eq!(
        attribute(record, "exception.type"),
        Some(&string("CheckoutFailure"))
    );
    let exception_message =
        as_str(attribute(record, "exception.message").expect("exception.message is set"));
    assert!(exception_message.contains("payment declined"));
    assert!(exception_message.contains("card expired"));
    assert!(attribute(record, "exception.stacktrace").is_none());
}
