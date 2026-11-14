# Google Cloud observability for Rust

Formerly `opentelemetry-gcloud-trace`, see [Migrating from 0.30](./migrating-from-0.30.md).

The library sends what a Rust service emits through `tracing` and OpenTelemetry to Google Cloud:

- [Cloud Logging](./logging/layer.md): a `tracing-subscriber` layer that turns every event into a structured log entry, correlated with the trace and span that emitted it, so the Google Cloud console shows each log line under its trace;
- three [log sinks](./logging/sinks.md): JSON lines on stdout for GKE, Cloud Run and hosts with the Cloud Logging agent, the Cloud Logging API, and OTLP;
- [Error Reporting](./errors/error-reporting.md): errors logged with the code location, the `source()` chain and a backtrace, and [panics](./errors/panic-hook.md), in the format Error Reporting groups;
- [Cloud Trace](./traces/choosing.md): OpenTelemetry span exporters for the Telemetry API and for the Cloud Trace API v2;
- [Authentication](./authentication.md) for any OTLP exporter, [metrics](./metrics.md) included, with Application Default Credentials refreshed in the background.

## Terms

OTLP is the OpenTelemetry protocol, the wire format OpenTelemetry SDKs send traces, metrics and logs in.
The Telemetry API (`telemetry.googleapis.com`) is the Google Cloud endpoint that takes OTLP for all three signals; see [Telemetry API overview](https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/overview).
Application Default Credentials (ADC) is the way Google client libraries find credentials: an environment variable, a local file written by `gcloud`, or the metadata server of the host.

## How the pieces fit

- A span is created by `tracing` macros or by the OpenTelemetry API. The `tracing-opentelemetry` layer turns `tracing` spans into OpenTelemetry spans, and a tracer provider hands finished spans to an exporter: `otlp` for the Telemetry API, or `trace-api` for the Cloud Trace API.
- A `tracing` event passes through the logging layer, which builds one log entry and gives it to the sink chosen when the layer was built.
- The logging layer reads the trace and span ids that the `tracing-opentelemetry` layer attached, so the entry and the span are linked in the console. The `tracing-opentelemetry` layer is therefore registered first, see [The logging layer](./logging/layer.md).
- The exporters, the Cloud Logging API sink and any OTLP exporter built elsewhere authenticate through the same ADC, see [Authentication](./authentication.md).

Neither exporter is enabled by default, see [Feature flags](./features.md).
To start using the library, see [Quick start](./quick-start.md).
