//! # Google Cloud Trace and Cloud Logging for OpenTelemetry and tracing
//!
//! The crate covers two uses, separately or together:
//! - an OpenTelemetry span exporter for Google Cloud Trace,
//!   [`GcpCloudTraceExporterBuilder`];
//! - a `tracing-subscriber` layer for the `tracing` crate that writes
//!   structured Google Cloud Logging entries, correlated with the trace and
//!   span that emitted them through the public `tracing-opentelemetry`
//!   context API; see the [`logs`] module.
//!
//! The `logs` feature (default) enables the logging layer with JSON lines
//! written to stdout; `logs-api` adds a sink writing through the Cloud
//! Logging API.
//!
//! ## Performance
//!
//! For optimal performance, a batch exporter is recommended as the simple exporter will export
//! each span synchronously on drop. You can enable the `rt-tokio`, `rt-tokio-current-thread`
//! features and specify a runtime on the pipeline to have a batch exporter
//! configured for you automatically.
//!
//! ```toml
//! [dependencies]
//! opentelemetry = { version = "*", features = ["rt-tokio"] }
//! opentelemetry-gcloud-trace = "*"
//! ```
//!
//! ```ignore
//! let gcp_trace_exporter = GcpCloudTraceExporterBuilder::new(config_env_var("PROJECT_ID")?)
//!
//! let tracer_provider = gcp_trace_exporter.create_provider().await?;
//! let tracer: opentelemetry_sdk::trace::Tracer = gcp_trace_exporter.install(&tracer_provider).await?;
//!
//! opentelemetry::global::set_tracer_provider(tracer_provider.clone());
//!
//! tracer.in_span("doing_work_parent", |cx| {
//!   // ...
//! });
//!
//! tracer_provider.shutdown()?;
//! ```
//! ## Configuration
//!
//! You can specify trace configuration using `create_provider_from_builder`:
//!
//! ```ignore
//!    gcp_trace_exporter.create_provider_from_builder (
//!       TracerProvider::builder()
//!          .with_sampler(Sampler::AlwaysOn)
//!          .with_id_generator(RandomIdGenerator::default())
//!    )
//! ```
//!
//! you can specify resource using `with_resource`:
//! ```ignore
//!    let resources = Resource::new(vec![KeyValue::new("service.name", "my-service")]);
//!    GcpCloudTraceExporterBuilder::new(google_project_id).with_resource(resource).await?;
//! ```
//!
//! and the longest a batch export may take using `with_max_export_timeout`:
//! ```ignore
//!    GcpCloudTraceExporterBuilder::new(google_project_id).with_max_export_timeout(Duration::from_secs(3));
//! ```
//!
//! Have a look at full examples in the `examples` directory.
//!

#![allow(unused_parens, clippy::new_without_default, clippy::needless_update)]

pub mod errors;
pub type TraceExportResult<E> = Result<E, crate::errors::GcloudTraceError>;

#[cfg(feature = "logs")]
pub mod logs;

mod google_trace_exporter_client;
mod span_exporter;

use crate::errors::GcloudTraceError;
use opentelemetry::trace::TracerProvider;
use opentelemetry::InstrumentationScope;
use opentelemetry_sdk::error::OTelSdkError;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor;
use opentelemetry_sdk::trace::{
    BatchConfigBuilder, SdkTracerProvider, SpanExporter, TracerProviderBuilder,
};
use opentelemetry_sdk::Resource;
use rsb_derive::*;
pub use span_exporter::GcpCloudTraceExporter;
use std::time::Duration;

pub type SdkTracer = opentelemetry_sdk::trace::Tracer;

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
    pub async fn for_default_project_id() -> TraceExportResult<Self> {
        let detected_project_id = gcloud_sdk::GoogleEnvironment::detect_google_project_id().await.ok_or_else(||
            crate::errors::GcloudTraceError::SystemError(
                crate::errors::GcloudTraceSystemError::new(
                    "No Google Project ID detected. Please specify it explicitly using env variable: PROJECT_ID or define it as default project for your service accounts".to_string()
                )
            )
        )?;
        Ok(Self::new(detected_project_id))
    }

    pub async fn create_provider(&self) -> Result<SdkTracerProvider, GcloudTraceError> {
        self.create_provider_from_builder(SdkTracerProvider::builder())
            .await
    }

    pub async fn create_provider_from_builder(
        &self,
        builder: TracerProviderBuilder,
    ) -> Result<SdkTracerProvider, GcloudTraceError> {
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
        let mut config = BatchConfigBuilder::default();
        if let Some(max_export_timeout) = self.max_export_timeout {
            config = config.with_max_export_timeout(max_export_timeout);
        }
        BatchSpanProcessor::builder(exporter, runtime::Tokio)
            .with_batch_config(config.build())
            .build()
    }

    pub async fn install(
        self,
        provider: &SdkTracerProvider,
    ) -> Result<opentelemetry_sdk::trace::Tracer, OTelSdkError> {
        let scope = InstrumentationScope::builder("opentelemetry-gcloud")
            .with_version(env!("CARGO_PKG_VERSION"))
            .with_schema_url("https://opentelemetry.io/schemas/1.23.0")
            .build();

        let tracer = provider.tracer_with_scope(scope);
        Ok(tracer)
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
