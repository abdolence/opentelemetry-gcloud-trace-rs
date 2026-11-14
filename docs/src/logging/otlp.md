# OTLP

The `logs-otlp` feature adds a sink that sends entries as OpenTelemetry log records to the Telemetry API.
The same `GcpOtlpExporterBuilder` that exports the spans builds the logger provider, so one set of credentials and one endpoint serve both signals.

```rust,ignore
{{#include ../../../examples/logging/otlp.rs:setup}}
```

`with_otel_logger_provider` takes the provider and the `LogId` of the log the entries are written to, which becomes the `gcp.log.name` attribute.
Its `build()` is synchronous.
The caller owns the provider and flushes it by shutting it down.

## Shutting down

`shutdown`, `force_flush` and dropping the provider block the calling thread.
The batch processor exports on a thread of its own, but the connection is driven by the Tokio runtime, which needs a thread free to do it.
On a current-thread runtime they wait out their timeout and the queued records are not exported.
Call them on a multi-threaded runtime, or from a thread outside the runtime.

The processor reports a failed export as an `ERROR` `tracing` event with the `opentelemetry_sdk` target.
That target is on the list of [transport crates](./sinks.md) the network sinks skip, so a failing export does not log itself into another export.

`max_export_timeout` bounds how long an export may take, see [OTLP exporter](../traces/otlp.md#timeouts).

## Mapping

The Telemetry API maps an OTLP log record onto a `LogEntry`, see [OTLP log record to LogEntry mapping](https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/otlp-log-record-to-log-entry).
The sink follows it:

| Event                              | OTLP log record                                                                  | `LogEntry`                    |
|------------------------------------|----------------------------------------------------------------------------------|-------------------------------|
| level, or a `severity` field       | `severity_number` and `severity_text`                                            | `severity`                    |
| message and fields                 | body, a key-value list                                                           | `jsonPayload`                 |
| enclosing span                     | trace ID, span ID and sampled flag                                               | `trace`, `spanId`, `traceSampled` |
| `http_request.*`                   | `gcp.http_request`; `latency` as seconds                                         | `httpRequest`                 |
| `operation.*`                      | `gcp.operation`                                                                  | `operation`                   |
| source location                    | `code.file.path` and `code.line.number`                                          | `sourceLocation`              |
| `insert_id`                        | `gcp.insert_id`                                                                  | `insertId`                    |
| `labels.*`                         | one attribute per label                                                          | `labels`                      |
| `LogId`                            | `gcp.log.name`                                                                   | `logName`                     |

The mapping page names the line attribute `code.function.number`, a typo: Google reads `code.line.number`, and that is what the sink sets.

Attributes that map to no typed field become labels.
The resource of the provider carries `gcp.project_id`, and `service.name` when set.
See [Comparison with the OpenTelemetry tracing bridge](./otlp-bridge.md) for entries side by side.

Entries sent over OTLP store `trace` as the bare trace ID rather than `projects/PROJECT_ID/traces/TRACE_ID`, so filter them with `trace:"TRACE_ID"`, which matches both forms.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/logging/otlp.rs).
