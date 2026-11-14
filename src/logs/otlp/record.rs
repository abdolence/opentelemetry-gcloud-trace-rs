//! Turns the shared log record produced by [`crate::logs::format`] into an OpenTelemetry log
//! record in the shape the Google Cloud Telemetry API maps onto a `LogEntry`.
//!
//! The mapping is documented at
//! <https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/otlp-log-record-to-log-entry>.
//! Each key below cites the row of that page it follows. What the page maps to no typed field
//! becomes a label, which is why only the record's own user fields go into the body.

use crate::ids::LogId;
use crate::logs::error_reporting::ExceptionDetails;
use crate::logs::format::{
    as_i64, duration_seconds, take, TraceCorrelation, KEY_HTTP_REQUEST, KEY_INSERT_ID, KEY_LABELS,
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
/// `LogEntry.operation`, as a nested map; flattened names are not read.
const ATTRIBUTE_OPERATION: &str = "gcp.operation";
/// `LogEntry.sourceLocation.file`.
const ATTRIBUTE_SOURCE_FILE: &str = "code.file.path";
/// `LogEntry.sourceLocation.line`.
const ATTRIBUTE_SOURCE_LINE: &str = "code.line.number";
/// OpenTelemetry's exception conventions, which Google maps to labels. `exception.stacktrace`
/// is left out: it would be a label as large as the backtrace already in the body's `message`.
/// <https://opentelemetry.io/docs/specs/semconv/exceptions/exceptions-logs/>
const ATTRIBUTE_EXCEPTION_TYPE: &str = "exception.type";
const ATTRIBUTE_EXCEPTION_MESSAGE: &str = "exception.message";

/// Fills `log_record` from `record`. `correlation` supplies the trace context as typed ids,
/// so the trace keys the record carries for the other sinks are dropped. `exception` is set
/// for an Error Reporting event.
pub(crate) fn populate<R: LogRecord>(
    log_record: &mut R,
    mut record: Map<String, Value>,
    exception: Option<&ExceptionDetails>,
    correlation: Option<&TraceCorrelation>,
    log_id: &LogId,
) {
    take(&mut record, KEY_TIME, |value| {
        parse_timestamp(&value).map(|timestamp| log_record.set_timestamp(timestamp))
    });
    take(&mut record, KEY_SEVERITY, |value| {
        value.as_str().and_then(severity).map(|(number, text)| {
            log_record.set_severity_number(number);
            log_record.set_severity_text(text);
        })
    });

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

    take(&mut record, KEY_SOURCE_LOCATION, |value| {
        let Value::Object(location) = value else {
            return None;
        };
        if let Some(file) = location.get("file").and_then(Value::as_str) {
            log_record.add_attribute(ATTRIBUTE_SOURCE_FILE, file.to_string());
        }
        if let Some(line) = location.get("line").and_then(as_i64) {
            log_record.add_attribute(ATTRIBUTE_SOURCE_LINE, line);
        }
        Some(())
    });
    if let Some(request) = take_members(&mut record, KEY_HTTP_REQUEST, http_request_member) {
        log_record.add_attribute(ATTRIBUTE_HTTP_REQUEST, request);
    }
    if let Some(operation) = take_members(&mut record, KEY_OPERATION, operation_member) {
        log_record.add_attribute(ATTRIBUTE_OPERATION, operation);
    }
    take(&mut record, KEY_INSERT_ID, |value| {
        value
            .as_str()
            .map(|insert_id| log_record.add_attribute(ATTRIBUTE_INSERT_ID, insert_id.to_string()))
    });
    if let Some(exception) = exception {
        if let Some(kind) = &exception.kind {
            log_record.add_attribute(ATTRIBUTE_EXCEPTION_TYPE, kind.clone());
        }
        log_record.add_attribute(ATTRIBUTE_EXCEPTION_MESSAGE, exception.message.clone());
    }
    // Every attribute that maps to no typed field becomes a label.
    take(&mut record, KEY_LABELS, |value| {
        let Value::Object(labels) = value else {
            return None;
        };
        for (name, value) in labels {
            if let Some(value) = any_value(value) {
                log_record.add_attribute(name, value);
            }
        }
        Some(())
    });

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

/// Moves the members of the object under `key` that `member` accepts into one attribute value,
/// and leaves the others in `record` under `key`. A value that is not an object stays whole.
/// The Telemetry API rejects the whole log record for a member of the wrong type.
fn take_members(
    record: &mut Map<String, Value>,
    key: &str,
    member: fn(&str, &Value) -> Option<AnyValue>,
) -> Option<AnyValue> {
    let members = match record.remove(key)? {
        Value::Object(members) => members,
        other => {
            record.insert(key.to_string(), other);
            return None;
        }
    };
    let mut accepted = Vec::new();
    let mut rejected = Map::new();
    for (name, value) in members {
        match member(&name, &value) {
            Some(accepted_value) => accepted.push((Key::from(name), accepted_value)),
            None => {
                rejected.insert(name, value);
            }
        }
    }
    if !rejected.is_empty() {
        record.insert(key.to_string(), Value::Object(rejected));
    }
    (!accepted.is_empty()).then(|| accepted.into_iter().collect())
}

/// The `HttpRequest` member as the Telemetry API reads it; `None` for an unknown member or a
/// value of the wrong type. `latency` is a number of seconds, where the record may hold a
/// Duration string such as `125ms`.
fn http_request_member(name: &str, value: &Value) -> Option<AnyValue> {
    match name {
        "requestMethod" | "requestUrl" | "userAgent" | "remoteIp" | "serverIp" | "referer"
        | "protocol" => string_member(value),
        "status" => as_i64(value)
            .and_then(|status| i32::try_from(status).ok())
            .map(|status| AnyValue::Int(i64::from(status))),
        "requestSize" | "responseSize" | "cacheFillBytes" => as_i64(value).map(AnyValue::Int),
        "cacheLookup" | "cacheHit" | "cacheValidatedWithOriginServer" => {
            value.as_bool().map(AnyValue::Boolean)
        }
        "latency" => duration_seconds(value).map(AnyValue::Double),
        _ => None,
    }
}

/// The `LogEntryOperation` member as the Telemetry API reads it; `None` for an unknown member
/// or a value of the wrong type.
fn operation_member(name: &str, value: &Value) -> Option<AnyValue> {
    match name {
        "id" | "producer" => string_member(value),
        "first" | "last" => value.as_bool().map(AnyValue::Boolean),
        _ => None,
    }
}

fn string_member(value: &Value) -> Option<AnyValue> {
    value.as_str().map(|text| AnyValue::from(text.to_string()))
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
