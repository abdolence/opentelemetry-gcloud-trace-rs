use super::GcpOtlpExporterBuilder;
use crate::errors::GcloudObservabilityResult;
use opentelemetry_otlp::LogExporter;
use opentelemetry_sdk::logs::{LoggerProviderBuilder, SdkLoggerProvider};
use opentelemetry_sdk::Resource;

impl GcpOtlpExporterBuilder {
    pub async fn log_exporter(&self) -> GcloudObservabilityResult<LogExporter> {
        let exporter = self
            .configure_exporter(LogExporter::builder().with_tonic())
            .await?;
        Ok(exporter.build()?)
    }

    pub async fn create_logger_provider(&self) -> GcloudObservabilityResult<SdkLoggerProvider> {
        self.create_logger_provider_from_builder(
            SdkLoggerProvider::builder().with_resource(Resource::builder().build()),
        )
        .await
    }

    /// Adds [`Self::resource`], `gcp.project_id` and a batch processor over
    /// [`Self::log_exporter`] to `builder`. The processor runs on a thread of its own and
    /// reports a failed export as an `ERROR` `tracing` event with the `opentelemetry_sdk`
    /// target.
    pub async fn create_logger_provider_from_builder(
        &self,
        builder: LoggerProviderBuilder,
    ) -> GcloudObservabilityResult<SdkLoggerProvider> {
        Ok(self
            .provider_resources()
            .fold(builder, LoggerProviderBuilder::with_resource)
            .with_batch_exporter(self.log_exporter().await?)
            .build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otlp::testing::ScriptedAccessToken;
    use opentelemetry::logs::{LogRecord, Logger, LoggerProvider};
    use std::sync::{Arc, Mutex};
    use tracing::{Event, Level, Subscriber};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::{Layer, Registry};

    struct ErrorTargets(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber> Layer<S> for ErrorTargets {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            if *event.metadata().level() == Level::ERROR {
                self.0
                    .lock()
                    .unwrap()
                    .push(event.metadata().target().to_string());
            }
        }
    }

    // The global default, because the batch processor reports from a thread that a scoped
    // subscriber does not reach. No other unit test may set it.
    // Multi-threaded: flushing blocks the calling thread until the batch processor is done,
    // and the exporter's channel is driven by this runtime.
    #[tokio::test(flavor = "multi_thread")]
    async fn failed_export_is_reported_through_tracing() {
        let error_targets = Arc::new(Mutex::new(Vec::new()));
        tracing::subscriber::set_global_default(
            Registry::default().with(ErrorTargets(Arc::clone(&error_targets))),
        )
        .unwrap();
        let unreachable_endpoint = "http://127.0.0.1:1";
        let provider = GcpOtlpExporterBuilder::new("orders-project".to_string())
            .with_endpoint(unreachable_endpoint.to_string())
            .with_auth_headers(ScriptedAccessToken::new("first-token").auth_headers())
            .create_logger_provider()
            .await
            .unwrap();
        let logger = provider.logger("test");
        let mut record = logger.create_log_record();
        record.set_body("order placed".into());
        logger.emit(record);

        let _ = tokio::task::block_in_place(|| provider.force_flush());

        assert!(error_targets
            .lock()
            .unwrap()
            .iter()
            .any(|target| target.starts_with("opentelemetry_sdk")));
    }
}
