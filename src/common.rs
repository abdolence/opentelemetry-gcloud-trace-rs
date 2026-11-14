//! Helpers shared by the exporters and the logging layer.

use crate::errors::{
    GcloudObservabilityError, GcloudObservabilityResult, GcloudObservabilitySystemError,
};
#[cfg(any(feature = "trace-api", feature = "otlp"))]
use opentelemetry::{trace::TracerProvider, InstrumentationScope};
#[cfg(any(feature = "trace-api", feature = "otlp"))]
use opentelemetry_sdk::{
    runtime,
    trace::{
        span_processor_with_async_runtime::BatchSpanProcessor, BatchConfigBuilder,
        SdkTracerProvider, SpanExporter, Tracer,
    },
};
#[cfg(any(feature = "trace-api", feature = "otlp"))]
use std::time::Duration;

pub(crate) async fn detect_google_project_id() -> GcloudObservabilityResult<String> {
    gcloud_sdk::GoogleEnvironment::detect_google_project_id()
        .await
        .ok_or_else(|| {
            GcloudObservabilityError::SystemError(GcloudObservabilitySystemError::new(
                "No Google Project ID detected. Please specify it explicitly using env variable: PROJECT_ID or define it as default project for your service accounts".to_string(),
            ))
        })
}

// The async-runtime batch processor rather than the SDK's default thread-based one:
// exporters built on tonic need the Tokio reactor, which the processor's own thread lacks.
#[cfg(any(feature = "trace-api", feature = "otlp"))]
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

#[cfg(any(feature = "trace-api", feature = "otlp"))]
pub(crate) fn tracer(provider: &SdkTracerProvider) -> Tracer {
    provider.tracer_with_scope(instrumentation_scope())
}

#[cfg(any(feature = "trace-api", feature = "otlp"))]
pub(crate) fn instrumentation_scope() -> InstrumentationScope {
    InstrumentationScope::builder("gcloud-observability")
        .with_version(env!("CARGO_PKG_VERSION"))
        .with_schema_url("https://opentelemetry.io/schemas/1.23.0")
        .build()
}
