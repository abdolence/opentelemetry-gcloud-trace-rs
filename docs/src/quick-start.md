# Quick start

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

The `logs` feature, which provides the logging layer, is on by default.
`otlp` adds the span exporter for the Telemetry API.

The setup below exports spans through OTLP and writes every event to stdout as a Cloud Logging JSON line that carries the trace and span it happened in:

```rust,ignore
{{#include ../../examples/quickstart.rs:setup}}
```

The setup needs these imports:

```rust,ignore
use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};
```

The JSON lines are what the Cloud Logging agent of GKE, Cloud Run and GCE collects from stdout.
Run on a laptop they only print; the spans reach the Telemetry API either way.

Call `tracer_provider.shutdown()` before the process ends. It exports the spans still queued, and blocks the calling thread, so it needs the multi-threaded Tokio runtime that `#[tokio::main]` starts.

The credentials are found by ADC and need the role described in [Authentication](./authentication.md).

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/quickstart.rs).

## Running the examples

All examples are in the [examples](https://github.com/abdolence/gcloud-observability-rs/tree/master/examples) directory, named after their goal.
Each example names its features in its header comment and in `required-features` in `Cargo.toml`:

```bash
PROJECT_ID=<your-google-project-id> cargo run --example quickstart --features logs,otlp
```

| Goal                        | Examples                                                            |
|-----------------------------|---------------------------------------------------------------------|
| Logging                     | `logging-dev-and-prod`, `logging-cloud-logging-api`, `logging-otlp` |
| Errors and panics           | `errors-error-reporting`, `errors-panic-hook`                       |
| Traces                      | `traces-otlp`, `traces-cloud-trace-api`                             |
| Traces with correlated logs | `traces-and-logs-otlp`, `traces-and-logs-cloud-trace-api`           |
| Metrics                     | `metrics-otlp`                                                      |
