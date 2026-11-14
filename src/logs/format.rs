//! Turns one `tracing` event into the `serde_json::Map` every sink builds its
//! entry from.
//!
//! Kept independent of `tracing_subscriber::Layer` and of any particular
//! sink: the JSON sink serialises the map directly, and the Cloud Logging API
//! sink converts the same map into a `LogEntry`, so the field mapping below
//! exists once.

use super::error_reporting::{error_chain, ExceptionDetails, ServiceContext};
use opentelemetry::trace::{SpanId, TraceId};
use serde_json::{Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::field::{Field, Visit};
use tracing::{Event, Level};

// Keys of the record that have a typed counterpart in each sink's entry; every other
// key is user payload.
pub(crate) const KEY_TIME: &str = "time";
pub(crate) const KEY_SEVERITY: &str = "severity";
pub(crate) const KEY_TRACE: &str = "logging.googleapis.com/trace";
pub(crate) const KEY_SPAN_ID: &str = "logging.googleapis.com/spanId";
pub(crate) const KEY_TRACE_SAMPLED: &str = "logging.googleapis.com/trace_sampled";
pub(crate) const KEY_SOURCE_LOCATION: &str = "logging.googleapis.com/sourceLocation";
pub(crate) const KEY_LABELS: &str = "logging.googleapis.com/labels";
pub(crate) const KEY_INSERT_ID: &str = "logging.googleapis.com/insertId";
pub(crate) const KEY_OPERATION: &str = "logging.googleapis.com/operation";
pub(crate) const KEY_HTTP_REQUEST: &str = "httpRequest";
pub(crate) const KEY_MESSAGE: &str = "message";

/// LogSeverity enum names Cloud Logging recognises, used to match a
/// `severity` event field case-insensitively before falling back to the
/// level-based mapping.
const LOG_SEVERITY_NAMES: [&str; 9] = [
    "DEFAULT",
    "DEBUG",
    "INFO",
    "NOTICE",
    "WARNING",
    "ERROR",
    "CRITICAL",
    "ALERT",
    "EMERGENCY",
];

/// A trace context already resolved and validated by the caller: format.rs
/// has no access to the tracing registry, so it never decides on its own
/// whether correlation applies.
pub(crate) struct TraceCorrelation {
    pub(crate) trace_id: TraceId,
    pub(crate) span_id: SpanId,
    pub(crate) sampled: bool,
}

/// Accepts both the JSON number and the decimal string proto3 uses for 64-bit
/// integers, which exceed what a JSON number can hold exactly.
pub(crate) fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(raw) => raw.trim().parse().ok(),
        _ => None,
    }
}

/// Removes `key` and applies `map`, putting the value back untouched when it does not fit the
/// typed field it was destined for, so a malformed value still reaches the sink's payload.
#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
pub(crate) fn take<F>(record: &mut Map<String, Value>, key: &str, map: F)
where
    F: FnOnce(Value) -> Option<()>,
{
    let Some(value) = record.remove(key) else {
        return;
    };
    let restore = value.clone();
    if map(value).is_none() {
        record.insert(key.to_string(), restore);
    }
}

/// A latency as seconds, from a JSON number of seconds or a Duration string with a unit suffix
/// such as `125ms`. The suffixes are tried longest first so `ms` is not read as `s`.
#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
pub(crate) fn duration_seconds(value: &Value) -> Option<f64> {
    const UNITS: [(&str, f64); 7] = [
        ("ns", 1e-9),
        ("us", 1e-6),
        ("\u{b5}s", 1e-6),
        ("ms", 1e-3),
        ("s", 1.0),
        ("m", 60.0),
        ("h", 3600.0),
    ];

    let seconds = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(raw) => {
            let raw = raw.trim();
            UNITS.iter().find_map(|(suffix, scale)| {
                let number = raw.strip_suffix(suffix)?;
                Some(number.trim().parse::<f64>().ok()? * scale)
            })?
        }
        _ => return None,
    };
    seconds.is_finite().then_some(seconds)
}

fn level_severity(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARNING",
        Level::INFO => "INFO",
        Level::DEBUG | Level::TRACE => "DEBUG",
    }
}

fn resolve_severity(level: &Level, override_value: Option<&str>) -> &'static str {
    override_value
        .and_then(|raw| {
            LOG_SEVERITY_NAMES
                .iter()
                .find(|name| name.eq_ignore_ascii_case(raw))
        })
        .copied()
        .unwrap_or_else(|| level_severity(level))
}

/// `request_method` becomes `requestMethod`; Google's `HttpRequest` proto is
/// the only place arbitrary event field names are camel-cased.
fn snake_to_camel(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper_next = false;
    for ch in input.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn stringify(value: Value) -> Value {
    match value {
        Value::String(_) => value,
        other => Value::String(other.to_string()),
    }
}

#[derive(Default)]
struct CollectedFields {
    message: Option<Value>,
    severity_override: Option<String>,
    http_request: Map<String, Value>,
    labels: Map<String, Value>,
    operation: Map<String, Value>,
    insert_id: Option<Value>,
    user_fields: Map<String, Value>,
    first_error_chain: Option<Vec<String>>,
}

impl CollectedFields {
    fn insert_named(&mut self, name: &str, value: Value) {
        if name == KEY_MESSAGE {
            self.message = Some(value);
        } else if name == "severity" {
            if let Value::String(raw) = value {
                self.severity_override = Some(raw);
            }
        } else if let Some(key) = name.strip_prefix("http_request.") {
            self.http_request.insert(snake_to_camel(key), value);
        } else if let Some(key) = name.strip_prefix("labels.") {
            self.labels.insert(key.to_string(), stringify(value));
        } else if let Some(key) = name.strip_prefix("operation.") {
            self.operation.insert(key.to_string(), value);
        } else if name == "insert_id" {
            self.insert_id = Some(value);
        } else {
            self.user_fields.insert(name.to_string(), value);
        }
    }
}

impl Visit for CollectedFields {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.insert_named(field.name(), Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert_named(field.name(), Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert_named(field.name(), Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert_named(field.name(), Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert_named(field.name(), Value::from(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        let chain = error_chain(value);
        self.insert_named(field.name(), Value::from(chain.join("\nCaused by: ")));
        self.first_error_chain.get_or_insert(chain);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.insert_named(field.name(), Value::from(format!("{value:?}")));
    }
}

fn now_rfc3339() -> String {
    // `Rfc3339` formatting only fails for years outside the calendar's
    // four-digit range, which the current time never is.
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// The record built for one event, and what only some sinks use of it.
pub(crate) struct EventRecord {
    pub(crate) fields: Map<String, Value>,
    /// Set for an Error Reporting event; the OTLP sink sends it as attributes.
    // Read only by the OTLP sink.
    #[cfg_attr(not(feature = "logs-otlp"), allow(dead_code))]
    pub(crate) exception: Option<ExceptionDetails>,
}

/// Whether a severity name is ERROR or above, which is what Error Reporting evaluates.
fn is_error_severity(severity: &str) -> bool {
    matches!(severity, "ERROR" | "CRITICAL" | "ALERT" | "EMERGENCY")
}

/// Assembles the log record for one event. `correlation` is `None` when the
/// event has no enclosing span, when the enclosing span carries no valid
/// OpenTelemetry context, or when no OpenTelemetry layer produced one.
/// `error_reporting` turns an event at ERROR severity or above into an Error
/// Reporting event.
pub(crate) fn build_log_record(
    event: &Event<'_>,
    project_id: &str,
    with_source_location: bool,
    correlation: Option<&TraceCorrelation>,
    error_reporting: Option<&ServiceContext>,
) -> EventRecord {
    let metadata = event.metadata();
    let mut fields = CollectedFields::default();
    event.record(&mut fields);

    let severity = resolve_severity(metadata.level(), fields.severity_override.as_deref());
    let mut map = Map::new();
    map.insert(KEY_TIME.to_string(), Value::from(now_rfc3339()));
    map.insert(KEY_SEVERITY.to_string(), Value::from(severity));
    map.insert(
        KEY_MESSAGE.to_string(),
        fields
            .message
            .take()
            .unwrap_or_else(|| Value::from(metadata.name())),
    );
    map.insert("target".to_string(), Value::from(metadata.target()));

    if with_source_location {
        let mut source_location = Map::new();
        if let Some(file) = metadata.file() {
            source_location.insert("file".to_string(), Value::from(file));
        }
        if let Some(line) = metadata.line() {
            source_location.insert("line".to_string(), Value::from(line.to_string()));
        }
        if !source_location.is_empty() {
            map.insert(
                KEY_SOURCE_LOCATION.to_string(),
                Value::Object(source_location),
            );
        }
    }

    if let Some(correlation) = correlation {
        map.insert(
            KEY_TRACE.to_string(),
            Value::from(format!(
                "projects/{project_id}/traces/{}",
                correlation.trace_id
            )),
        );
        map.insert(
            KEY_SPAN_ID.to_string(),
            Value::from(correlation.span_id.to_string()),
        );
        if correlation.sampled {
            map.insert(KEY_TRACE_SAMPLED.to_string(), Value::Bool(true));
        }
    }

    for (key, value) in fields.user_fields {
        map.insert(key, value);
    }

    if !fields.http_request.is_empty() {
        map.insert(
            KEY_HTTP_REQUEST.to_string(),
            Value::Object(fields.http_request),
        );
    }
    if !fields.labels.is_empty() {
        map.insert(KEY_LABELS.to_string(), Value::Object(fields.labels));
    }
    if !fields.operation.is_empty() {
        map.insert(KEY_OPERATION.to_string(), Value::Object(fields.operation));
    }
    if let Some(insert_id) = fields.insert_id {
        map.insert(KEY_INSERT_ID.to_string(), insert_id);
    }

    let exception = error_reporting
        .filter(|_| is_error_severity(severity))
        .map(|service| service.report(&mut map, metadata, fields.first_error_chain.as_deref()));

    EventRecord {
        fields: map,
        exception,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_override_matches_case_insensitively() {
        assert_eq!(resolve_severity(&Level::INFO, Some("warning")), "WARNING");
        assert_eq!(resolve_severity(&Level::INFO, Some("WARNING")), "WARNING");
    }

    #[test]
    fn severity_override_falls_back_on_unknown_string() {
        assert_eq!(
            resolve_severity(&Level::ERROR, Some("not-a-severity")),
            "ERROR"
        );
    }

    #[test]
    fn severity_maps_trace_and_debug_to_debug() {
        assert_eq!(level_severity(&Level::TRACE), "DEBUG");
        assert_eq!(level_severity(&Level::DEBUG), "DEBUG");
    }

    #[test]
    fn snake_to_camel_converts_http_request_keys() {
        assert_eq!(snake_to_camel("request_method"), "requestMethod");
        assert_eq!(snake_to_camel("status"), "status");
    }
}
