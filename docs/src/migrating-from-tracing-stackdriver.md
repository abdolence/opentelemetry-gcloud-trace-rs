# Migrating from tracing-stackdriver

Swap the `tracing-stackdriver` layer for `GcpCloudLoggingLayer` at the same position in the subscriber stack:

```diff
- .with(tracing_stackdriver::layer().with_cloud_trace(
-     tracing_stackdriver::CloudTraceConfiguration { project_id },
- ))
+ .with(gcloud_observability::logs::GcpCloudLoggingLayerBuilder::new(project_id).build()?)
```

The layer reads the trace and span IDs through the public `tracing-opentelemetry` API, so register `tracing_opentelemetry::layer()` before it, see [The logging layer](./logging/layer.md#layer-ordering).

Two things change in the emitted log lines:

- there is no `span` or `spans` object carrying the enclosing span's name and fields: the span attributes live on the trace, and are not repeated into every log line;
- event field names are written as given, not camel-cased; only the `httpRequest.*` keys are camel-cased, matching Google's own field names, see [Field conventions](./logging/fields.md).

A layer has one [sink](./logging/sinks.md). JSON lines on stdout, the default, fit GKE, Cloud Run and hosts with the Cloud Logging agent.
