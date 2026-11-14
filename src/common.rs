//! Helpers shared by the exporters and the logging layer.

use crate::errors::{GcloudTraceError, GcloudTraceSystemError, TraceExportResult};
#[cfg(feature = "trace-api")]
use opentelemetry::{trace::TracerProvider, InstrumentationScope};
#[cfg(feature = "trace-api")]
use opentelemetry_sdk::{
    runtime,
    trace::{
        span_processor_with_async_runtime::BatchSpanProcessor, BatchConfigBuilder,
        SdkTracerProvider, SpanExporter, Tracer,
    },
};
#[cfg(feature = "trace-api")]
use std::time::Duration;

pub(crate) async fn detect_google_project_id() -> TraceExportResult<String> {
    gcloud_sdk::GoogleEnvironment::detect_google_project_id()
        .await
        .ok_or_else(|| {
            GcloudTraceError::SystemError(GcloudTraceSystemError::new(
                "No Google Project ID detected. Please specify it explicitly using env variable: PROJECT_ID or define it as default project for your service accounts".to_string(),
            ))
        })
}

// The async-runtime batch processor rather than the SDK's default thread-based one:
// exporters built on tonic need the Tokio reactor, which the processor's own thread lacks.
#[cfg(feature = "trace-api")]
pub(crate) fn batch_span_processor(
    exporter: impl SpanExporter + 'static,
    max_export_timeout: Option<Duration>,
) -> BatchSpanProcessor<runtime::Tokio> {
    let mut config = BatchConfigBuilder::default();
    if let Some(max_export_timeout) = max_export_timeout {
        config = config.with_max_export_timeout(max_export_timeout);
    }
    BatchSpanProcessor::builder(exporter, runtime::Tokio)
        .with_batch_config(config.build())
        .build()
}

#[cfg(feature = "trace-api")]
pub(crate) fn tracer(provider: &SdkTracerProvider) -> Tracer {
    let scope = InstrumentationScope::builder("opentelemetry-gcloud")
        .with_version(env!("CARGO_PKG_VERSION"))
        .with_schema_url("https://opentelemetry.io/schemas/1.23.0")
        .build();
    provider.tracer_with_scope(scope)
}
