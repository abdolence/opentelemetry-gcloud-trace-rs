# Choosing an exporter

Two exporters send spans to Google Cloud Trace. Neither is enabled by default.

| Exporter                              | Feature     | Sends to                                                  |
|---------------------------------------|-------------|-----------------------------------------------------------|
| [OTLP exporter](./otlp.md)            | `otlp`      | the Telemetry API, `telemetry.googleapis.com`, over OTLP  |
| [Cloud Trace API exporter](./cloud-trace-api.md) | `trace-api` | the Cloud Trace API v2, `cloudtrace.googleapis.com`       |

The Telemetry API takes traces, metrics and logs over OTLP.
Choose `otlp` unless the case below applies. It shares credentials and an endpoint with the [OTLP log sink](../logging/otlp.md) and with [metrics](../metrics.md), and supports regional endpoints.

Choose `trace-api` when the project is in an Assured Workloads environment with data residency or Impact Level 4 (IL4, a US Department of Defense cloud security level) requirements: do not send spans to the Telemetry API there. See the section "Cloud Trace and data residency" of the [Telemetry API traces reference](https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/v1.traces).

## Things to know before moving to OTLP

- Keep the Cloud Trace API enabled on the project, beside the Telemetry API: with it disabled, Google Cloud Observability discards the trace data sent to the Telemetry API. See the [Telemetry API traces reference](https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/v1.traces) and the [Cloud Trace release notes](https://docs.cloud.google.com/trace/docs/release-notes).
- The role to grant is `roles/telemetry.writer`, see [Authentication](../authentication.md).
- A VPC Service Controls restriction on the Telemetry API applies only to that service. The Cloud Trace API is not covered by it, so check the perimeter for the API the exporter sends to.
- Google notes that the Cloud Trace format can have lower limits for certain fields than OTLP, so some OTLP data may be lost or not map to Cloud Trace fields. See [Migrate from the Cloud Trace API to OTLP endpoints](https://docs.cloud.google.com/trace/docs/migrate-to-otlp-endpoints).
- A regional endpoint, `telemetry.REGION.rep.googleapis.com`, is chosen with the exporter's `endpoint`, see [OTLP exporter](./otlp.md#regional-endpoints).

Logging does not depend on the choice: the [logging layer](../logging/layer.md) reads the span context from the `tracing-opentelemetry` layer, whichever exporter sits behind it.
