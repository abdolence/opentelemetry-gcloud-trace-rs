//! Error Reporting support for the logging layer.
//!
//! Cloud Error Reporting groups the entries it recognises as errors. For entries written
//! through Cloud Logging it recognises one when its `jsonPayload` carries the
//! `ReportedErrorEvent` type, which forces evaluation, and reads the error text from
//! `stack_trace`, then `exception`, then `message`; the first present wins. Rust stack traces
//! are accepted when they carry standard `std::backtrace` frames. Without a trace, an error is
//! grouped by its message and by `context.reportLocation`, which is why that is always set.
//!
//! <https://docs.cloud.google.com/error-reporting/docs/formatting-error-messages>
//! <https://docs.cloud.google.com/error-reporting/reference/rest/v1beta1/projects.events/report>

use serde_json::{Map, Value};
use std::backtrace::{Backtrace, BacktraceStatus};
use std::error::Error;
use std::fmt;
use tracing::Metadata;

use super::format::{as_i64, KEY_HTTP_REQUEST, KEY_MESSAGE};

/// Forces Error Reporting to evaluate the entry as an error event.
const KEY_TYPE: &str = "@type";
const REPORTED_ERROR_EVENT_TYPE: &str =
    "type.googleapis.com/google.devtools.clouderrorreporting.v1beta1.ReportedErrorEvent";
/// Required: names the service the error is grouped under.
const KEY_SERVICE_CONTEXT: &str = "serviceContext";
/// Holds `reportLocation` and `httpRequest`, as Error Reporting's `ErrorContext`.
const KEY_CONTEXT: &str = "context";
/// Required when the entry carries no stack trace.
const KEY_REPORT_LOCATION: &str = "reportLocation";
/// Error Reporting's own request context, a different shape from the top-level `httpRequest`
/// that Cloud Logging maps to `LogEntry.httpRequest`.
const KEY_CONTEXT_HTTP_REQUEST: &str = "httpRequest";
/// An event field that names the exception type for the OTLP `exception.type` attribute; a
/// `&dyn Error` has no type name on stable Rust.
const FIELD_EXCEPTION_TYPE: &str = "exception.type";

/// `HttpRequest` member names mapped to Error Reporting's `HttpRequestContext` members.
const HTTP_REQUEST_CONTEXT_KEYS: [(&str, &str); 6] = [
    ("requestMethod", "method"),
    ("requestUrl", "url"),
    ("userAgent", "userAgent"),
    ("referer", "referrer"),
    ("status", "responseStatusCode"),
    ("remoteIp", "remoteIp"),
];

/// The service Error Reporting groups errors under, as `serviceContext` in its entries.
///
/// Set through
/// [`GcpCloudLoggingLayerBuilder::with_error_reporting`](crate::logs::GcpCloudLoggingLayerBuilder::with_error_reporting).
#[derive(Debug, Clone)]
pub struct ServiceContext {
    service: String,
    version: Option<String>,
}

impl ServiceContext {
    /// Names the service, for example the Cloud Run service or the binary name.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            version: None,
        }
    }

    /// Sets the deployed version, which Error Reporting shows beside the error.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Turns `record`, built for an event at ERROR severity or above, into an Error Reporting
    /// event, and returns the exception details the OTLP sink sends as attributes.
    ///
    /// `error` is the first `&dyn Error` field of the event. The backtrace is captured here, at
    /// the event site, so it costs nothing unless `RUST_BACKTRACE` or `RUST_LIB_BACKTRACE`
    /// enables it.
    pub(crate) fn report(
        &self,
        record: &mut Map<String, Value>,
        metadata: &Metadata<'_>,
        error: Option<&ErrorChain>,
    ) -> ExceptionDetails {
        let event_message = match record.get(KEY_MESSAGE) {
            Some(Value::String(message)) => message.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        };
        let exception_message = match error {
            Some(chain) => chain.to_string(),
            None => event_message.clone(),
        };
        let stacktrace = capture_backtrace();

        // The trace goes in `message` only: `stack_trace` would hide it, and with it the header.
        let mut message = match error {
            Some(_) => format!("{event_message}: {exception_message}"),
            None => event_message,
        };
        if let Some(stacktrace) = &stacktrace {
            message.push_str("\n\n");
            message.push_str(stacktrace);
        }
        record.insert(KEY_MESSAGE.to_string(), Value::from(message));

        record.insert(KEY_TYPE.to_string(), Value::from(REPORTED_ERROR_EVENT_TYPE));
        let mut service = Map::new();
        service.insert("service".to_string(), Value::from(self.service.clone()));
        if let Some(version) = &self.version {
            service.insert("version".to_string(), Value::from(version.clone()));
        }
        record.insert(KEY_SERVICE_CONTEXT.to_string(), Value::Object(service));

        let mut context = Map::new();
        let mut location = Map::new();
        if let Some(file) = metadata.file() {
            location.insert("filePath".to_string(), Value::from(file));
        }
        if let Some(line) = metadata.line() {
            location.insert("lineNumber".to_string(), Value::from(line));
        }
        if let Some(module_path) = metadata.module_path() {
            location.insert("functionName".to_string(), Value::from(module_path));
        }
        if !location.is_empty() {
            context.insert(KEY_REPORT_LOCATION.to_string(), Value::Object(location));
        }
        if let Some(Value::Object(request)) = record.get(KEY_HTTP_REQUEST) {
            let request = http_request_context(request);
            if !request.is_empty() {
                context.insert(KEY_CONTEXT_HTTP_REQUEST.to_string(), Value::Object(request));
            }
        }
        if !context.is_empty() {
            record.insert(KEY_CONTEXT.to_string(), Value::Object(context));
        }

        ExceptionDetails {
            kind: match record.get(FIELD_EXCEPTION_TYPE) {
                Some(Value::String(kind)) => Some(kind.clone()),
                _ => None,
            },
            message: exception_message,
            stacktrace,
        }
    }
}

fn http_request_context(request: &Map<String, Value>) -> Map<String, Value> {
    HTTP_REQUEST_CONTEXT_KEYS
        .iter()
        .filter_map(|(logging_key, context_key)| {
            let value = request.get(*logging_key)?;
            // Error Reporting types the status as an integer; the layer may hold it as a string.
            let value = if *logging_key == "status" {
                as_i64(value).map_or_else(|| value.clone(), Value::from)
            } else {
                value.clone()
            };
            Some((context_key.to_string(), value))
        })
        .collect()
}

/// The backtrace at the call site, or `None` when capturing is disabled or unsupported.
/// Never forces a capture: that costs milliseconds per event.
fn capture_backtrace() -> Option<String> {
    let backtrace = Backtrace::capture();
    (backtrace.status() == BacktraceStatus::Captured).then(|| backtrace.to_string())
}

/// What the OTLP sink sets as the OpenTelemetry `exception.*` attributes on an error event.
// Read only by the OTLP sink.
#[cfg_attr(not(feature = "logs-otlp"), allow(dead_code))]
pub(crate) struct ExceptionDetails {
    pub(crate) kind: Option<String>,
    pub(crate) message: String,
    pub(crate) stacktrace: Option<String>,
}

/// An error and its `source()` chain, flattened to text. `tracing` hands an event field only
/// as `&dyn Error`, so the chain is read while the event is recorded.
pub(crate) struct ErrorChain {
    display: String,
    causes: Vec<String>,
}

impl ErrorChain {
    pub(crate) fn new(error: &(dyn Error + 'static)) -> Self {
        Self {
            display: error.to_string(),
            causes: std::iter::successors(error.source(), |cause| (*cause).source())
                .map(ToString::to_string)
                .collect(),
        }
    }
}

impl fmt::Display for ErrorChain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.display)?;
        for cause in &self.causes {
            write!(formatter, "\nCaused by: {cause}")?;
        }
        Ok(())
    }
}

/// Emits one `tracing` error event for every panic, then calls the hook that was installed
/// before. Call it once, early in `main`.
///
/// The event carries the panic message and `panic.location`. It reaches Error Reporting when
/// the logging layer was built with
/// [`with_error_reporting`](crate::logs::GcpCloudLoggingLayerBuilder::with_error_reporting), and
/// the backtrace the layer captures then starts at the panic. `reportLocation` names this hook,
/// not the panicking code, so a panic is grouped by its trace, which needs `RUST_BACKTRACE`.
pub fn install_panic_hook() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("panic with a non-string payload");
        match info.location() {
            Some(location) => tracing::error!(
                "exception.type" = "panic",
                "panic.location" = %location,
                "{message}"
            ),
            None => tracing::error!("exception.type" = "panic", "{message}"),
        }
        previous_hook(info);
    }));
}
