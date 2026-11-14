# Migration guide

## 0.31

The crate is renamed from `opentelemetry-gcloud-trace` to `gcloud-observability`, and its Cloud Trace API exporter moves behind a feature.

The crate is no longer trace-only.
With Google's Telemetry API, which takes OTLP, a trace exporter alone adds little over `opentelemetry-otlp`.
The value of the library is the Cloud Logging layer, Error Reporting and the authentication of OTLP exporters, so the name says what the crate is for.

- Version numbers continue from 0.30: the first release under the new name is 0.31.0.
- `opentelemetry-gcloud-trace` 0.31.0 is a forwarding crate. It re-exports `gcloud-observability`, with the same feature names, so a dependency on the old name keeps building. The breaking changes below apply to it as well.
- The GitHub repository is `abdolence/gcloud-observability-rs`. The old URL redirects.

Changes to make:

- the dependency is renamed, and `trace-api` or `otlp` is added to its features;
- the exporter types move to modules, and the error types are renamed;
- `install` is synchronous, and `GcpCloudLoggingLayerBuilder::build()` returns a `Result`;
- the log ID is a `LogId`;
- the example names changed;
- the instrumentation scope name changed.

The project ID is checked, and errors report their source differently, without a build failing.

### Dependency and features

Rename the dependency and its `use` paths:

```diff
-opentelemetry-gcloud-trace = "0.30"
+gcloud-observability = { version = "0.31", features = ["trace-api"] }
```

```diff
-use opentelemetry_gcloud_trace::...;
+use gcloud_observability::...;
```

Without the rename, `opentelemetry-gcloud-trace = { version = "0.31", features = ["trace-api"] }` works, and the paths keep the old crate name: `opentelemetry_gcloud_trace::trace_api::...`.

No exporter is enabled by default in 0.31.
The exporter of 0.30 is the Cloud Trace API exporter, so `features = ["trace-api"]` keeps the behaviour.
To export through the Telemetry API instead, use `features = ["otlp"]` and `GcpOtlpExporterBuilder`, see [Choosing an exporter](https://abdolence.github.io/gcloud-observability-rs/traces/choosing.html).

`logs`, `logs-api`, `tls-roots`, `tls-webpki-roots` and `auth-default-crypto` keep their names.
`logs-otlp` and `otlp` are new.

### Module paths

Nothing is re-exported at the crate root:

| 0.30                                                      | 0.31                                                       |
|-----------------------------------------------------------|------------------------------------------------------------|
| `opentelemetry_gcloud_trace::GcpCloudTraceExporterBuilder` | `gcloud_observability::trace_api::GcpCloudTraceExporterBuilder` |
| `opentelemetry_gcloud_trace::GcpCloudTraceExporter`        | `gcloud_observability::trace_api::GcpCloudTraceExporter`   |
| `opentelemetry_gcloud_trace::SdkTracer`                    | removed; use `opentelemetry_sdk::trace::Tracer`            |
| `opentelemetry_gcloud_trace::TraceExportResult`            | `gcloud_observability::errors::GcloudObservabilityResult`  |
| `opentelemetry_gcloud_trace::logs::*`                      | `gcloud_observability::logs::*`                            |
| `opentelemetry_gcloud_trace::errors::*`                    | `gcloud_observability::errors::*`                          |

### Error types

| 0.30                    | 0.31                              |
|-------------------------|-----------------------------------|
| `GcloudTraceError`        | `GcloudObservabilityError`        |
| `GcloudTraceSystemError`  | `GcloudObservabilitySystemError`  |
| `GcloudTraceNetworkError` | `GcloudObservabilityNetworkError` |

`GcloudObservabilityError` has a new variant, `InvalidParametersError`, so a `match` that lists every variant needs an arm for it.

### Error display and source

The format of `Display` is unchanged.
`source()` of `GcloudObservabilityError` returns the root cause of the inner error, or none, where 0.30 returned the inner error that `Display` had already printed.
A program that prints the chain, such as `anyhow` or a `tracing` event with a `&dyn Error` field, shows each error once.

An error from `gcloud-sdk` has the message `Google SDK error` and the `gcloud-sdk` error as its source, where 0.30 put the source's text in the message.
Code that matches on the message text needs to read the source instead.

### Install is synchronous

`install` takes `&self`, returns the tracer directly and has no `await`:

```diff
-let tracer = gcp_trace_exporter.install(&tracer_provider).await?;
+let tracer = gcp_trace_exporter.install(&tracer_provider);
```

### Building the logging layer

`GcpCloudLoggingLayerBuilder::build()` returns a `Result`: it refuses an invalid project ID.

```diff
-let log_layer = GcpCloudLoggingLayerBuilder::new(project_id).build();
+let log_layer = GcpCloudLoggingLayerBuilder::new(project_id).build()?;
```

### Log ID

`GcpCloudLoggingApiConfig::new` takes a `LogId` in place of a string.
`LogId` is 1 to 511 letters, digits and `/ _ - .`, and is checked when it is created:

```diff
-GcpCloudLoggingApiConfig::new("my-service")
+GcpCloudLoggingApiConfig::new(LogId::new("my-service")?)
```

`LogId::from_static("my-service")` checks a constant at compile time.
`LogId` is in `gcloud_observability::ids`.
A `/` in the log ID is URL-encoded as `%2F` in the `logName`, as Cloud Logging stores it.

### Project ID

Building an exporter or a logging layer refuses a project ID that is empty or contains `/` or a control character, with `InvalidParametersError` for the field `google_project_id`.

### Example names

The examples are named after their goal, and each has `required-features` in `Cargo.toml`:

| 0.30                         | 0.31                                                                |
|------------------------------|---------------------------------------------------------------------|
| `enable-exporter`            | `traces-cloud-trace-api`, `traces-otlp`                             |
| `tracing-exporter`           | `traces-cloud-trace-api`, `traces-otlp`                             |
| `traces-and-logs`            | `traces-and-logs-cloud-trace-api`, `traces-and-logs-otlp`           |
| `logging`                    | `logging-json`                                                      |
| `logging-only-api`           | `logging-cloud-logging-api`                                         |

`quickstart`, `logging-otlp`, `errors-error-reporting`, `errors-panic-hook` and `metrics-otlp` are new.

### Instrumentation scope name

The tracer returned by `install` has the instrumentation scope name `gcloud-observability`, where 0.30 used `opentelemetry-gcloud`.
Update a filter or a query that selects spans by the scope name.

### Events written by the logging layer

An event field recorded as a `&dyn Error` keeps its whole `source()` chain, one `Caused by:` line per source.
`error = %err` and `error = ?err` record the text only, as before.

The Cloud Logging API sink and the OTLP sink skip events from the crates they export through (`h2`, `hyper`, `tonic`, `tower`, `rustls`, `gcloud_sdk`, `opentelemetry*`), because at DEBUG every export would make events that are exported in turn.
The JSON lines sink writes them.

### New in 0.31

- The [OTLP exporter](https://abdolence.github.io/gcloud-observability-rs/traces/otlp.html) for the Telemetry API, and the OTLP log sink. The OTLP log processor exports on a thread of its own, and reports a failed export as an `ERROR` `tracing` event with the `opentelemetry_sdk` target.
- [Error Reporting](https://abdolence.github.io/gcloud-observability-rs/errors/error-reporting.html), with backtraces and a panic hook.
- `GcpAuthInterceptor`, to authenticate any OTLP exporter, see [Metrics over OTLP](https://abdolence.github.io/gcloud-observability-rs/metrics.html).
