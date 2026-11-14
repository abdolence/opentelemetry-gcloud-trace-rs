# The logging layer

`GcpCloudLoggingLayer` is a `tracing-subscriber` layer. It turns every `tracing` event into one Cloud Logging entry, with these parts:

- `severity`, from the event level;
- `message` and `target`, and every other event field as the entry's `jsonPayload`, Cloud Logging's structured payload;
- `logging.googleapis.com/sourceLocation`, the file and line of the event;
- `logging.googleapis.com/trace`, `spanId` and `trace_sampled`, taken from the OpenTelemetry span the event happened in, which link the entry to its span in the console;
- `httpRequest`, labels and an operation, from [conventional field names](./fields.md).

A JSON line from the default sink looks like this:

```json
{
  "time": "2026-11-14T16:21:43.008982Z",
  "severity": "INFO",
  "message": "checkout request received",
  "target": "orders",
  "logging.googleapis.com/sourceLocation": { "file": "src/orders.rs", "line": "78" },
  "logging.googleapis.com/trace": "projects/my-project/traces/b3f6d198940b478f16da57f0c19e491b",
  "logging.googleapis.com/spanId": "cbdd640d48c893e0",
  "logging.googleapis.com/trace_sampled": true,
  "order_id": "order-42"
}
```

## Building the layer

```rust,ignore
let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id).build()?;
```

`build()` returns an error when the project ID is empty or contains `/` or a control character: `InvalidParametersError` with the field `google_project_id`.
`GcpCloudLoggingLayerBuilder::for_default_project_id()` takes the project of the environment the process runs in.
`with_source_location(false)` leaves `sourceLocation` out.

A layer has exactly one [sink](./sinks.md), chosen when it is built.

## Layer ordering

Register `tracing_opentelemetry::layer()` before the logging layer.
A layer sees only the span context attached by layers registered ahead of it, so registering the logging layer first, or leaving the OpenTelemetry layer out, drops `trace`, `spanId` and `traceSampled` from every entry.
The logging layer prints one warning to stderr the first time this happens.

```rust,ignore
{{#include ../../../examples/logging/dev_and_prod.rs:layers}}
```

The layer reads the context through the public `tracing_opentelemetry::get_otel_context` API.
Wrapping the layer in `Option`, `Box<dyn Layer>` or a per-layer filter needs no extra step.
Under a scoped subscriber (`tracing::subscriber::with_default`) with the layer wrapped that way, the entry is correlated with the span entered on the thread, so an explicit `parent:` on an event is not honoured. A global subscriber has no such limit.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/logging/dev_and_prod.rs).
