# Metrics over OTLP

The library has no metrics API of its own.
An `opentelemetry-otlp` `MetricExporter` is pointed at the Telemetry endpoint and authenticated with `GcpAuthInterceptor`, which attaches and refreshes the Google credentials of each request.
The `otlp` feature enables the tonic transport and TLS of `opentelemetry-otlp`; enable its `metrics` feature in your own `Cargo.toml`:

```toml
[dependencies]
opentelemetry-otlp = { version = "0.33", default-features = false, features = ["metrics"] }
```

```rust,ignore
{{#include ../../examples/metrics/otlp.rs:setup}}
```

`GcpAuthInterceptor::from_adc()` uses the Application Default Credentials with the `cloud-platform` scope.
`GcpAuthInterceptor::new` takes other `GoogleAuthHeaders`, which `gcloud_observability::otlp` re-exports.
See [Authentication](./authentication.md) for how the headers are kept current.

## Resource attributes

The Telemetry API places each metric by the resource it comes from, and refuses a resource that misses one of these with `InvalidArgument`:

- `gcp.project_id`: the project;
- `service.name` and `service.instance.id`: the task;
- `cloud.region`, or `cloud.availability_zone`: where it ran. The value cannot be `global`.

Metric data is stored on a regional endpoint only when its location label matches the region or a zone of it, see [OTLP exporter](./traces/otlp.md#regional-endpoints).

Call `meter_provider.shutdown()` before the process ends: it exports the metrics collected so far and stops the periodic reader.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/metrics/otlp.rs).
