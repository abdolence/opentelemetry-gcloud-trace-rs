# OTLP exporter

The `otlp` feature adds `GcpOtlpExporterBuilder`, which builds OpenTelemetry exporters and providers that send spans, and with `logs-otlp` log records, to the Telemetry API through OTLP.
It sets the endpoint, the TLS configuration, the `gcp.project_id` resource attribute and the authentication, see [Authentication](../authentication.md).

```rust,ignore
{{#include ../../../examples/traces/otlp.rs:setup}}
```

`create_provider` builds a tracer provider with a batch span processor over the exporter, and `install(&provider)` returns a tracer for it.
`install` is synchronous.
Use `create_provider_from_builder` to start from your own `SdkTracerProvider::builder()`, with a sampler or an ID generator, for example.

Spans come from the OpenTelemetry API directly:

```rust,ignore
{{#include ../../../examples/traces/otlp.rs:opentelemetry_api}}
```

or from `tracing`, through the `tracing-opentelemetry` layer, which is how most Rust code creates them:

```rust,ignore
{{#include ../../../examples/traces/otlp.rs:tracing}}
```

## Resource

The provider's resource always has `gcp.project_id` set to the project of the builder.
A resource passed with `with_resource` is merged under it, key by key, so `service.name` is kept.
`create_provider` starts from the OpenTelemetry SDK's default resource.
A builder passed to `create_provider_from_builder` without a resource of its own does not get that default: it ends up with `gcp.project_id` and the builder's resource only, so set `service.name` on one of them.

The project ID is checked when an exporter is built: an empty ID, or one with `/` or a control character, fails with `InvalidParametersError` for the field `google_project_id`.

## Regional endpoints

The default endpoint is `https://telemetry.googleapis.com`.
A regional endpoint, `telemetry.REGION.rep.googleapis.com`, is set with `with_endpoint`, which takes a `TelemetryEndpoint`:

```rust,ignore
let otlp = GcpOtlpExporterBuilder::new(google_project_id)
    .with_endpoint("https://telemetry.europe-west1.rep.googleapis.com".parse()?);
```

`TelemetryEndpoint` accepts only `https` URIs with a host, because every request carries an access token.
TLS is verified against the host of the endpoint.

Regional endpoints store log and trace data in the region. Metric data is stored there only when its location label matches the region or a zone of it.
The endpoint does not decide where the observability bucket is created: that comes from the default settings, organisation policies and supported bucket locations.
See [Telemetry API overview](https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/overview).

## Timeouts

`max_export_timeout` bounds how long a batch export may take before the batch is dropped.
It is the timeout of connecting, the TLS handshake included, and of every export request, and for spans also the batch processor's bound.

While it is unset, connecting, the TLS handshake and every request time out after 10 seconds, or after `OTEL_EXPORTER_OTLP_TIMEOUT` in milliseconds when that is set.
The signal-specific `OTEL_EXPORTER_OTLP_TRACES_TIMEOUT` and `OTEL_EXPORTER_OTLP_LOGS_TIMEOUT` are not honoured.
The span processor gives up after 30 seconds, or `OTEL_BSP_EXPORT_TIMEOUT` when that is set.

## Shutting down

Call `shutdown` on the provider before the process ends. `shutdown`, `force_flush` and dropping the provider block the calling thread until the batch task, which runs on the Tokio runtime the provider was created on, is done, so on a current-thread runtime they never return.
Call them on a multi-threaded runtime, or from a thread outside the runtime.

Shutdown exports every batch queued ahead of it, one after the other, and each may take the full timeout, so it can take the timeout once per 512 queued spans, rounded up.
On a platform that kills the process shortly after SIGTERM, such as Cloud Run, keep that total below the time the platform allows:

```rust,ignore
let otlp = GcpOtlpExporterBuilder::new(google_project_id)
    .with_max_export_timeout(Duration::from_secs(3));
```

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/traces/otlp.rs).
