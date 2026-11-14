use super::GcpOtlpExporterBuilder;
use crate::errors::TraceExportResult;
use opentelemetry_otlp::LogExporter;
use opentelemetry_sdk::logs::log_processor_with_async_runtime::BatchLogProcessor;
use opentelemetry_sdk::logs::{BatchConfigBuilder, LoggerProviderBuilder, SdkLoggerProvider};
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::Resource;

impl GcpOtlpExporterBuilder {
    pub async fn log_exporter(&self) -> TraceExportResult<LogExporter> {
        let exporter = self
            .configure_exporter(LogExporter::builder().with_tonic())
            .await?;
        Ok(exporter.build()?)
    }

    pub async fn create_logger_provider(&self) -> TraceExportResult<SdkLoggerProvider> {
        self.create_logger_provider_from_builder(
            SdkLoggerProvider::builder().with_resource(Resource::builder().build()),
        )
        .await
    }

    /// Adds [`Self::resource`], `gcp.project_id` and a batch processor over
    /// [`Self::log_exporter`] to `builder`. The processor runs on Tokio, which the tonic
    /// exporter needs, rather than on a thread of its own.
    pub async fn create_logger_provider_from_builder(
        &self,
        builder: LoggerProviderBuilder,
    ) -> TraceExportResult<SdkLoggerProvider> {
        let mut config = BatchConfigBuilder::default();
        if let Some(max_export_timeout) = self.max_export_timeout {
            config = config.with_max_export_timeout(max_export_timeout);
        }
        let processor = BatchLogProcessor::builder(self.log_exporter().await?, runtime::Tokio)
            .with_batch_config(config.build())
            .build();
        Ok(self
            .provider_resources()
            .fold(builder, LoggerProviderBuilder::with_resource)
            .with_log_processor(processor)
            .build())
    }
}
