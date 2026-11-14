use super::GcpOtlpExporterBuilder;
use crate::common;
use crate::errors::TraceExportResult;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::{SdkTracerProvider, Tracer, TracerProviderBuilder};

impl GcpOtlpExporterBuilder {
    pub async fn span_exporter(&self) -> TraceExportResult<SpanExporter> {
        let exporter = self
            .configure_exporter(SpanExporter::builder().with_tonic())
            .await?;
        Ok(exporter.build()?)
    }

    pub async fn create_provider(&self) -> TraceExportResult<SdkTracerProvider> {
        self.create_provider_from_builder(SdkTracerProvider::builder())
            .await
    }

    /// Adds the resource of [`Self::resource`] and a batch processor over
    /// [`Self::span_exporter`] to `builder`.
    pub async fn create_provider_from_builder(
        &self,
        builder: TracerProviderBuilder,
    ) -> TraceExportResult<SdkTracerProvider> {
        let exporter = self.span_exporter().await?;
        let [resource, project] = self.provider_resources();
        Ok(builder
            .with_resource(resource)
            .with_resource(project)
            .with_span_processor(common::batch_span_processor(
                exporter,
                self.max_export_timeout,
            ))
            .build())
    }

    pub fn install(&self, provider: &SdkTracerProvider) -> Tracer {
        common::tracer(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otlp::testing::ScriptedAccessToken;
    use opentelemetry::{Context, Key, Value};
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::{Span, SpanData, SpanProcessor};
    use opentelemetry_sdk::Resource;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Keeps the resource its provider hands it.
    #[derive(Debug, Clone, Default)]
    struct ResourceProbe(Arc<Mutex<Option<Resource>>>);

    impl SpanProcessor for ResourceProbe {
        fn on_start(&self, _span: &mut Span, _cx: &Context) {}

        fn on_end(&self, _span: SpanData) {}

        fn force_flush(&self) -> OTelSdkResult {
            Ok(())
        }

        fn shutdown_with_timeout(&self, _timeout: Duration) -> OTelSdkResult {
            Ok(())
        }

        fn set_resource(&mut self, resource: &Resource) {
            *self.0.lock().unwrap() = Some(resource.clone());
        }
    }

    // Multi-threaded: dropping the provider shuts its batch processor down by blocking on
    // the processor's task.
    #[tokio::test(flavor = "multi_thread")]
    async fn provider_resource_names_the_project_and_keeps_the_service_name() {
        let probe = ResourceProbe::default();
        let builder = GcpOtlpExporterBuilder::new("orders-project".to_string())
            .with_resource(Resource::builder().with_service_name("orders").build())
            .with_auth_headers(ScriptedAccessToken::new("first-token").auth_headers());

        let _provider = builder
            .create_provider_from_builder(
                SdkTracerProvider::builder().with_span_processor(probe.clone()),
            )
            .await
            .unwrap();

        let resource = probe
            .0
            .lock()
            .unwrap()
            .clone()
            .expect("the provider hands its processors a resource");
        assert_eq!(
            resource.get(&Key::from_static_str("gcp.project_id")),
            Some(Value::from("orders-project"))
        );
        assert_eq!(
            resource.get(&Key::from_static_str("service.name")),
            Some(Value::from("orders"))
        );
    }
}
