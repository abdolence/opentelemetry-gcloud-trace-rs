//! Turns the shared log record produced by [`crate::logs::format`] into a
//! Cloud Logging `LogEntry`.
//!
//! Google gives a handful of JSON keys a dedicated `LogEntry` field and treats
//! everything else as free-form payload. Anything this module cannot map onto
//! its typed field is left in `json_payload` under the key it arrived with,
//! rather than dropped: a malformed `latency` or an unrecognised `httpRequest`
//! member should cost the reader a typed field, never the value itself.

use gcloud_sdk::google::logging::r#type::{HttpRequest, LogSeverity};
use gcloud_sdk::google::logging::v2::{
    log_entry, LogEntry, LogEntryOperation, LogEntrySourceLocation,
};
use gcloud_sdk::prost_types;
use serde_json::{Map, Value};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const KEY_TIME: &str = "time";
const KEY_SEVERITY: &str = "severity";
const KEY_TRACE: &str = "logging.googleapis.com/trace";
const KEY_SPAN_ID: &str = "logging.googleapis.com/spanId";
const KEY_TRACE_SAMPLED: &str = "logging.googleapis.com/trace_sampled";
const KEY_SOURCE_LOCATION: &str = "logging.googleapis.com/sourceLocation";
const KEY_LABELS: &str = "logging.googleapis.com/labels";
const KEY_INSERT_ID: &str = "logging.googleapis.com/insertId";
const KEY_OPERATION: &str = "logging.googleapis.com/operation";
const KEY_HTTP_REQUEST: &str = "httpRequest";

/// Moves every key Cloud Logging gives a dedicated `LogEntry` field out of the
/// record and leaves the rest as the entry's `json_payload`.
pub(crate) fn build_log_entry(mut record: Map<String, Value>) -> LogEntry {
    let mut entry = LogEntry::default();

    take(&mut record, KEY_TIME, |value| {
        parse_timestamp(&value).map(|timestamp| entry.timestamp = Some(timestamp))
    });
    take(&mut record, KEY_SEVERITY, |value| {
        value
            .as_str()
            .and_then(LogSeverity::from_str_name)
            .map(|severity| entry.severity = severity as i32)
    });
    take(&mut record, KEY_TRACE, |value| {
        as_string(&value).map(|trace| entry.trace = trace)
    });
    take(&mut record, KEY_SPAN_ID, |value| {
        as_string(&value).map(|span_id| entry.span_id = span_id)
    });
    take(&mut record, KEY_TRACE_SAMPLED, |value| {
        value.as_bool().map(|sampled| entry.trace_sampled = sampled)
    });
    take(&mut record, KEY_INSERT_ID, |value| {
        as_string(&value).map(|insert_id| entry.insert_id = insert_id)
    });
    take(&mut record, KEY_SOURCE_LOCATION, |value| {
        build_source_location(&value).map(|location| entry.source_location = Some(location))
    });
    take(&mut record, KEY_OPERATION, |value| {
        build_operation(&value).map(|operation| entry.operation = Some(operation))
    });
    take(&mut record, KEY_LABELS, |value| match value {
        Value::Object(labels) => {
            entry.labels = labels
                .into_iter()
                .map(|(key, value)| {
                    let value = as_string(&value).unwrap_or_else(|| value.to_string());
                    (key, value)
                })
                .collect();
            Some(())
        }
        _ => None,
    });

    if let Some(value) = record.remove(KEY_HTTP_REQUEST) {
        let (request, unmapped) = build_http_request(value);
        entry.http_request = request;
        if let Some(unmapped) = unmapped {
            record.insert(KEY_HTTP_REQUEST.to_string(), unmapped);
        }
    }

    entry.payload = Some(log_entry::Payload::JsonPayload(prost_types::Struct {
        fields: record
            .into_iter()
            .map(|(key, value)| (key, json_to_prost(value)))
            .collect(),
    }));
    entry
}

/// Removes `key` and applies `map`, putting the value back untouched when it
/// does not fit the proto field it was destined for, so a malformed value
/// still reaches Cloud Logging inside the payload.
fn take<F>(record: &mut Map<String, Value>, key: &str, map: F)
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

fn as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

/// Accepts both the JSON number and the decimal string proto3 uses for 64-bit
/// integers, which exceed what a JSON number can hold exactly.
fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(raw) => raw.trim().parse().ok(),
        _ => None,
    }
}

fn parse_timestamp(value: &Value) -> Option<prost_types::Timestamp> {
    let raw = value.as_str()?;
    let moment = OffsetDateTime::parse(raw, &Rfc3339).ok()?;
    Some(prost_types::Timestamp {
        seconds: moment.unix_timestamp(),
        nanos: moment.nanosecond() as i32,
    })
}

fn build_source_location(value: &Value) -> Option<LogEntrySourceLocation> {
    let fields = value.as_object()?;
    Some(LogEntrySourceLocation {
        file: fields.get("file").and_then(as_string).unwrap_or_default(),
        line: fields.get("line").and_then(as_i64).unwrap_or_default(),
        function: fields
            .get("function")
            .and_then(as_string)
            .unwrap_or_default(),
    })
}

fn build_operation(value: &Value) -> Option<LogEntryOperation> {
    let fields = value.as_object()?;
    Some(LogEntryOperation {
        id: fields.get("id").and_then(as_string).unwrap_or_default(),
        producer: fields
            .get("producer")
            .and_then(as_string)
            .unwrap_or_default(),
        first: fields
            .get("first")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        last: fields.get("last").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Splits an `httpRequest` object into the typed proto and whatever could not
/// be mapped onto it, which the caller keeps in the payload.
fn build_http_request(value: Value) -> (Option<HttpRequest>, Option<Value>) {
    let Value::Object(fields) = value else {
        return (None, Some(value));
    };

    let mut request = HttpRequest::default();
    let mut unmapped = Map::new();
    let mut mapped_any = false;

    for (key, value) in fields {
        let mapped = match key.as_str() {
            "requestMethod" => assign(&mut request.request_method, as_string(&value)),
            "requestUrl" => assign(&mut request.request_url, as_string(&value)),
            "userAgent" => assign(&mut request.user_agent, as_string(&value)),
            "remoteIp" => assign(&mut request.remote_ip, as_string(&value)),
            "serverIp" => assign(&mut request.server_ip, as_string(&value)),
            "referer" => assign(&mut request.referer, as_string(&value)),
            "protocol" => assign(&mut request.protocol, as_string(&value)),
            "status" => assign(
                &mut request.status,
                as_i64(&value).and_then(|status| i32::try_from(status).ok()),
            ),
            "requestSize" => assign(&mut request.request_size, as_i64(&value)),
            "responseSize" => assign(&mut request.response_size, as_i64(&value)),
            "cacheFillBytes" => assign(&mut request.cache_fill_bytes, as_i64(&value)),
            "cacheLookup" => assign(&mut request.cache_lookup, value.as_bool()),
            "cacheHit" => assign(&mut request.cache_hit, value.as_bool()),
            "cacheValidatedWithOriginServer" => assign(
                &mut request.cache_validated_with_origin_server,
                value.as_bool(),
            ),
            "latency" => assign(&mut request.latency, parse_duration(&value).map(Some)),
            _ => false,
        };
        if mapped {
            mapped_any = true;
        } else {
            unmapped.insert(key, value);
        }
    }

    (
        mapped_any.then_some(request),
        (!unmapped.is_empty()).then_some(Value::Object(unmapped)),
    )
}

fn assign<T>(field: &mut T, parsed: Option<T>) -> bool {
    match parsed {
        Some(value) => {
            *field = value;
            true
        }
        None => false,
    }
}

/// Google writes `HttpRequest.latency` as a Duration string; the suffixes are
/// tried longest first so `ms` is not read as `s`.
fn parse_duration(value: &Value) -> Option<prost_types::Duration> {
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
    if !seconds.is_finite() {
        return None;
    }

    let total_nanos = (seconds * 1e9).round();
    let whole_seconds = (total_nanos / 1e9).trunc();
    Some(prost_types::Duration {
        seconds: whole_seconds as i64,
        nanos: (total_nanos - whole_seconds * 1e9) as i32,
    })
}

fn json_to_prost(value: Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match value {
        Value::Null => Kind::NullValue(0),
        Value::Bool(value) => Kind::BoolValue(value),
        // Cloud Logging's payload is a protobuf `Struct`, whose only numeric
        // kind is a double; an integer beyond 2^53 is kept as a string rather
        // than silently rounded.
        Value::Number(number) => match number.as_f64() {
            Some(number) => Kind::NumberValue(number),
            None => Kind::StringValue(number.to_string()),
        },
        Value::String(value) => Kind::StringValue(value),
        Value::Array(values) => Kind::ListValue(prost_types::ListValue {
            values: values.into_iter().map(json_to_prost).collect(),
        }),
        Value::Object(fields) => Kind::StructValue(prost_types::Struct {
            fields: fields
                .into_iter()
                .map(|(key, value)| (key, json_to_prost(value)))
                .collect(),
        }),
    };
    prost_types::Value { kind: Some(kind) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("test record must be a JSON object, got {other}"),
        }
    }

    fn payload(entry: &LogEntry) -> &prost_types::Struct {
        match entry.payload.as_ref() {
            Some(log_entry::Payload::JsonPayload(payload)) => payload,
            other => panic!("expected a json payload, got {other:?}"),
        }
    }

    fn prost_value(kind: prost_types::value::Kind) -> prost_types::Value {
        prost_types::Value { kind: Some(kind) }
    }

    fn payload_string(entry: &LogEntry, key: &str) -> String {
        match payload(entry).fields.get(key).and_then(|v| v.kind.as_ref()) {
            Some(prost_types::value::Kind::StringValue(value)) => value.clone(),
            other => panic!("expected a string at {key}, got {other:?}"),
        }
    }

    #[test]
    fn rfc3339_time_becomes_the_entry_timestamp() {
        let entry = build_log_entry(record(json!({
            "time": "2026-09-09T12:34:56.5Z",
            "message": "hello",
        })));
        let timestamp = entry.timestamp.expect("timestamp is set from `time`");
        assert_eq!(timestamp.seconds, 1_788_957_296);
        assert_eq!(timestamp.nanos, 500_000_000);
        assert!(!payload(&entry).fields.contains_key("time"));
    }

    #[test]
    fn unparseable_time_stays_in_the_payload() {
        let entry = build_log_entry(record(json!({ "time": "not a timestamp" })));
        assert!(entry.timestamp.is_none());
        assert_eq!(payload_string(&entry, "time"), "not a timestamp");
    }

    #[test]
    fn severity_name_becomes_the_severity_enum() {
        let entry = build_log_entry(record(json!({ "severity": "WARNING" })));
        assert_eq!(entry.severity, LogSeverity::Warning as i32);
        assert!(!payload(&entry).fields.contains_key("severity"));
    }

    #[test]
    fn unknown_severity_stays_in_the_payload() {
        let entry = build_log_entry(record(json!({ "severity": "LOUD" })));
        assert_eq!(entry.severity, LogSeverity::Default as i32);
        assert_eq!(payload_string(&entry, "severity"), "LOUD");
    }

    #[test]
    fn trace_correlation_keys_become_typed_fields() {
        let entry = build_log_entry(record(json!({
            "logging.googleapis.com/trace": "projects/p/traces/0af7651916cd43dd8448eb211c80319c",
            "logging.googleapis.com/spanId": "b7ad6b7169203331",
            "logging.googleapis.com/trace_sampled": true,
        })));
        assert_eq!(
            entry.trace,
            "projects/p/traces/0af7651916cd43dd8448eb211c80319c"
        );
        assert_eq!(entry.span_id, "b7ad6b7169203331");
        assert!(entry.trace_sampled);
        assert!(payload(&entry).fields.is_empty());
    }

    #[test]
    fn source_location_line_is_parsed_from_its_string() {
        let entry = build_log_entry(record(json!({
            "logging.googleapis.com/sourceLocation": {
                "file": "src/main.rs",
                "line": "42",
                "function": "main",
            },
        })));
        let location = entry.source_location.expect("source location is mapped");
        assert_eq!(location.file, "src/main.rs");
        assert_eq!(location.line, 42);
        assert_eq!(location.function, "main");
    }

    #[test]
    fn labels_become_the_string_label_map() {
        let entry = build_log_entry(record(json!({
            "logging.googleapis.com/labels": { "tenant": "acme", "shard": "7" },
        })));
        assert_eq!(entry.labels.get("tenant").map(String::as_str), Some("acme"));
        assert_eq!(entry.labels.get("shard").map(String::as_str), Some("7"));
    }

    #[test]
    fn insert_id_and_operation_become_typed_fields() {
        let entry = build_log_entry(record(json!({
            "logging.googleapis.com/insertId": "unique-1",
            "logging.googleapis.com/operation": {
                "id": "op-1", "producer": "svc", "first": true, "last": false,
            },
        })));
        assert_eq!(entry.insert_id, "unique-1");
        let operation = entry.operation.expect("operation is mapped");
        assert_eq!(operation.id, "op-1");
        assert_eq!(operation.producer, "svc");
        assert!(operation.first);
        assert!(!operation.last);
    }

    #[test]
    fn http_request_fields_are_parsed_into_their_proto_types() {
        let entry = build_log_entry(record(json!({
            "httpRequest": {
                "requestMethod": "GET",
                "requestUrl": "https://example.test/a",
                "userAgent": "curl/8",
                "remoteIp": "10.0.0.1",
                "serverIp": "10.0.0.2",
                "referer": "https://example.test/",
                "protocol": "HTTP/1.1",
                "status": 200,
                "requestSize": "512",
                "responseSize": 1024,
                "cacheFillBytes": "64",
                "cacheLookup": true,
                "cacheHit": false,
                "cacheValidatedWithOriginServer": true,
                "latency": "1.5s",
            },
        })));
        let request = entry.http_request.clone().expect("http request is mapped");
        assert_eq!(request.request_method, "GET");
        assert_eq!(request.request_url, "https://example.test/a");
        assert_eq!(request.user_agent, "curl/8");
        assert_eq!(request.remote_ip, "10.0.0.1");
        assert_eq!(request.server_ip, "10.0.0.2");
        assert_eq!(request.referer, "https://example.test/");
        assert_eq!(request.protocol, "HTTP/1.1");
        assert_eq!(request.status, 200);
        assert_eq!(request.request_size, 512);
        assert_eq!(request.response_size, 1024);
        assert_eq!(request.cache_fill_bytes, 64);
        assert!(request.cache_lookup);
        assert!(!request.cache_hit);
        assert!(request.cache_validated_with_origin_server);
        let latency = request.latency.expect("latency is mapped");
        assert_eq!(latency.seconds, 1);
        assert_eq!(latency.nanos, 500_000_000);
        assert!(!payload(&entry).fields.contains_key("httpRequest"));
    }

    #[test]
    fn latency_accepts_millisecond_and_numeric_forms() {
        for (given, seconds, nanos) in [
            (json!("250ms"), 0, 250_000_000),
            (json!("2m"), 120, 0),
            (json!(0.5), 0, 500_000_000),
        ] {
            let entry = build_log_entry(record(json!({
                "httpRequest": { "latency": given },
            })));
            let latency = entry
                .http_request
                .clone()
                .and_then(|request| request.latency)
                .expect("latency is mapped");
            assert_eq!((latency.seconds, latency.nanos), (seconds, nanos));
        }
    }

    #[test]
    fn unmappable_http_request_members_stay_in_the_payload() {
        let entry = build_log_entry(record(json!({
            "httpRequest": {
                "requestMethod": "POST",
                "latency": "soon",
                "somethingElse": 3,
            },
        })));
        let request = entry.http_request.clone().expect("http request is mapped");
        assert_eq!(request.request_method, "POST");
        assert!(request.latency.is_none());

        let kept = match payload(&entry)
            .fields
            .get("httpRequest")
            .and_then(|value| value.kind.as_ref())
        {
            Some(prost_types::value::Kind::StructValue(kept)) => kept.clone(),
            other => panic!("expected the unmapped members as a struct, got {other:?}"),
        };
        assert_eq!(kept.fields.len(), 2);
        assert!(kept.fields.contains_key("latency"));
        assert!(kept.fields.contains_key("somethingElse"));
    }

    #[test]
    fn remaining_fields_round_trip_through_the_json_payload() {
        let entry = build_log_entry(record(json!({
            "message": "hello",
            "target": "my_app::worker",
            "nested": { "list": [1, "two", true, null], "inner": { "deep": 1.5 } },
        })));
        assert_eq!(payload_string(&entry, "message"), "hello");
        assert_eq!(payload_string(&entry, "target"), "my_app::worker");

        let nested = payload(&entry)
            .fields
            .get("nested")
            .cloned()
            .expect("nested value is kept");
        let expected = prost_value(prost_types::value::Kind::StructValue(prost_types::Struct {
            fields: [
                (
                    "list".to_string(),
                    prost_value(prost_types::value::Kind::ListValue(
                        prost_types::ListValue {
                            values: vec![
                                prost_value(prost_types::value::Kind::NumberValue(1.0)),
                                prost_value(prost_types::value::Kind::StringValue(
                                    "two".to_string(),
                                )),
                                prost_value(prost_types::value::Kind::BoolValue(true)),
                                prost_value(prost_types::value::Kind::NullValue(0)),
                            ],
                        },
                    )),
                ),
                (
                    "inner".to_string(),
                    prost_value(prost_types::value::Kind::StructValue(prost_types::Struct {
                        fields: [(
                            "deep".to_string(),
                            prost_value(prost_types::value::Kind::NumberValue(1.5)),
                        )]
                        .into_iter()
                        .collect(),
                    })),
                ),
            ]
            .into_iter()
            .collect(),
        }));
        assert_eq!(nested, expected);
    }
}
