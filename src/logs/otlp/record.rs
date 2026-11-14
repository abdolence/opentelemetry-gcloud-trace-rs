//! Turns the shared log record produced by [`crate::logs::format`] into an OpenTelemetry log
//! record in the shape the Google Cloud Telemetry API maps onto a `LogEntry`.
//!
//! The mapping is documented at
//! <https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/otlp-log-record-to-log-entry>.
//! Each key below cites the row of that page it follows. What the page maps to no typed field
//! becomes a label, which is why only the record's own user fields go into the body.

use crate::logs::error_reporting::ExceptionDetails;
use crate::logs::format::{
    as_i64, duration_seconds, TraceCorrelation, KEY_HTTP_REQUEST, KEY_INSERT_ID, KEY_LABELS,
    KEY_OPERATION, KEY_SEVERITY, KEY_SOURCE_LOCATION, KEY_SPAN_ID, KEY_TIME, KEY_TRACE,
    KEY_TRACE_SAMPLED,
};
use opentelemetry::logs::{AnyValue, LogRecord, Severity};
use opentelemetry::trace::TraceFlags;
use opentelemetry::Key;
use serde_json::{Map, Value};
use std::time::SystemTime;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Read for `LogEntry.logName`, after the record's `event_name`, which is left unset here so
/// that this one decides.
const ATTRIBUTE_LOG_NAME: &str = "gcp.log.name";
/// `LogEntry.insertId`.
const ATTRIBUTE_INSERT_ID: &str = "gcp.insert_id";
/// `LogEntry.httpRequest`, as a nested map of the proto's JSON member names.
const ATTRIBUTE_HTTP_REQUEST: &str = "gcp.http_request";
/// `HttpRequest.latency` member name.
const KEY_LATENCY: &str = "latency";
/// `LogEntry.operation`, as a nested map; flattened names are not read.
const ATTRIBUTE_OPERATION: &str = "gcp.operation";
/// `LogEntry.sourceLocation.file`.
const ATTRIBUTE_SOURCE_FILE: &str = "code.file.path";
/// `LogEntry.sourceLocation.function`.
const ATTRIBUTE_SOURCE_FUNCTION: &str = "code.function.name";
/// `LogEntry.sourceLocation.line`.
const ATTRIBUTE_SOURCE_LINE: &str = "code.line.number";
/// OpenTelemetry's exception conventions, which Google maps to labels. The stack trace is a
/// single string here; `tracing-opentelemetry`'s array of the source chain under the same name
/// is a different thing and is never reused.
/// <https://opentelemetry.io/docs/specs/semconv/exceptions/exceptions-logs/>
const ATTRIBUTE_EXCEPTION_TYPE: &str = "exception.type";
const ATTRIBUTE_EXCEPTION_MESSAGE: &str = "exception.message";
const ATTRIBUTE_EXCEPTION_STACKTRACE: &str = "exception.stacktrace";

/// Fills `log_record` from `record`. `correlation` supplies the trace context as typed ids,
/// so the trace keys the record carries for the other sinks are dropped. `exception` is set
/// for an Error Reporting event.
pub(crate) fn populate<R: LogRecord>(
    log_record: &mut R,
    mut record: Map<String, Value>,
    exception: Option<&ExceptionDetails>,
    correlation: Option<&TraceCorrelation>,
    log_id: &str,
) {
    if let Some(timestamp) = record.remove(KEY_TIME).as_ref().and_then(parse_timestamp) {
        log_record.set_timestamp(timestamp);
    }
    if let Some((number, text)) = record
        .remove(KEY_SEVERITY)
        .as_ref()
        .and_then(Value::as_str)
        .and_then(severity)
    {
        log_record.set_severity_number(number);
        log_record.set_severity_text(text);
    }

    record.remove(KEY_TRACE);
    record.remove(KEY_SPAN_ID);
    record.remove(KEY_TRACE_SAMPLED);
    if let Some(correlation) = correlation {
        let flags = if correlation.sampled {
            TraceFlags::SAMPLED
        } else {
            TraceFlags::default()
        };
        log_record.set_trace_context(correlation.trace_id, correlation.span_id, Some(flags));
    }

    log_record.add_attribute(ATTRIBUTE_LOG_NAME, log_id.to_string());

    if let Some(Value::Object(location)) = record.remove(KEY_SOURCE_LOCATION) {
        if let Some(file) = location.get("file").and_then(Value::as_str) {
            log_record.add_attribute(ATTRIBUTE_SOURCE_FILE, file.to_string());
        }
        if let Some(line) = location.get("line").and_then(as_i64) {
            log_record.add_attribute(ATTRIBUTE_SOURCE_LINE, line);
        }
        if let Some(function) = location.get("function").and_then(Value::as_str) {
            log_record.add_attribute(ATTRIBUTE_SOURCE_FUNCTION, function.to_string());
        }
    }
    if let Some(Value::Object(mut request)) = record.remove(KEY_HTTP_REQUEST) {
        // The Telemetry API reads `latency` as a number of seconds and rejects the whole log
        // record for a Duration string such as `125ms`.
        if let Some(latency) = request.remove(KEY_LATENCY) {
            match duration_seconds(&latency) {
                Some(seconds) => {
                    request.insert(KEY_LATENCY.to_string(), Value::from(seconds));
                }
                None => {
                    let mut unmapped = Map::new();
                    unmapped.insert(KEY_LATENCY.to_string(), latency);
                    record.insert(KEY_HTTP_REQUEST.to_string(), Value::Object(unmapped));
                }
            }
        }
        if !request.is_empty() {
            log_record.add_attribute(ATTRIBUTE_HTTP_REQUEST, any_map(request));
        }
    }
    if let Some(Value::Object(operation)) = record.remove(KEY_OPERATION) {
        log_record.add_attribute(ATTRIBUTE_OPERATION, any_map(operation));
    }
    if let Some(insert_id) = record
        .remove(KEY_INSERT_ID)
        .as_ref()
        .and_then(Value::as_str)
    {
        log_record.add_attribute(ATTRIBUTE_INSERT_ID, insert_id.to_string());
    }
    if let Some(exception) = exception {
        if let Some(kind) = &exception.kind {
            log_record.add_attribute(ATTRIBUTE_EXCEPTION_TYPE, kind.clone());
        }
        log_record.add_attribute(ATTRIBUTE_EXCEPTION_MESSAGE, exception.message.clone());
        if let Some(stacktrace) = &exception.stacktrace {
            log_record.add_attribute(ATTRIBUTE_EXCEPTION_STACKTRACE, stacktrace.clone());
        }
    }
    // Every attribute that maps to no typed field becomes a label.
    if let Some(Value::Object(labels)) = record.remove(KEY_LABELS) {
        for (name, value) in labels {
            if let Some(value) = any_value(value) {
                log_record.add_attribute(name, value);
            }
        }
    }

    // A KeyValueList body becomes `jsonPayload`.
    log_record.set_body(any_map(record));
}

/// The severity number Google maps onto the Cloud Logging severity of the same name, and the
/// name itself. `DEFAULT` has no number: a record with neither number nor text is `DEFAULT`.
fn severity(name: &str) -> Option<(Severity, &'static str)> {
    match name {
        "DEBUG" => Some((Severity::Debug, "DEBUG")),
        "INFO" => Some((Severity::Info, "INFO")),
        "NOTICE" => Some((Severity::Info3, "NOTICE")),
        "WARNING" => Some((Severity::Warn, "WARNING")),
        "ERROR" => Some((Severity::Error, "ERROR")),
        "CRITICAL" => Some((Severity::Fatal, "CRITICAL")),
        "ALERT" => Some((Severity::Fatal3, "ALERT")),
        "EMERGENCY" => Some((Severity::Fatal4, "EMERGENCY")),
        _ => None,
    }
}

fn parse_timestamp(value: &Value) -> Option<SystemTime> {
    OffsetDateTime::parse(value.as_str()?, &Rfc3339)
        .ok()
        .map(SystemTime::from)
}

fn any_map(fields: Map<String, Value>) -> AnyValue {
    fields
        .into_iter()
        .filter_map(|(name, value)| Some((Key::from(name), any_value(value)?)))
        .collect()
}

/// `None` for JSON null, which an OpenTelemetry value cannot express.
fn any_value(value: Value) -> Option<AnyValue> {
    match value {
        Value::Null => None,
        Value::Bool(value) => Some(AnyValue::Boolean(value)),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => Some(AnyValue::Int(integer)),
            // An integer beyond `i64` would lose digits as a double.
            None if number.is_u64() => Some(AnyValue::String(number.to_string().into())),
            None => number.as_f64().map(AnyValue::Double),
        },
        Value::String(value) => Some(AnyValue::String(value.into())),
        Value::Array(values) => Some(AnyValue::ListAny(Box::new(
            values.into_iter().filter_map(any_value).collect(),
        ))),
        Value::Object(fields) => Some(any_map(fields)),
    }
}
