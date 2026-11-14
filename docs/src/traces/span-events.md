# Span events and annotations

`tracing-opentelemetry` records every `tracing` event inside a span as an event of that span.
The two exporters treat them differently.

- The [OTLP exporter](./otlp.md) sends span events with the span.
- The [Cloud Trace API exporter](./cloud-trace-api.md) does not export them by default. They would be reported twice, once as a Cloud Trace annotation (`time_events`) and once as a log entry by the [logging layer](../logging/layer.md). `with_span_events(true)` on `GcpCloudTraceExporterBuilder`, before `create_provider()`, exports them as annotations. Span status is exported either way.

Events added through the OpenTelemetry API with `span.add_event(...)` are span events too, and follow the same rule.
Whether to log an event or attach it to a span is a choice of where it is read: a log entry is searched in Logging and linked to its trace, an annotation is shown on the span in Trace Explorer.
