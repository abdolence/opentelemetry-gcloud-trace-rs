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
//! let tracer = GcpCloudTraceExporterBuilder::new(google_project_id)
//!   .install()
//!   .await?;
//! ```
//! ## Configuration
//!
//! You can specify trace configuration using `with_tracer_provider_builder`:
//!
//! ```ignore
//!    GcpCloudTraceExporterBuilder::new(google_project_id).with_tracer_provider_builder(
//!       TracerProvider::builder()
//!          .with_sampler(Sampler::AlwaysOn)
//!          .with_id_generator(RandomIdGenerator::default())
//!    )
//! ```
//!
//! you can specify resource using `with_resource`:
//! ```ignore
//!    let resources = Resource::new(vec![KeyValue::new("service.name", "my-service")]);
//!    GcpCloudTraceExporterBuilder::new(google_project_id).with_tracer_provider_builder(
//!       TracerProvider::builder()
//!          .with_sampler(Sampler::AlwaysOn)
//!          .with_id_generator(RandomIdGenerator::default(
//!          .with_resource(resource.clone()),
//!    )
//!    .with_resource(resource)
//!    .await?;

#![allow(unused_parens, clippy::new_without_default, clippy::needless_update)]

pub mod errors;
pub type TraceExportResult<E> = Result<E, crate::errors::GcloudTraceError>;

mod google_trace_exporter_client;
mod span_exporter;

use opentelemetry::trace::TracerProvider;
use opentelemetry::InstrumentationScope;
use opentelemetry_sdk::trace::TracerProviderBuilder;
use opentelemetry_sdk::Resource;
use rsb_derive::*;
pub use span_exporter::GcpCloudTraceExporter;

pub type SdkTracer = opentelemetry_sdk::trace::Tracer;

#[derive(Debug, Builder)]
pub struct GcpCloudTraceExporterBuilder {
    pub google_project_id: String,
    pub tracer_provider_builder: Option<TracerProviderBuilder>,
    pub resource: Option<Resource>,
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

    pub async fn install(
        self,
    ) -> Result<opentelemetry_sdk::trace::Tracer, opentelemetry::trace::TraceError> {
        self.install_batch().await
    }

    pub async fn install_simple(
        self,
    ) -> Result<opentelemetry_sdk::trace::Tracer, opentelemetry::trace::TraceError> {
        self.install().await
    }

    pub async fn install_batch(
        self,
    ) -> Result<opentelemetry_sdk::trace::Tracer, opentelemetry::trace::TraceError> {
        let provider = {
            let exporter = GcpCloudTraceExporter::new(
                &self.google_project_id,
                self.resource
                    .unwrap_or_else(|| Resource::builder_empty().build()),
            )
            .await?;

            self.tracer_provider_builder
                .unwrap_or_default()
                .with_batch_exporter(exporter)
                .build()
        };

        let scope = InstrumentationScope::builder("opentelemetry-gcloud")
            .with_version(env!("CARGO_PKG_VERSION"))
            .with_schema_url("https://opentelemetry.io/schemas/1.23.0")
            .build();

        let tracer = provider.tracer_with_scope(scope);

        let _ = opentelemetry::global::set_tracer_provider(provider);
        Ok(tracer)
    }
}
