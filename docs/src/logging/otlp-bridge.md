# Comparison with the OpenTelemetry tracing bridge

`opentelemetry-appender-tracing` is the stock bridge from `tracing` events to OpenTelemetry log records.
Sent to the Telemetry API, its records and those of the [OTLP sink](./otlp.md) reach Cloud Logging differently, because the sink maps the conventional fields onto typed `LogEntry` fields before sending.

The same event, as the bridge sends it, trimmed:

```json
{
  "logName": "projects/my-project/logs/event%20src%2Fbin%2Fbridge.rs%3A26",
  "severity": "INFO",
  "textPayload": "bridge probe checkout request received",
  "labels": {
    "http_request.request_method": "POST",
    "http_request.request_url": "https://example.test/orders/order-42/checkout",
    "http_request.status": "200",
    "labels.tenant": "acme"
  },
  "trace": "b5c11cf3e9cb0d1710251307eaaed739",
  "spanId": "49f19eb158811ff5",
  "traceSampled": true
}
```

and as the OTLP sink sends it, trimmed:

```json
{
  "logName": "projects/my-project/logs/gcloud-observability-example",
  "severity": "INFO",
  "jsonPayload": {
    "message": "checkout request received",
    "target": "logging_otlp"
  },
  "httpRequest": {
    "requestMethod": "POST",
    "requestUrl": "https://example.test/orders/order-42/checkout",
    "status": 200,
    "latency": "0.125s"
  },
  "labels": {
    "gcp.log.name": "gcloud-observability-example",
    "tenant": "acme"
  },
  "sourceLocation": { "file": "examples/logging/otlp.rs", "line": "78" },
  "insertId": "1noy1uke8wh62",
  "trace": "b3f6d198940b478f16da57f0c19e491b",
  "spanId": "cbdd640d48c893e0",
  "traceSampled": true
}
```

The differences:

- the bridge writes a `textPayload` with the message only; the sink writes a `jsonPayload` with the message, the target and every other field with its type;
- the bridge sends every field as a string label, and the `http_request.*` fields stay labels; the sink fills the typed `httpRequest` and `sourceLocation`, and an `operation` and an `insertId` when the event has them;
- the bridge's `logName` is made per call site; the sink writes every entry to the one log named by its `LogId`;
- both entries carry the trace and span of the enclosing span.
