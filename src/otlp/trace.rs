use super::GcpOtlpExporterBuilder;
use crate::common;
use crate::errors::GcloudObservabilityResult;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{
    SdkTracerProvider, SpanData, SpanEvents, SpanExporter as SdkSpanExporter, Tracer,
    TracerProviderBuilder,
};
use opentelemetry_sdk::Resource;
use std::future::Future;
use std::time::Duration;

/// An exporter that sends spans through OTLP, leaving their events out unless
/// [`GcpOtlpExporterBuilder::span_events`] is `true`.
///
/// Created by [`GcpOtlpExporterBuilder::span_exporter`].
#[derive(Debug)]
pub struct GcpOtlpSpanExporter<E = SpanExporter> {
    inner: E,
    span_events: bool,
}

impl<E: SdkSpanExporter> SdkSpanExporter for GcpOtlpSpanExporter<E> {
    fn export(&self, mut batch: Vec<SpanData>) -> impl Future<Output = OTelSdkResult> + Send {
        if !self.span_events {
            for span in &mut batch {
                span.events = SpanEvents::default();
            }
        }
        self.inner.export(batch)
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

impl GcpOtlpExporterBuilder {
    /// The OTLP span exporter, authenticated and pointed at the Telemetry API. Spans are
    /// exported without their events unless [`Self::span_events`] is `true`.
    pub async fn span_exporter(&self) -> GcloudObservabilityResult<GcpOtlpSpanExporter> {
        let exporter = self
            .configure_exporter(SpanExporter::builder().with_tonic())
            .await?;
        Ok(self.span_exporter_over(exporter.build()?))
    }

    fn span_exporter_over<E>(&self, inner: E) -> GcpOtlpSpanExporter<E> {
        GcpOtlpSpanExporter {
            inner,
            span_events: self.span_events.unwrap_or(false),
        }
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
    use opentelemetry::trace::{
        Link, Span as _, SpanContext, SpanId, TraceFlags, TraceId, TraceState, Tracer as _,
        TracerProvider as _,
    };
    use opentelemetry::{Context, Key, KeyValue, Value};
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::trace::{
        InMemorySpanExporter, SimpleSpanProcessor, Span, SpanData, SpanProcessor,
    };
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

    /// An endpoint that takes the connection and never answers the TLS handshake.
    async fn silent_endpoint() -> TelemetryEndpoint {
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
        endpoint
    }

    fn finished_span() -> SpanData {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        drop(provider.tracer("test").start("work"));
        exporter
            .get_finished_spans()
            .unwrap()
            .into_iter()
            .next()
            .expect("the simple processor exports the span as it ends")
    }

    // Multi-threaded: shutting down blocks this thread on the batch task.
    #[tokio::test(flavor = "multi_thread")]
    async fn configured_export_timeout_bounds_shutdown_against_a_silent_endpoint() {
        let provider = GcpOtlpExporterBuilder::new(ORDERS_PROJECT.to_string())
            .with_endpoint(silent_endpoint().await)
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

    #[tokio::test]
    async fn configured_export_timeout_bounds_connecting_to_a_silent_endpoint() {
        let exporter = GcpOtlpExporterBuilder::new(ORDERS_PROJECT.to_string())
            .with_endpoint(silent_endpoint().await)
            .with_max_export_timeout(Duration::from_millis(300))
            .with_auth_headers(ScriptedAccessToken::new("first-token").auth_headers())
            .span_exporter()
            .await
            .unwrap();

        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            exporter.export(vec![finished_span()]),
        )
        .await;

        assert!(outcome
            .expect("the export gives up by itself, within its timeout")
            .is_err());
    }

    #[tokio::test]
    async fn default_export_timeout_bounds_connecting_to_a_silent_endpoint() {
        let exporter = GcpOtlpExporterBuilder::new(ORDERS_PROJECT.to_string())
            .with_endpoint(silent_endpoint().await)
            .with_default_export_timeout(Duration::from_millis(300))
            .with_auth_headers(ScriptedAccessToken::new("first-token").auth_headers())
            .span_exporter()
            .await
            .unwrap();

        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            exporter.export(vec![finished_span()]),
        )
        .await;

        assert!(outcome
            .expect("the export gives up by itself, within the default timeout")
            .is_err());
    }

    /// A span with an event, an attribute and a link.
    fn span_with_event() -> SpanData {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
            .build();
        let linked = SpanContext::new(
            TraceId::from(1),
            SpanId::from(2),
            TraceFlags::SAMPLED,
            true,
            TraceState::default(),
        );
        let tracer = provider.tracer("test");
        let mut span = tracer
            .span_builder("work")
            .with_attributes([KeyValue::new("order", 7)])
            .with_links(vec![Link::with_context(linked)])
            .start(&tracer);
        span.add_event("charged", vec![]);
        drop(span);
        exporter
            .get_finished_spans()
            .unwrap()
            .into_iter()
            .next()
            .expect("the simple processor exports the span as it ends")
    }

    async fn exported_through(builder: &GcpOtlpExporterBuilder, span: SpanData) -> SpanData {
        let sink = InMemorySpanExporter::default();
        builder
            .span_exporter_over(sink.clone())
            .export(vec![span])
            .await
            .unwrap();
        sink.get_finished_spans()
            .unwrap()
            .into_iter()
            .next()
            .expect("the exporter passes the span on")
    }

    #[tokio::test]
    async fn span_events_are_left_out_by_default() {
        let builder = GcpOtlpExporterBuilder::new(ORDERS_PROJECT.to_string());
        let sent = exported_through(&builder, span_with_event()).await;

        assert!(sent.events.is_empty());
        assert_eq!(sent.events.dropped_count, 0);
        assert_eq!(sent.links.len(), 1);
        assert_eq!(sent.attributes.len(), 1);
    }

    #[tokio::test]
    async fn span_events_are_sent_when_enabled() {
        let builder =
            GcpOtlpExporterBuilder::new(ORDERS_PROJECT.to_string()).with_span_events(true);
        let sent = exported_through(&builder, span_with_event()).await;

        assert_eq!(sent.events.len(), 1);
    }
}
