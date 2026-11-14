# Authentication

Every exporter and sink that talks to Google authenticates with Application Default Credentials (ADC), found in this order:

- a JSON file whose path is in the `GOOGLE_APPLICATION_CREDENTIALS` environment variable;
- a JSON file in the location known to `gcloud`, written by `gcloud auth application-default login`;
- on Google Compute Engine, GKE and Cloud Run, the credentials of the metadata server.

The sinks that write to stdout need no credentials.

## Local development

Don't confuse `gcloud auth login` with `gcloud auth application-default login`.
The first authorises only the `gcloud` tool; the second writes the user credentials that ADC reads.

A user credential sends its requests against a quota project.
Set it with `gcloud auth application-default set-quota-project <project>` when requests are refused for lack of one. The credentials add the `x-goog-user-project` header themselves, and the library does not set it.

## APIs to enable

| Sends to                               | API to enable                                      |
|----------------------------------------|----------------------------------------------------|
| `otlp`, `logs-otlp`, metrics           | Telemetry API (`telemetry.googleapis.com`)         |
| `logs-api`                             | Cloud Logging API (`logging.googleapis.com`)       |
| `trace-api`                            | Cloud Trace API (`cloudtrace.googleapis.com`)      |

Keep the Cloud Trace API enabled when exporting through the Telemetry API as well: with it disabled, Google Cloud Observability discards the trace data sent to the Telemetry API. See [Telemetry API traces reference](https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/v1.traces) and the [Cloud Trace release notes](https://docs.cloud.google.com/trace/docs/release-notes).

## Roles

Grant the roles to the service account the application runs as.

| Sends to                         | Role                                                                          |
|----------------------------------|-------------------------------------------------------------------------------|
| Telemetry API                    | `roles/telemetry.writer` on the project, for traces, metrics and logs         |
| Cloud Logging API                | `roles/logging.logWriter` on the project                                      |
| Cloud Trace API                  | `roles/cloudtrace.agent` on the project                                       |

The Telemetry API also needs `roles/serviceusage.serviceUsageConsumer` on the quota project.

Google's migration guide for the Telemetry API lists narrower roles in place of `roles/telemetry.writer`: `roles/telemetry.tracesWriter`, `roles/logging.logWriter` and `roles/monitoring.metricWriter`.
See [Telemetry API overview](https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/overview) and [Migrate from the Cloud Trace API to OTLP endpoints](https://docs.cloud.google.com/trace/docs/migrate-to-otlp-endpoints).

## Tokens and refresh

A tonic interceptor runs synchronously, while Google tokens are refreshed asynchronously.
`GcpAuthInterceptor` bridges the two: it fetches the first headers when it is created, so bad credentials fail when the exporter is built rather than in a background task, and a background task polls the credentials every 5 seconds and publishes the headers when they change.
A failed refresh keeps the headers published last. A warning is logged when refreshing starts failing and another when it recovers.
The task runs on the Tokio runtime the interceptor was created on, and ends when the last clone is dropped.

All exporters built from one `GcpOtlpExporterBuilder` share one interceptor and one set of credentials.
Pass other credentials with `with_auth_headers`, and build `GcpAuthInterceptor` yourself for an OTLP exporter built elsewhere, see [Metrics over OTLP](./metrics.md).

## Crypto provider

Service account keys sign their tokens with a rustls crypto provider; see [TLS and crypto features](./reference/tls.md).
