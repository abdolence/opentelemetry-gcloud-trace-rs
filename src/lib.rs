//! # OpenTelemetry Google Cloud Trace Exporter
//!
//! OpenTelemetry exporter implementation for Google Cloud Trace
//!
//! ## Performance
//!
//! For optimal performance, a batch exporter is recommended as the simple exporter will export
//! each span synchronously on drop. You can enable the [`rt-tokio`], [`rt-tokio-current-thread`]
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
//! let google_project_id = config_env_var("PROJECT_ID")?;
//! let tracer: opentelemetry::sdk::trace::Tracer = GcpCloudTraceExporterBuilder::new(google_project_id)
//!   .install_batch(opentelemetry::runtime::Tokio)
//!   .await?;
//! ```
//! ## Configuration
//!
//! You can specify trace configuration using `with_trace_config`:
//!
//! ```ignore
//!    GcpCloudTraceExporterBuilder::new(google_project_id).with_trace_config(
//!       trace::config()
//!          .with_sampler(Sampler::AlwaysOn)
//!          .with_id_generator(RandomIdGenerator::default())
//!    )
//! ```

#![allow(unused_parens, clippy::new_without_default, clippy::needless_update)]

pub mod errors;
pub type TraceExportResult<E> = Result<E, crate::errors::GcloudTraceError>;

mod google_trace_exporter_client;
mod span_exporter;

use opentelemetry::trace::TracerProvider;
pub use span_exporter::GcpCloudTraceExporter;

use rsb_derive::*;

#[derive(Debug, Builder)]
pub struct GcpCloudTraceExporterBuilder {
    pub google_project_id: String,
    pub trace_config: Option<opentelemetry::sdk::trace::Config>,
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

    pub async fn install_simple(
        self,
    ) -> Result<opentelemetry::sdk::trace::Tracer, opentelemetry::trace::TraceError> {
        let exporter = GcpCloudTraceExporter::new(&self.google_project_id).await?;

        let mut provider_builder =
            opentelemetry::sdk::trace::TracerProvider::builder().with_simple_exporter(exporter);
        provider_builder = if let Some(config) = self.trace_config {
            provider_builder.with_config(config)
        } else {
            provider_builder
        };
        let provider = provider_builder.build();
        let tracer = provider.versioned_tracer(
            "opentelemetry-gcloud",
            Some(env!("CARGO_PKG_VERSION")),
            Some("https://opentelemetry.io/schemas/1.23.0"),
            None,
        );
        let _ = opentelemetry::global::set_tracer_provider(provider);
        Ok(tracer)
    }

    pub async fn install_batch<
        R: opentelemetry::sdk::runtime::Runtime
            + opentelemetry::runtime::RuntimeChannel<opentelemetry::sdk::trace::BatchMessage>,
    >(
        self,
        runtime: R,
    ) -> Result<opentelemetry::sdk::trace::Tracer, opentelemetry::trace::TraceError> {
        let exporter = GcpCloudTraceExporter::new(&self.google_project_id).await?;

        let mut provider_builder = opentelemetry::sdk::trace::TracerProvider::builder()
            .with_batch_exporter(exporter, runtime);
        provider_builder = if let Some(config) = self.trace_config {
            provider_builder.with_config(config)
        } else {
            provider_builder
        };
        let provider = provider_builder.build();
        let tracer = provider.versioned_tracer(
            "opentelemetry-gcloud",
            Some(env!("CARGO_PKG_VERSION")),
            Some("https://opentelemetry.io/schemas/1.23.0"),
            None,
        );
        let _ = opentelemetry::global::set_tracer_provider(provider);
        Ok(tracer)
    }
}
