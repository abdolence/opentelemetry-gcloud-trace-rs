#![cfg(feature = "logs")]

mod common;

use common::SharedBuffer;
use gcloud_observability::logs::{GcpCloudLoggingLayerBuilder, ServiceContext};
use serde_json::Value;
use std::error::Error;
use std::fmt;
use std::sync::Once;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;

const REPORTED_ERROR_EVENT: &str =
    "type.googleapis.com/google.devtools.clouderrorreporting.v1beta1.ReportedErrorEvent";

#[derive(Debug)]
struct RootCause;

impl fmt::Display for RootCause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("card expired")
    }
}

impl Error for RootCause {}

#[derive(Debug)]
struct GatewayFailure(RootCause);

impl fmt::Display for GatewayFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("gateway rejected the charge")
    }
}

impl Error for GatewayFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

#[derive(Debug)]
struct CheckoutFailure(GatewayFailure);

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

fn checkout_failure() -> CheckoutFailure {
    CheckoutFailure(GatewayFailure(RootCause))
}

/// `Backtrace::capture` reads the environment once per process, so it is set before any test
/// reaches it.
fn enable_backtraces() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("RUST_BACKTRACE", "1"));
}

fn lines_from(
    builder: GcpCloudLoggingLayerBuilder<SharedBuffer>,
    buffer: &SharedBuffer,
    emit: impl FnOnce(),
) -> Vec<Value> {
    enable_backtraces();
    let subscriber = Registry::default().with(builder.build());
    tracing::subscriber::with_default(subscriber, emit);
    buffer.lines()
}

fn reporting_builder(buffer: &SharedBuffer) -> GcpCloudLoggingLayerBuilder<SharedBuffer> {
    GcpCloudLoggingLayerBuilder::new("test-project")
        .with_json_writer(buffer.clone())
        .with_error_reporting(ServiceContext::new("orders").with_version("1.4.2"))
}

fn plain_builder(buffer: &SharedBuffer) -> GcpCloudLoggingLayerBuilder<SharedBuffer> {
    GcpCloudLoggingLayerBuilder::new("test-project").with_json_writer(buffer.clone())
}

fn position(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is missing from `{haystack}`"))
}

#[test]
fn error_event_is_reported_with_service_location_and_source_chain() {
    let buffer = SharedBuffer::default();
    let event_line = line!() + 3;
    let lines = lines_from(reporting_builder(&buffer), &buffer, || {
        let failure = checkout_failure();
        tracing::error!(error = &failure as &dyn Error, "checkout failed");
    });

    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(line["@type"], REPORTED_ERROR_EVENT);
    assert_eq!(line["serviceContext"]["service"], "orders");
    assert_eq!(line["serviceContext"]["version"], "1.4.2");
    let location = &line["context"]["reportLocation"];
    assert!(location["filePath"]
        .as_str()
        .is_some_and(|path| path.ends_with("logs_error_reporting.rs")));
    assert_eq!(location["lineNumber"], event_line);
    assert_eq!(location["functionName"], "logs_error_reporting");

    let message = line["message"].as_str().expect("message is a string");
    let header = position(message, "checkout failed");
    let outer = position(message, "payment declined");
    let middle = position(message, "gateway rejected the charge");
    let root = position(message, "card expired");
    assert!(header < outer && outer < middle && middle < root);
}

#[test]
fn source_chain_is_on_the_header_line_of_the_message() {
    let buffer = SharedBuffer::default();
    let lines = lines_from(reporting_builder(&buffer), &buffer, || {
        let failure = checkout_failure();
        tracing::error!(error = &failure as &dyn Error, "checkout failed");
    });

    let message = lines[0]["message"].as_str().expect("message is a string");
    let header = message.lines().next().expect("message has a header line");
    for part in [
        "checkout failed",
        "payment declined",
        "gateway rejected the charge",
        "card expired",
    ] {
        assert!(header.contains(part), "`{part}` is missing from `{header}`");
    }
    assert!(!message.contains("Caused by"));
}

#[test]
fn error_message_carries_the_captured_backtrace_and_no_stack_trace_key() {
    let buffer = SharedBuffer::default();
    let lines = lines_from(reporting_builder(&buffer), &buffer, || {
        tracing::error!("checkout failed");
    });

    let message = lines[0]["message"].as_str().expect("message is a string");
    assert!(message.contains("logs_error_reporting"));
    assert!(lines[0].get("stack_trace").is_none());
}

#[test]
fn http_request_fields_are_copied_into_the_error_context() {
    let buffer = SharedBuffer::default();
    let lines = lines_from(reporting_builder(&buffer), &buffer, || {
        tracing::error!(
            "http_request.request_method" = "POST",
            "http_request.request_url" = "https://example.test/checkout",
            "http_request.status" = 500,
            "http_request.user_agent" = "curl/8",
            "http_request.referer" = "https://example.test/cart",
            "http_request.remote_ip" = "10.0.0.1",
            "checkout failed"
        );
    });

    let request = &lines[0]["context"]["httpRequest"];
    assert_eq!(request["method"], "POST");
    assert_eq!(request["url"], "https://example.test/checkout");
    assert_eq!(request["responseStatusCode"], 500);
    assert_eq!(request["userAgent"], "curl/8");
    assert_eq!(request["referrer"], "https://example.test/cart");
    assert_eq!(request["remoteIp"], "10.0.0.1");
    assert_eq!(lines[0]["httpRequest"]["requestMethod"], "POST");
}

#[test]
fn severity_override_at_critical_is_reported() {
    let buffer = SharedBuffer::default();
    let lines = lines_from(reporting_builder(&buffer), &buffer, || {
        tracing::info!(severity = "critical", "data store unreachable");
    });

    assert_eq!(lines[0]["@type"], REPORTED_ERROR_EVENT);
}

#[test]
fn events_below_error_are_left_untouched() {
    let buffer = SharedBuffer::default();
    let lines = lines_from(reporting_builder(&buffer), &buffer, || {
        tracing::info!("checkout started");
        tracing::warn!("slow gateway");
    });

    assert_eq!(lines.len(), 2);
    for line in &lines {
        for key in ["@type", "serviceContext", "context"] {
            assert!(line.get(key).is_none(), "{key} was added to {line}");
        }
    }
    assert_eq!(lines[0]["message"], "checkout started");
}

#[test]
fn nothing_is_added_without_the_opt_in() {
    let buffer = SharedBuffer::default();
    let lines = lines_from(plain_builder(&buffer), &buffer, || {
        tracing::error!("checkout failed");
    });

    for key in ["@type", "serviceContext", "context"] {
        assert!(lines[0].get(key).is_none(), "{key} was added");
    }
    assert_eq!(lines[0]["message"], "checkout failed");
}

#[test]
fn error_field_keeps_its_source_chain_without_the_opt_in() {
    let buffer = SharedBuffer::default();
    let lines = lines_from(plain_builder(&buffer), &buffer, || {
        let failure = checkout_failure();
        tracing::error!(error = &failure as &dyn Error, "checkout failed");
    });

    let rendered = lines[0]["error"].as_str().expect("error is a string");
    let outer = position(rendered, "payment declined");
    let middle = position(rendered, "gateway rejected the charge");
    let root = position(rendered, "card expired");
    assert!(outer < middle && middle < root);
}
