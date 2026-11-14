# Quick start

Cargo.toml:

```toml
[dependencies]
gcloud-observability = { version = "0.31", features = ["logs-otlp"] }
opentelemetry_sdk = "0.33"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-opentelemetry = "0.34"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Minimum supported Rust version is 1.91.

The `logs` feature, which provides the logging layer, is on by default.
`logs-otlp` adds the span exporter for the Telemetry API and the OTLP sink of the logging layer; it includes `logs` and `otlp`.

Spans go to the Telemetry API through OTLP in both modes.
`APP_MODE` (default `development`) picks the log sink:

- `development`: console lines, and the same entries over OTLP from the same builder, so that a run from a laptop shows the correlated entries in the console;
- `production`: JSON lines on stdout, which a logging agent on GKE, Cloud Run or GCE collects.

```rust,ignore
{{#include ../../examples/quickstart.rs:app-mode}}

{{#include ../../examples/quickstart.rs:setup}}
```

The setup needs these imports:

```rust,ignore
use gcloud_observability::ids::LogId;
use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};
```

Call `tracer_provider.shutdown()` and, in development mode, `logger_provider.shutdown()` before the process ends. They export what is still queued, and block the calling thread, so they need the multi-threaded Tokio runtime that `#[tokio::main]` starts.

The development mode writes every event to the terminal and over OTLP. A service picks one sink.

The credentials are found by ADC and need the role described in [Authentication](./authentication.md).

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/quickstart.rs).

## Running the examples

All examples are in the [examples](https://github.com/abdolence/gcloud-observability-rs/tree/master/examples) directory, named after their goal.
Each example names its features in its header comment and in `required-features` in `Cargo.toml`:

```bash
PROJECT_ID=<your-google-project-id> cargo run --example quickstart --features logs-otlp
PROJECT_ID=<your-google-project-id> APP_MODE=production cargo run --example quickstart --features logs-otlp
```

| Goal                        | Examples                                                            |
|-----------------------------|---------------------------------------------------------------------|
| Logging                     | `logging-dev-and-prod`, `logging-cloud-logging-api`, `logging-otlp` |
| Errors and panics           | `errors-error-reporting`, `errors-panic-hook`                       |
| Traces                      | `traces-otlp`, `traces-cloud-trace-api`                             |
| Traces with correlated logs | `traces-and-logs-otlp`, `traces-and-logs-cloud-trace-api`           |
| Metrics                     | `metrics-otlp`                                                      |
