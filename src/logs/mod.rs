//! A [`tracing_subscriber::Layer`] that writes structured JSON log lines for
//! Google Cloud Logging, correlated to Cloud Trace.
//!
//! Correlation reads the span context that `tracing-opentelemetry`'s layer
//! attaches to each span through its public
//! [`tracing_opentelemetry::get_otel_context`] API. **Register that layer
//! before this one** on the subscriber: a layer only sees context attached by
//! layers registered ahead of it, so registering this layer first, or
//! omitting the OpenTelemetry layer, silently disables `trace`/`spanId` on
//! every log line (a warning is printed once to stderr when that happens).
//!
//! ```ignore
//! use opentelemetry_gcloud_trace::logs::GcpCloudLoggingLayerBuilder;
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! let log_layer = GcpCloudLoggingLayerBuilder::new(project_id).build();
//!
//! let subscriber = tracing_subscriber::registry::Registry::default()
//!     .with(tracing_opentelemetry::layer().with_tracer(tracer))
//!     .with(log_layer);
//! ```
//!
//! Two sinks are available and exactly one is chosen when the layer is built:
//! JSON lines to a writer (default stdout, for hosts whose logging agent
//! collects them), or the Cloud Logging API under the `logs-api` feature (for
//! hosts with no agent). [`GcpCloudLoggingLayer`] holds its sink behind a
//! private enum rather than a type parameter, so callers compose subscribers
//! the same way regardless of which sink a layer was built with.

#[cfg(feature = "logs-api")]
mod api;
mod format;
mod json_sink;

#[cfg(feature = "logs-api")]
pub use api::{GcpCloudLoggingApiConfig, GcpCloudLoggingHandle, MonitoredResource};

use crate::errors::{GcloudTraceError, GcloudTraceSystemError};
use crate::TraceExportResult;
use json_sink::JsonSink;
use std::sync::{Once, OnceLock};
use tracing::dispatcher::{Dispatch, WeakDispatch};
use tracing::span;
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

enum LogSink {
    Json(JsonSink),
    #[cfg(feature = "logs-api")]
    CloudLoggingApi(api::ApiSink),
}

/// Builds a [`GcpCloudLoggingLayer`].
///
/// `W` is the sink configuration accumulated so far; today it is always the
/// JSON writer type, and [`Self::build`] is defined only where `W` implements
/// `MakeWriter`. A future Cloud Logging API sink would occupy this same type
/// parameter with a non-`MakeWriter` config and expose its own `build_async`,
/// so the two build methods stay mutually exclusive at the type level rather
/// than by runtime check.
pub struct GcpCloudLoggingLayerBuilder<W = fn() -> std::io::Stdout> {
    project_id: String,
    with_source_location: bool,
    make_writer: W,
}

impl GcpCloudLoggingLayerBuilder<fn() -> std::io::Stdout> {
    /// Starts a builder writing JSON lines to stdout, with source location
    /// enabled.
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            with_source_location: true,
            make_writer: std::io::stdout,
        }
    }

    /// Detects the Google Cloud project id from the environment or the
    /// default service account, the same way [`crate::GcpCloudTraceExporterBuilder::for_default_project_id`]
    /// does, and starts a builder from it.
    pub async fn for_default_project_id() -> TraceExportResult<Self> {
        let detected_project_id = gcloud_sdk::GoogleEnvironment::detect_google_project_id()
            .await
            .ok_or_else(|| {
                GcloudTraceError::SystemError(GcloudTraceSystemError::new(
                    "No Google Project ID detected. Please specify it explicitly using env variable: PROJECT_ID or define it as default project for your service accounts".to_string(),
                ))
            })?;
        Ok(Self::new(detected_project_id))
    }
}

impl<W> GcpCloudLoggingLayerBuilder<W> {
    /// Adds `logging.googleapis.com/sourceLocation` (file and line) to every
    /// log line. Enabled by default.
    pub fn with_source_location(mut self, enabled: bool) -> Self {
        self.with_source_location = enabled;
        self
    }

    /// Sets where JSON lines are written. Defaults to [`std::io::stdout`].
    pub fn with_json_writer<W2>(self, make_writer: W2) -> GcpCloudLoggingLayerBuilder<W2> {
        GcpCloudLoggingLayerBuilder {
            project_id: self.project_id,
            with_source_location: self.with_source_location,
            make_writer,
        }
    }

    /// Switches the layer to write through the Cloud Logging API instead of
    /// JSON lines. The returned builder has [`Self::build`] replaced by
    /// `build_async`, so a sink is chosen exactly once and cannot be
    /// contradicted later.
    #[cfg(feature = "logs-api")]
    pub fn with_cloud_logging_api(
        self,
        config: GcpCloudLoggingApiConfig,
    ) -> GcpCloudLoggingLayerBuilder<GcpCloudLoggingApiConfig> {
        GcpCloudLoggingLayerBuilder {
            project_id: self.project_id,
            with_source_location: self.with_source_location,
            make_writer: config,
        }
    }
}

#[cfg(feature = "logs-api")]
impl GcpCloudLoggingLayerBuilder<GcpCloudLoggingApiConfig> {
    /// Connects to the Cloud Logging API and starts the background task that
    /// writes batches of entries.
    ///
    /// Must be called from within a Tokio runtime, whose handle the background
    /// task is spawned on; the entries are written for as long as that runtime
    /// lives. The returned handle flushes and stops the task - see
    /// [`GcpCloudLoggingHandle`], and note that dropping it without calling
    /// `shutdown` leaves queued entries unwritten.
    pub async fn build_async(
        self,
    ) -> TraceExportResult<(GcpCloudLoggingLayer, GcpCloudLoggingHandle)> {
        let config = self.make_writer;
        let resource = config.resource_or_default(&self.project_id);
        let log_name = format!("projects/{}/logs/{}", self.project_id, config.log_id);
        let sink = std::sync::Arc::new(api::GcloudLogEntrySink::new().await?);
        let (api_sink, handle) = api::spawn(&config, log_name, resource, sink);
        Ok((
            GcpCloudLoggingLayer {
                project_id: self.project_id,
                with_source_location: self.with_source_location,
                sink: LogSink::CloudLoggingApi(api_sink),
                dispatch: OnceLock::new(),
                missing_otel_layer_warned: Once::new(),
            },
            handle,
        ))
    }
}

impl<W> GcpCloudLoggingLayerBuilder<W>
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    /// Builds the layer. Register it on a subscriber after the
    /// `tracing-opentelemetry` layer to get trace correlation.
    pub fn build(self) -> GcpCloudLoggingLayer {
        GcpCloudLoggingLayer {
            project_id: self.project_id,
            with_source_location: self.with_source_location,
            sink: LogSink::Json(JsonSink::new(self.make_writer)),
            dispatch: OnceLock::new(),
            missing_otel_layer_warned: Once::new(),
        }
    }
}

/// A [`tracing_subscriber::Layer`] that turns every event into one Cloud
/// Logging structured JSON line.
pub struct GcpCloudLoggingLayer {
    project_id: String,
    with_source_location: bool,
    sink: LogSink,
    // Captured from `on_register_dispatch` rather than read via
    // `tracing::dispatcher::get_default` inside `on_event`: that call is
    // already nested inside the dispatch that delivered the event, and a
    // nested `get_default` is documented to see `Dispatch::none()` instead of
    // the real subscriber, which would silently disable correlation.
    dispatch: OnceLock<WeakDispatch>,
    missing_otel_layer_warned: Once,
}

impl GcpCloudLoggingLayer {
    fn warn_missing_otel_layer(&self) {
        self.missing_otel_layer_warned.call_once(|| {
            eprintln!(
                "opentelemetry-gcloud-trace: trace correlation is disabled for this event \
                 because no tracing-opentelemetry layer is registered, or it is registered \
                 after GcpCloudLoggingLayer; register `tracing_opentelemetry::layer()` first."
            );
        });
    }
}

impl<S> Layer<S> for GcpCloudLoggingLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_register_dispatch(&self, subscriber: &Dispatch) {
        // Ignored if already set: a layer instance is only ever registered
        // into one subscriber in practice, and keeping the first dispatch is
        // as good a choice as any if that assumption is ever violated.
        let _ = self.dispatch.set(subscriber.downgrade());
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // `get_otel_context` locks span extensions internally, so the span id
        // is copied out and the `SpanRef` borrow is dropped before calling it
        // - holding the borrow across the call risks a deadlock.
        let resolved_span_id: Option<span::Id> = {
            let span = match event.parent() {
                Some(id) => ctx.span(id),
                None => ctx.lookup_current(),
            };
            span.map(|span_ref| span_ref.id())
        };

        let mut correlation = None;
        if let Some(span_id) = resolved_span_id {
            let otel_context = self
                .dispatch
                .get()
                .and_then(WeakDispatch::upgrade)
                .and_then(|dispatch| tracing_opentelemetry::get_otel_context(&span_id, &dispatch));
            match otel_context {
                Some(cx) => {
                    use opentelemetry::trace::TraceContextExt;
                    let span_context = cx.span().span_context().clone();
                    if span_context.is_valid() {
                        correlation = Some(format::TraceCorrelation {
                            trace_id: span_context.trace_id().to_string(),
                            span_id: span_context.span_id().to_string(),
                            sampled: span_context.is_sampled(),
                        });
                    }
                }
                None => self.warn_missing_otel_layer(),
            }
        }

        let record = format::build_log_record(
            event,
            &self.project_id,
            self.with_source_location,
            correlation.as_ref(),
        );

        match &self.sink {
            LogSink::Json(sink) => sink.write(&record, event.metadata()),
            #[cfg(feature = "logs-api")]
            LogSink::CloudLoggingApi(sink) => sink.send(record),
        }
    }
}
