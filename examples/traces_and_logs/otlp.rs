//! Spans and log entries sent to the Google Cloud Telemetry API through OTLP from one builder,
//! with each log entry correlated to the span that emitted it, so that the Google Cloud console
//! shows trace/log correlation end to end: open the trace in Trace Explorer and its "Logs"
//! panel lists each entry under the span that emitted it, or open an entry in Logs Explorer
//! and follow its trace link back to the waterfall. The workload is a multi-level span
//! hierarchy with a log line at every level, the same as
//! `examples/traces_and_logs/cloud_trace_api.rs`.
//!
//! One set of credentials and one endpoint serve both signals. The caller owns the tracer and
//! logger providers and flushes them by shutting them down.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example traces-and-logs-otlp --features logs-otlp
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

    // ANCHOR: setup
    let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone()).with_resource(
        opentelemetry_sdk::Resource::builder()
            .with_service_name("traces-and-logs-otlp-example")
            .build(),
    );
    let tracer_provider = otlp.create_provider().await?;
    let tracer = otlp.install(&tracer_provider);
    let logger_provider = otlp.create_logger_provider().await?;

    let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id.clone())
        .with_otel_logger_provider(
            &logger_provider,
            LogId::new("gcloud-observability-example")?,
        )
        .build()?;

    // The OpenTelemetry layer goes before the logging layer: the logging layer reads the span
    // context the OpenTelemetry layer attaches, and sees only what is registered ahead of it.
    let subscriber = Registry::default()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(tracing_subscriber::fmt::layer())
        .with(log_layer);

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
    logger_provider.shutdown()?;

    Ok(())
}
