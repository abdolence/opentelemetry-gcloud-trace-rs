[![Cargo](https://img.shields.io/crates/v/gcloud-observability.svg)](https://crates.io/crates/gcloud-observability)
![tests and formatting](https://github.com/abdolence/gcloud-observability-rs/workflows/tests%20&amp;%20formatting/badge.svg)
![security audit](https://github.com/abdolence/gcloud-observability-rs/workflows/security%20audit/badge.svg)

# Google Cloud observability for Rust

Formerly `opentelemetry-gcloud-trace`, see the [migration guide](MIGRATION.md).

The library sends what a Rust service emits through `tracing` and OpenTelemetry to Google Cloud:

- Cloud Logging: a `tracing-subscriber` layer that writes structured log entries, each correlated with the trace and span that emitted it, so the console shows every log line under its trace;
- Cloud Trace: OpenTelemetry span exporters for the Telemetry API, which takes OTLP (the OpenTelemetry wire protocol), and for the Cloud Trace API v2;
- Error Reporting: errors logged with the code location, the `source()` chain and a backtrace, and panics, in the format Error Reporting groups;
- Authentication of any OTLP exporter, metrics included, with Google Application Default Credentials (ADC), refreshed in the background;
- Three log sinks: JSON lines on stdout for GKE, Cloud Run and hosts with the logging agent, the Cloud Logging API, and OTLP.

## Documentation

Please follow to the official website: <https://abdolence.github.io/gcloud-observability-rs/>.

## Features

| Feature               | Goal                                                                                       |
|-----------------------|--------------------------------------------------------------------------------------------|
| `logs` (default)      | The logging layer with the JSON lines sink, and Error Reporting for every sink             |
| `logs-api`            | The logging layer writes through the Cloud Logging API, for hosts with no logging agent    |
| `logs-otlp`           | The logging layer sends entries over OTLP to the Telemetry API; includes `logs` and `otlp` |
| `otlp`                | Span exporter and authentication for the Telemetry API (`telemetry.googleapis.com`)        |
| `trace-api`           | Span exporter for the Cloud Trace API v2 (`cloudtrace.googleapis.com`)                     |
| `tls-roots` (default) | Native TLS root certificates                                                               |
| `tls-webpki-roots`    | TLS root certificates of the `webpki-roots` crate                                          |
| `auth-default-crypto` | The rustls crypto provider that service account keys are signed with (default)             |

No exporter is enabled by default.

## Quick start

Cargo.toml:

```toml
[dependencies]
gcloud-observability = { version = "0.31", features = ["otlp"] }
opentelemetry_sdk = "0.33"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-opentelemetry = "0.34"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Minimum supported Rust version is 1.91.

```rust,ignore
use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use tracing::{info, info_span};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;

    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_service_name("quickstart-example")
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);

    let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id).build()?;

    // The OpenTelemetry layer goes before the logging layer: the logging layer reads the span
    // context the OpenTelemetry layer attaches, and sees only what is registered ahead of it.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(log_layer);
    tracing::subscriber::set_global_default(subscriber)?;

    info_span!("handle_request", order_id = "order-42").in_scope(|| {
        info!("labels.tenant" = "acme", "Serving a request.");
    });

    // Flushes the spans still queued; blocks, so it needs the multi-threaded runtime.
    tracer_provider.shutdown()?;

    Ok(())
}
```

The spans are exported with the credentials ADC finds, which need the `roles/telemetry.writer` role on the project.
The logging layer writes JSON lines to stdout, which the Cloud Logging agent of GKE, Cloud Run and GCE collects.
See [Authentication](https://abdolence.github.io/gcloud-observability-rs/authentication.html).

Full example available [here](examples/quickstart.rs).

More examples, by goal, are in the [examples](examples) directory:

| Goal                        | Examples                                                    |
|-----------------------------|-------------------------------------------------------------|
| Logging                     | `logging-json`, `logging-cloud-logging-api`, `logging-otlp` |
| Errors and panics           | `errors-error-reporting`, `errors-panic-hook`               |
| Traces                      | `traces-otlp`, `traces-cloud-trace-api`                     |
| Traces with correlated logs | `traces-and-logs-otlp`, `traces-and-logs-cloud-trace-api`   |
| Metrics                     | `metrics-otlp`                                              |

Each runs as `PROJECT_ID=<your-google-project-id> cargo run --example <name> --features <features>`.
The features it needs are in its header and in `required-features` in `Cargo.toml`.

## Compatibility matrix

<!-- ANCHOR: compatibility -->
| gcloud-observability version | opentelemetry version | opentelemetry-otlp | tracing-opentelemetry | gcloud-sdk |
|------------------------------|-----------------------|--------------------|-----------------------|------------|
| 0.31                         | 0.33                  | 0.33               | 0.34                  | 0.33       |
| 0.30                         | 0.33                  | -                  | 0.34                  | 0.33       |
| 0.29                         | 0.33                  | -                  | 0.34                  | 0.33       |
| 0.28                         | 0.33                  | -                  | 0.34                  | 0.32       |
| 0.27                         | 0.32                  | -                  | 0.33                  | 0.32       |
| 0.26                         | 0.32                  | -                  | 0.33                  | 0.32       |
| 0.25                         | 0.32                  | -                  | 0.33                  | 0.31       |
| 0.24                         | 0.32                  | -                  | 0.33                  | 0.30       |
| 0.23                         | 0.31                  | -                  | 0.32                  | 0.29       |

Versions up to 0.30 are published as `opentelemetry-gcloud-trace`.
<!-- ANCHOR_END: compatibility -->

## Migrating

0.31 renames the crate and moves the Cloud Trace API exporter behind the `trace-api` feature.
See the [migration guide](MIGRATION.md) and
[Migrating from 0.30](https://abdolence.github.io/gcloud-observability-rs/migrating-from-0.30.html).

From `tracing-stackdriver`: [Migrating from tracing-stackdriver](https://abdolence.github.io/gcloud-observability-rs/migrating-from-tracing-stackdriver.html).

## Licence

Apache Software License (ASL)

## Author

Abdulla Abdurakhmanov
