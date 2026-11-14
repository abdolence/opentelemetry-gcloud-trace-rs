//! Exports spans to Google Cloud Trace through the Cloud Trace API v2
//! (`cloudtrace.googleapis.com`).

mod client;
mod exporter;

pub use exporter::GcpCloudTraceExporter;

use crate::common;
use crate::errors::{GcloudObservabilityError, GcloudObservabilityResult};
use opentelemetry_sdk::error::OTelSdkError;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanExporter, TracerProviderBuilder};
use opentelemetry_sdk::Resource;
use rsb_derive::*;
use std::time::Duration;

#[derive(Debug, Builder)]
#[non_exhaustive]
pub struct GcpCloudTraceExporterBuilder {
    pub google_project_id: String,
    pub resource: Option<Resource>,
    /// Exports span events to Cloud Trace as `time_events` annotations. Defaults to `false`
    /// since span events are commonly captured by a logging layer as well.
    pub span_events: Option<bool>,
    /// The longest a batch export may take before the batch is dropped. Shutdown exports every
    /// batch queued ahead of it, one after the other, so it can take this long once per 512
    /// queued spans, rounded up. Defaults to the OpenTelemetry SDK default of 30 seconds, or
    /// `OTEL_BSP_EXPORT_TIMEOUT` when set.
    pub max_export_timeout: Option<Duration>,
}

impl GcpCloudTraceExporterBuilder {
    pub async fn for_default_project_id() -> GcloudObservabilityResult<Self> {
        Ok(Self::new(common::detect_google_project_id().await?))
    }

    pub async fn create_provider(&self) -> Result<SdkTracerProvider, GcloudObservabilityError> {
        self.create_provider_from_builder(SdkTracerProvider::builder())
            .await
    }

    pub async fn create_provider_from_builder(
        &self,
        builder: TracerProviderBuilder,
    ) -> Result<SdkTracerProvider, GcloudObservabilityError> {
        let exporter = GcpCloudTraceExporter::new(
            &self.google_project_id,
            self.resource
                .clone()
                .unwrap_or_else(|| Resource::builder_empty().build()),
            self.span_events.unwrap_or(false),
        )
        .await?;

        let tracer_provider = builder
            .with_span_processor(self.batch_span_processor(exporter))
            .build();

        Ok(tracer_provider)
    }

    fn batch_span_processor(
        &self,
        exporter: impl SpanExporter + 'static,
    ) -> BatchSpanProcessor<runtime::Tokio> {
        common::batch_span_processor(exporter, self.max_export_timeout)
    }

    pub async fn install(
        self,
        provider: &SdkTracerProvider,
    ) -> Result<opentelemetry_sdk::trace::Tracer, OTelSdkError> {
        Ok(common::tracer(provider))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{Tracer, TracerProvider};
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::SpanData;
    use std::time::Instant;

    #[derive(Debug)]
    struct HungExporter;

    impl SpanExporter for HungExporter {
        fn export(
            &self,
            _batch: Vec<SpanData>,
        ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
            std::future::pending()
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn configured_export_timeout_bounds_shutdown() {
        let builder = GcpCloudTraceExporterBuilder::new("test-project".to_string())
            .with_max_export_timeout(Duration::from_millis(200));
        let provider = SdkTracerProvider::builder()
            .with_span_processor(builder.batch_span_processor(HungExporter))
            .build();
        provider.tracer("test").start("hung");

        let started = Instant::now();
        provider
            .shutdown()
            .expect_err("an abandoned export is reported");

        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
