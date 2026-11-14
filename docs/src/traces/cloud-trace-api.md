# Cloud Trace API exporter

The `trace-api` feature adds `trace_api::GcpCloudTraceExporterBuilder`, an exporter for the Cloud Trace API v2 (`cloudtrace.googleapis.com`).
It authenticates with ADC, see [Authentication](../authentication.md), and needs the `roles/cloudtrace.agent` role.

```rust,ignore
{{#include ../../../examples/traces/cloud_trace_api.rs:setup}}
```

`create_provider` and `create_provider_from_builder` build a tracer provider with a batch span processor, and `install(&provider)` returns a tracer.
`install` is synchronous.
`with_resource` sets the resource the exporter reports spans with.

Spans come from the OpenTelemetry API directly:

```rust,ignore
{{#include ../../../examples/traces/cloud_trace_api.rs:opentelemetry_api}}
```

or from `tracing`:

```rust,ignore
{{#include ../../../examples/traces/cloud_trace_api.rs:tracing}}
```

## Export timeout

`with_max_export_timeout` bounds how long a batch export may take before the batch is dropped. It defaults to the OpenTelemetry SDK default of 30 seconds, or `OTEL_BSP_EXPORT_TIMEOUT` when that is set.
Shutdown exports every batch queued ahead of it, one after the other, so it can take the timeout once per 512 queued spans, rounded up.
Keep that total below the time the platform allows after SIGTERM.

Shutdown blocks the calling thread until the batch task is done, so it needs a multi-threaded Tokio runtime, as for the [OTLP exporter](./otlp.md#shutting-down).

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/traces/cloud_trace_api.rs).
