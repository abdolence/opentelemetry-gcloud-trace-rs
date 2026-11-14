//! Spans and log entries correlated to the span that emitted them, so that the Google Cloud
//! console shows trace/log correlation end to end: open the trace in Trace Explorer and its
//! "Logs" panel lists each entry under the span that emitted it, or open an entry in Logs
//! Explorer and follow its trace link back to the waterfall. The workload is a multi-level span
//! hierarchy with a log line at every level, the same as
//! `examples/traces_and_logs/cloud_trace_api.rs`.
//!
//! Spans go to the Google Cloud Telemetry API through OTLP in both modes. `APP_MODE` (default
//! `development`) picks the log sink. `development` prints human-readable lines to the console
//! and also sends the entries through OTLP from the same builder, so a run from a laptop shows
//! the trace/log correlation in the console immediately, with no logging agent required.
//! `production` writes JSON lines to stdout only, which is what a GKE, Cloud Run or GCE logging
//! agent collects, and it is only once the agent has ingested those lines that the correlation
//! appears in the console.
//!
//! The caller owns the tracer and logger providers and flushes them by shutting them down.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example traces-and-logs-otlp --features logs-otlp
//! ```
//!
//! or, for the JSON-only variant:
//!
//! ```sh
//! PROJECT_ID=your-project APP_MODE=production cargo run --example traces-and-logs-otlp --features logs-otlp
//! ```

use gcloud_observability::ids::LogId;
use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
use gcloud_observability::otlp::GcpOtlpExporterBuilder;
use opentelemetry::trace::TraceContextExt;
use std::time::Duration;
use tracing::*;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

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

#[instrument(skip_all, fields(user_id = %user_id))]
async fn authenticate(user_id: &str) {
    tokio::time::sleep(Duration::from_millis(20)).await;
    info!(user_id, "user authenticated");
    // Dropped by the default `info` filter; run with `RUST_LOG=debug` to see it.
    debug!(user_id, "token scopes checked");
}

#[instrument(skip_all, fields(order_id = %order_id))]
async fn fetch_from_db(order_id: &str) -> u64 {
    tokio::time::sleep(Duration::from_millis(15)).await;
    let rows = 3;
    info!(order_id, rows, "loaded order rows");
    rows
}

#[instrument]
async fn payment_attempt(attempt: u32) -> Result<(), String> {
    tokio::time::sleep(Duration::from_millis(30)).await;
    if attempt < 3 {
        Err(format!("provider timeout on attempt {attempt}"))
    } else {
        Ok(())
    }
}

/// Runs on its own spawned task so the example also demonstrates carrying a
/// span across a `tokio::spawn` boundary with `Instrument`, which only works
/// with a globally installed subscriber - a thread-local one set through
/// `tracing::subscriber::with_default` would not be visible on the spawned
/// task's thread.
async fn call_payment_provider() -> Result<(), String> {
    for attempt in 1..=3u32 {
        let result = payment_attempt(attempt).await;
        match result {
            Ok(()) => {
                info!(attempt, "payment captured");
                return Ok(());
            }
            Err(ref err) if attempt < 3 => {
                warn!(attempt, error = %err, "payment attempt failed, retrying");
            }
            Err(err) => return Err(err),
        }
    }
    unreachable!("loop always returns by the third attempt")
}

#[instrument(skip_all, fields(order_id = %order_id))]
async fn load_order(order_id: &str) -> Result<u64, String> {
    let rows = fetch_from_db(order_id).await;
    let span = info_span!("call_payment_provider");
    tokio::spawn(call_payment_provider().instrument(span))
        .await
        .map_err(|join_err| join_err.to_string())??;
    Ok(rows)
}

#[instrument(skip_all, fields(order_id = %order_id))]
async fn send_notification(order_id: &str) -> Result<(), String> {
    tokio::time::sleep(Duration::from_millis(10)).await;
    let err = format!("no notification channel configured for order {order_id}");
    // Logged here rather than by the caller so the ERROR entry sits under
    // the `send_notification` span in the waterfall, not under the request.
    error!(error = %err, "failed to notify customer of checkout");
    Err(err)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;
    let mode = AppMode::from_env();

    // ANCHOR: setup
    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_service_name("traces-and-logs-otlp-example")
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);

    // `Option<Layer>` implements `Layer`, so every sink is built from one `match` and composed
    // unconditionally below: an inactive sink is a `None` that costs nothing. Development mode
    // writes every event twice, to the terminal and over OTLP, purely so that a run from a
    // laptop shows the correlated entries in the console with nothing else to set up. A real
    // service picks one sink: the JSON layer where an agent collects stdout, or the OTLP layer
    // where none does, never both.
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
            Some(GcpCloudLoggingLayerBuilder::new(google_project_id.clone()).build()?),
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

    // A global subscriber, not a scoped `with_default`, because the workload below carries a
    // span across a `tokio::spawn` boundary onto a different thread.
    tracing::subscriber::set_global_default(subscriber)?;
    // ANCHOR_END: setup

    let root = info_span!("handle_request", order_id = "order-42", tenant = "acme");
    let trace_id = {
        let _enter = root.enter();

        info!(
            "labels.tenant" = "acme",
            "http_request.request_method" = "POST",
            "http_request.request_url" = "https://example.test/orders/order-42/checkout",
            "http_request.status" = 200,
            "http_request.latency" = "125ms",
            "checkout request received"
        );

        authenticate("user-7").await;

        match load_order("order-42").await {
            Ok(rows) => info!(rows, "order ready for fulfilment"),
            Err(ref err) => error!(error = %err, "order could not be loaded"),
        }

        if send_notification("order-42").await.is_err() {
            warn!("checkout completed without customer notification");
        }

        root.context().span().span_context().trace_id()
    };
    drop(root);

    info!(
        trace_id = %trace_id,
        trace_explorer = %format!(
            "https://console.cloud.google.com/traces/list?project={google_project_id}&tid={trace_id}"
        ),
        logs_explorer = %format!(
            "https://console.cloud.google.com/logs/query;query=trace%3D%22projects%2F{google_project_id}%2Ftraces%2F{trace_id}%22?project={google_project_id}"
        ),
        "request handled; open the trace in the console"
    );

    // Both block until the batches are exported, so they need the multi-threaded runtime
    // `#[tokio::main]` starts.
    tracer_provider.shutdown()?;
    if let Some(logger_provider) = logger_provider {
        logger_provider.shutdown()?;
    }

    Ok(())
}
