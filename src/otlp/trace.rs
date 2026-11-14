use super::GcpOtlpExporterBuilder;
use crate::common;
use crate::errors::GcloudObservabilityResult;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::{SdkTracerProvider, Tracer, TracerProviderBuilder};
use opentelemetry_sdk::Resource;

impl GcpOtlpExporterBuilder {
    pub async fn span_exporter(&self) -> GcloudObservabilityResult<SpanExporter> {
        let exporter = self
            .configure_exporter(SpanExporter::builder().with_tonic())
            .await?;
        Ok(exporter.build()?)
    }

    /// [`Self::create_provider_from_builder`] over a builder with the OpenTelemetry SDK's
    /// default resource.
    ///
    /// Shut the provider down before the process ends. `shutdown`, `force_flush` and dropping it
    /// block the calling thread until the batch task, which runs on the Tokio runtime the
    /// provider was created on, is done, so on a current-thread runtime they never return.
    /// Call them on a multi-threaded runtime, or from a thread outside the runtime.
    pub async fn create_provider(&self) -> GcloudObservabilityResult<SdkTracerProvider> {
        self.create_provider_from_builder(
            SdkTracerProvider::builder().with_resource(Resource::builder().build()),
        )
        .await
    }

    /// Adds [`Self::resource`], `gcp.project_id` and a batch processor over
    /// [`Self::span_exporter`] to `builder`.
    ///
    /// A `builder` with no resource of its own is not given the OpenTelemetry SDK's default
    /// one, as [`Self::create_provider`] gives; it ends up with only [`Self::resource`] and
    /// `gcp.project_id`.
    ///
    /// Shut the provider down before the process ends. `shutdown`, `force_flush` and dropping it
    /// block the calling thread until the batch task, which runs on the Tokio runtime the
    /// provider was created on, is done, so on a current-thread runtime they never return.
    /// Call them on a multi-threaded runtime, or from a thread outside the runtime.
    pub async fn create_provider_from_builder(
        &self,
        builder: TracerProviderBuilder,
    ) -> GcloudObservabilityResult<SdkTracerProvider> {
        let exporter = self.span_exporter().await?;
        Ok(self
            .provider_resources()
            .fold(builder, TracerProviderBuilder::with_resource)
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
    use crate::otlp::testing::{ScriptedAccessToken, ORDERS_PROJECT};
    use crate::otlp::TelemetryEndpoint;
    use opentelemetry::trace::{Tracer as _, TracerProvider as _};
    use opentelemetry::{Context, Key, Value};
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::{Span, SpanData, SpanProcessor};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tokio::net::TcpListener;

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

    impl ResourceProbe {
        fn resource(&self) -> Resource {
            self.0
                .lock()
                .unwrap()
                .clone()
                .expect("the provider hands its processors a resource")
        }
    }

    // Multi-threaded: dropping the provider shuts its batch processor down by blocking on
    // the processor's task.
    #[tokio::test(flavor = "multi_thread")]
    async fn provider_resource_names_the_project_and_keeps_the_service_name() {
        let probe = ResourceProbe::default();
        let builder = GcpOtlpExporterBuilder::new(ORDERS_PROJECT.to_string())
            .with_resource(Resource::builder().with_service_name("orders").build())
            .with_auth_headers(ScriptedAccessToken::new("first-token").auth_headers());

        let _provider = builder
            .create_provider_from_builder(
                SdkTracerProvider::builder().with_span_processor(probe.clone()),
            )
            .await
            .unwrap();

        let resource = probe.resource();
        assert_eq!(
            resource.get(&Key::from_static_str("gcp.project_id")),
            Some(Value::from(ORDERS_PROJECT))
        );
        assert_eq!(
            resource.get(&Key::from_static_str("service.name")),
            Some(Value::from("orders"))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn provider_builder_resource_wins_while_no_resource_is_set() {
        let probe = ResourceProbe::default();
        let builder = GcpOtlpExporterBuilder::new(ORDERS_PROJECT.to_string())
            .with_auth_headers(ScriptedAccessToken::new("first-token").auth_headers());

        let _provider = builder
            .create_provider_from_builder(
                SdkTracerProvider::builder()
                    .with_resource(
                        Resource::builder_empty()
                            .with_service_name("orders")
                            .build(),
                    )
                    .with_span_processor(probe.clone()),
            )
            .await
            .unwrap();

        let resource = probe.resource();
        assert_eq!(
            resource.get(&Key::from_static_str("service.name")),
            Some(Value::from("orders"))
        );
        assert_eq!(
            resource.get(&Key::from_static_str("gcp.project_id")),
            Some(Value::from(ORDERS_PROJECT))
        );
    }

    // An endpoint that takes the connection and never answers the TLS handshake. The OTLP
    // exporter's own request timeout does not cover connecting, so it is the batch processor
    // that gives up on the export.
    // Multi-threaded: shutting down blocks this thread on the batch task.
    #[tokio::test(flavor = "multi_thread")]
    async fn configured_export_timeout_bounds_shutdown_against_a_silent_endpoint() {
        let silent_server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint =
            TelemetryEndpoint::new(format!("https://{}", silent_server.local_addr().unwrap()))
                .unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((connection, _)) = silent_server.accept().await {
                held.push(connection);
            }
        });
        let provider = GcpOtlpExporterBuilder::new(ORDERS_PROJECT.to_string())
            .with_endpoint(endpoint)
            .with_max_export_timeout(Duration::from_millis(300))
            .with_auth_headers(ScriptedAccessToken::new("first-token").auth_headers())
            .create_provider()
            .await
            .unwrap();
        drop(provider.tracer("test").start("work"));

        let started = Instant::now();
        provider
            .shutdown()
            .expect_err("an abandoned export is reported");

        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
