//! The shortest useful setup: `tracing` spans exported to Cloud Trace through OTLP, with every
//! event correlated to the span it happened in.
//!
//! `APP_MODE` (default `development`) picks the log sink. `development` prints console lines
//! and also sends the entries over OTLP, so a run from a laptop shows the correlated entries in
//! the Google Cloud console with no logging agent. `production` writes Cloud Logging JSON lines
//! to stdout, which the Cloud Logging agent of GKE, Cloud Run and GCE collects.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example quickstart --features logs-otlp
//! PROJECT_ID=your-project APP_MODE=production cargo run --example quickstart --features logs-otlp
//! ```

use gcloud_observability::ids::LogId;
use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use tracing::{info, info_span};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

// ANCHOR: app-mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Development,
    Production,
}

impl AppMode {
    fn from_env() -> Self {
        match std::env::var("APP_MODE").as_deref() {
            Ok("production") => AppMode::Production,
            _ => AppMode::Development,
        }
    }
}
// ANCHOR_END: app-mode

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;
    let mode = AppMode::from_env();

    // ANCHOR: setup
    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_service_name("quickstart-example")
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);

    // `Option<Layer>` implements `Layer`, so every sink is built from one `match` and composed
    // unconditionally below: an inactive sink is a `None` that costs nothing. Development mode
    // writes every event twice, to the terminal and over OTLP, so that a run from a laptop
    // shows the correlated entries in the console. A real service picks one sink: the JSON
    // layer where an agent collects stdout, or the OTLP layer where none does, never both.
    let (console_layer, otlp_layer, logger_provider, json_layer) = match mode {
        AppMode::Development => {
            let logger_provider = otlp.create_logger_provider().await?;
            let otlp_layer = GcpCloudLoggingLayerBuilder::new(google_project_id.clone())
                .with_otel_logger_provider(
                    &logger_provider,
                    LogId::new("gcloud-observability-example")?,
                )
                .build()?;
            (
                Some(tracing_subscriber::fmt::layer()),
                Some(otlp_layer),
                Some(logger_provider),
                None,
            )
        }
        AppMode::Production => (
            None,
            None,
            None,
            Some(GcpCloudLoggingLayerBuilder::new(google_project_id).build()?),
        ),
    };

    // The OpenTelemetry layer goes before the logging layers: they read the span context it
    // attaches, and see only what is registered ahead of them.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(console_layer)
        .with(otlp_layer)
        .with(json_layer);
    tracing::subscriber::set_global_default(subscriber)?;
    // ANCHOR_END: setup

    info_span!("handle_request", order_id = "order-42").in_scope(|| {
        info!("labels.tenant" = "acme", "Serving a request.");
        info_span!("load_order").in_scope(|| info!(rows = 3, "Loaded the order rows."));
    });

    // Both block until the batches are exported, so they need the multi-threaded runtime
    // `#[tokio::main]` starts.
    tracer_provider.shutdown()?;
    if let Some(logger_provider) = logger_provider {
        logger_provider.shutdown()?;
    }

    Ok(())
}
