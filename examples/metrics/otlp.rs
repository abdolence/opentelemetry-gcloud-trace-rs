//! Exports metrics to the Google Cloud Telemetry API through OTLP. The crate has no metrics API
//! of its own: an `opentelemetry-otlp` `MetricExporter` is pointed at the Telemetry endpoint and
//! authenticated with [`GcpAuthInterceptor`], which attaches and refreshes the Google
//! credentials of each request.
//!
//! The Telemetry API places each metric by the resource it comes from: `gcp.project_id` names the
//! project, `service.name` and `service.instance.id` the task, and `cloud.region` (or
//! `cloud.availability_zone`) where it ran, which cannot be `global`. A resource missing one is
//! refused with `InvalidArgument`.
//!
//! Run with:
//!
//! ```sh
//! PROJECT_ID=your-project cargo run --example metrics-otlp --features otlp
//! ```

use gcloud_observability::otlp::GcpAuthInterceptor;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::tonic_types::transport::ClientTlsConfig;
use opentelemetry_otlp::{MetricExporter, WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::metrics::SdkMeterProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let google_project_id = std::env::var("PROJECT_ID")?;

    // ANCHOR: setup
    let exporter = MetricExporter::builder()
        .with_tonic()
        .with_endpoint("https://telemetry.googleapis.com")
        .with_tls_config(ClientTlsConfig::new().with_enabled_roots())
        .with_interceptor(GcpAuthInterceptor::from_adc().await?)
        .build()?;

    let meter_provider = SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("metrics-otlp-example")
                .with_attribute(KeyValue::new(
                    "service.instance.id",
                    "metrics-otlp-example-1",
                ))
                .with_attribute(KeyValue::new("cloud.region", "europe-west1"))
                .with_attribute(KeyValue::new("gcp.project_id", google_project_id))
                .build(),
        )
        .build();
    // ANCHOR_END: setup

    let orders = meter_provider
        .meter("metrics-otlp-example")
        .u64_counter("orders_processed")
        .build();
    orders.add(1, &[KeyValue::new("tenant", "acme")]);

    // Exports the metrics collected so far, then stops the periodic reader.
    meter_provider.shutdown()?;

    Ok(())
}
