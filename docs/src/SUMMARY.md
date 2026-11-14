# Table of contents

[Google Cloud observability for Rust](./intro.md)

# Getting started

- [Quick start](./quick-start.md)
- [Feature flags](./features.md)
- [Authentication](./authentication.md)

# Logging

- [The logging layer](./logging/layer.md)
- [Sinks](./logging/sinks.md)
    - [JSON lines](./logging/json-lines.md)
    - [Cloud Logging API](./logging/cloud-logging-api.md)
    - [OTLP](./logging/otlp.md)
- [Field conventions](./logging/fields.md)
- [One application, two modes](./logging/two-modes.md)
- [Comparison with the OpenTelemetry tracing bridge](./logging/otlp-bridge.md)

# Errors and stack traces

- [Error Reporting](./errors/error-reporting.md)
- [Backtraces](./errors/backtraces.md)
- [Panic hook](./errors/panic-hook.md)

# Traces

- [Choosing an exporter](./traces/choosing.md)
- [OTLP exporter](./traces/otlp.md)
- [Cloud Trace API exporter](./traces/cloud-trace-api.md)
- [Span events and annotations](./traces/span-events.md)
- [Traces and logs together](./traces/traces-and-logs.md)

# Metrics

- [Metrics over OTLP](./metrics.md)

# Reference

- [TLS and crypto features](./reference/tls.md)
- [Compatibility matrix](./reference/compatibility.md)
- [Limitations](./reference/limitations.md)
- [Troubleshooting](./reference/troubleshooting.md)

# Migration

- [Migrating from opentelemetry-gcloud-trace 0.30](./migrating-from-0.30.md)
- [Migrating from tracing-stackdriver](./migrating-from-tracing-stackdriver.md)
