//! Turns one `tracing` event into the `serde_json::Map` a Cloud Logging
//! structured JSON line, or a future `LogEntry`, is built from.
//!
//! Kept independent of `tracing_subscriber::Layer` and of any particular
//! sink: the JSON sink serialises the map directly, and a Cloud Logging API
//! sink can convert the same map into a `LogEntry` without duplicating any of
//! the field mapping below.

use serde_json::{Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::field::{Field, Visit};
use tracing::{Event, Level};

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
    pub(crate) trace_id: String,
    pub(crate) span_id: String,
    pub(crate) sampled: bool,
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
}

impl CollectedFields {
    fn insert_named(&mut self, name: &str, value: Value) {
        if name == "message" {
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
        self.insert_named(field.name(), Value::from(value.to_string()));
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

/// Assembles the log record for one event. `correlation` is `None` when the
/// event has no enclosing span, when the enclosing span carries no valid
/// OpenTelemetry context, or when no OpenTelemetry layer produced one.
pub(crate) fn build_log_record(
    event: &Event<'_>,
    project_id: &str,
    with_source_location: bool,
    correlation: Option<&TraceCorrelation>,
) -> Map<String, Value> {
    let metadata = event.metadata();
    let mut fields = CollectedFields::default();
    event.record(&mut fields);

    let mut map = Map::new();
    map.insert("time".to_string(), Value::from(now_rfc3339()));
    map.insert(
        "severity".to_string(),
        Value::from(resolve_severity(
            metadata.level(),
            fields.severity_override.as_deref(),
        )),
    );
    map.insert(
        "message".to_string(),
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
                "logging.googleapis.com/sourceLocation".to_string(),
                Value::Object(source_location),
            );
        }
    }

    if let Some(correlation) = correlation {
        map.insert(
            "logging.googleapis.com/trace".to_string(),
            Value::from(format!(
                "projects/{project_id}/traces/{}",
                correlation.trace_id
            )),
        );
        map.insert(
            "logging.googleapis.com/spanId".to_string(),
            Value::from(correlation.span_id.clone()),
        );
        if correlation.sampled {
            map.insert(
                "logging.googleapis.com/trace_sampled".to_string(),
                Value::Bool(true),
            );
        }
    }

    for (key, value) in fields.user_fields {
        map.insert(key, value);
    }

    if !fields.http_request.is_empty() {
        map.insert(
            "httpRequest".to_string(),
            Value::Object(fields.http_request),
        );
    }
    if !fields.labels.is_empty() {
        map.insert(
            "logging.googleapis.com/labels".to_string(),
            Value::Object(fields.labels),
        );
    }
    if !fields.operation.is_empty() {
        map.insert(
            "logging.googleapis.com/operation".to_string(),
            Value::Object(fields.operation),
        );
    }
    if let Some(insert_id) = fields.insert_id {
        map.insert("logging.googleapis.com/insertId".to_string(), insert_id);
    }

    map
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
