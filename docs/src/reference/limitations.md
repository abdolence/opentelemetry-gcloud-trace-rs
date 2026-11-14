# Limitations

- The exporters and the Cloud Logging API sink run on Tokio and need a Tokio runtime when they are created. Other runtimes are not supported.
- Shutting down a tracer provider or an OTLP logger provider blocks the calling thread, and on a current-thread runtime never returns or waits out its timeout. Shut them down on a multi-threaded runtime, or from a thread outside it. `GcpCloudLoggingHandle::shutdown` is async and has no such limit.
- The Cloud Logging API and OTLP sinks queue entries and send them in the background. Entries still queued when the process ends are lost, which includes a panic that unwinds out of `main`, see [Panic hook](../errors/panic-hook.md).
- The Cloud Logging API sink drops entries when its queue is full.
- The Cloud Logging API and OTLP sinks skip events from the transport crates they export through, see [Sinks](../logging/sinks.md).
- The OTLP exporters send only to `https` endpoints.
- The signal-specific `OTEL_EXPORTER_OTLP_TRACES_TIMEOUT` and `OTEL_EXPORTER_OTLP_LOGS_TIMEOUT` are not honoured, see [OTLP exporter](../traces/otlp.md#timeouts).
- A provider builder passed to `create_provider_from_builder` without a resource gets only `gcp.project_id` and the builder's resource.
- There is no metrics API: [metrics](../metrics.md) use `opentelemetry-otlp` with `GcpAuthInterceptor`.
- A `&dyn Error` event field keeps its `source()` chain; `error = %err` and `error = ?err` keep only the text.
- An error event has a backtrace only when `RUST_BACKTRACE` or `RUST_LIB_BACKTRACE` is set.
- The logging layer wrapped in `Option`, `Box<dyn Layer>` or a per-layer filter, under a scoped subscriber, is correlated with the span entered on the thread, not with an event's explicit `parent:`.
