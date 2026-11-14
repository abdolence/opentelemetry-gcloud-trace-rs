//! A [`tracing_subscriber::Layer`] that writes structured log entries for
//! Google Cloud Logging, correlated to Cloud Trace.
//!
//! Correlation reads the span context that `tracing-opentelemetry`'s layer
//! attaches to each span through its public
//! [`tracing_opentelemetry::get_otel_context`] API. **Register that layer
//! before this one** on the subscriber: a layer only sees context attached by
//! layers registered ahead of it, so registering this layer first, or
//! omitting the OpenTelemetry layer, silently disables `trace`/`spanId` on
//! every log line (a warning is printed once to stderr when that happens).
//! Wrapping this layer in `Option`, `Box<dyn Layer>` or a per-layer filter is
//! supported and needs no extra step. One caveat: under a scoped
//! (`tracing::subscriber::with_default`) subscriber with the layer wrapped
//! that way, correlation instead follows whatever tracing span is currently
//! entered on the thread, so an event's explicit `parent:` is not honoured in
//! that specific configuration.
//!
//! ```ignore
//! use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id).build()?;
//!
//! let subscriber = tracing_subscriber::registry::Registry::default()
//!     .with(tracing_opentelemetry::layer().with_tracer(tracer))
//!     .with(log_layer);
//! ```
//!
//! An event field passed as a `&dyn Error` keeps its whole `source()` chain, one `Caused by:`
//! line per source; `error = %err` and `error = ?err` record only the text they format.
//! [`GcpCloudLoggingLayerBuilder::with_error_reporting`] makes every event at ERROR severity
//! or above an event Cloud Error Reporting groups, with the code location, the error chain
//! and a backtrace in `message`; set `RUST_BACKTRACE=1` or `RUST_LIB_BACKTRACE=1` to capture
//! the backtrace. [`install_panic_hook`] reports panics the same way.
//!
//! Three sinks are available and exactly one is chosen when the layer is built:
//! JSON lines to a writer (default stdout, for hosts whose logging agent
//! collects them), the Cloud Logging API under the `logs-api` feature (for
//! hosts with no agent), or an OpenTelemetry logger provider under the
//! `logs-otlp` feature, which exports to the Google Cloud Telemetry API.
//! [`GcpCloudLoggingLayer`] holds its sink behind a
//! private enum rather than a type parameter, so callers compose subscribers
//! the same way regardless of which sink a layer was built with.

#[cfg(feature = "logs-api")]
mod api;
mod error_reporting;
mod format;
mod json;
#[cfg(feature = "logs-otlp")]
mod otlp;

#[cfg(feature = "logs-api")]
pub use api::{GcpCloudLoggingApiConfig, GcpCloudLoggingHandle, MonitoredResource};

#[cfg(feature = "logs-otlp")]
pub use otlp::OtlpSink;

use crate::common;
use crate::errors::GcloudObservabilityResult;
#[cfg(feature = "logs-otlp")]
use crate::ids::LogId;
pub use error_reporting::{install_panic_hook, ServiceContext};
use json::JsonSink;
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
    #[cfg(feature = "logs-otlp")]
    Otlp(OtlpSink),
}

impl LogSink {
    fn exports_over_network(&self) -> bool {
        match self {
            Self::Json(_) => false,
            #[cfg(feature = "logs-api")]
            Self::CloudLoggingApi(_) => true,
            #[cfg(feature = "logs-otlp")]
            Self::Otlp(_) => true,
        }
    }
}

/// The crates whose events describe the connection a network sink exports over. At DEBUG
/// or TRACE every export makes some, and an entry made of one is another export to make.
const TRANSPORT_CRATES: [&str; 10] = [
    "h2",
    "hyper",
    "hyper_util",
    "tonic",
    "tower",
    "rustls",
    "gcloud_sdk",
    "opentelemetry",
    "opentelemetry_sdk",
    "opentelemetry_otlp",
];

fn is_transport_target(target: &str) -> bool {
    target
        .split("::")
        .next()
        .is_some_and(|crate_name| TRANSPORT_CRATES.contains(&crate_name))
}

/// Builds a [`GcpCloudLoggingLayer`].
///
/// `S` is the sink the layer is built for: a `MakeWriter` for JSON lines, a
/// `GcpCloudLoggingApiConfig` (feature `logs-api`) or an `OtlpSink` (feature
/// `logs-otlp`). Each has its own build method, `build` for JSON lines and
/// OpenTelemetry, `build_async` for the Cloud Logging API, so a builder
/// becomes exactly one sink, which the compiler checks rather than a runtime
/// test.
pub struct GcpCloudLoggingLayerBuilder<S = fn() -> std::io::Stdout> {
    google_project_id: String,
    with_source_location: bool,
    error_reporting: Option<ServiceContext>,
    sink: S,
}

impl GcpCloudLoggingLayerBuilder<fn() -> std::io::Stdout> {
    /// Starts a builder writing JSON lines to stdout, with source location
    /// enabled.
    pub fn new(google_project_id: impl Into<String>) -> Self {
        Self {
            google_project_id: google_project_id.into(),
            with_source_location: true,
            error_reporting: None,
            sink: std::io::stdout,
        }
    }

    /// Starts a builder for the project of the environment the process runs in, as
    /// [`gcloud_sdk::GoogleEnvironment::detect_google_project_id`] finds it.
    pub async fn for_default_project_id() -> GcloudObservabilityResult<Self> {
        Ok(Self::new(common::detect_google_project_id().await?))
    }
}

impl<S> GcpCloudLoggingLayerBuilder<S> {
    /// Adds `logging.googleapis.com/sourceLocation` (file and line) to every
    /// log line. Enabled by default.
    pub fn with_source_location(mut self, enabled: bool) -> Self {
        self.with_source_location = enabled;
        self
    }

    /// Makes every event at ERROR severity or above (including a `severity` field of
    /// `critical`, `alert` or `emergency`) a Cloud Error Reporting event for `service`.
    ///
    /// The entry gets `@type`, `serviceContext`, `context.reportLocation` from the event's
    /// code location, and `context.httpRequest` when `http_request.*` fields are present. Its
    /// `message` is the event message and the error's `Display` and `source()` chain on one
    /// line, `message: outer: middle: root`, then the backtrace, which is captured only when
    /// `RUST_BACKTRACE` or `RUST_LIB_BACKTRACE` enables it. Errors with the same first line
    /// are one group in Error Reporting, wherever they were logged.
    ///
    /// On the OTLP sink the same keys go in the body, and the `exception.type` and
    /// `exception.message` attributes are added. A `&dyn Error` has no type name on stable
    /// Rust, so `exception.type` is set from an `exception.type` event field when there is
    /// one.
    pub fn with_error_reporting(mut self, service: ServiceContext) -> Self {
        self.error_reporting = Some(service);
        self
    }

    /// Sets where JSON lines are written. Defaults to [`std::io::stdout`].
    pub fn with_json_writer<W2>(self, make_writer: W2) -> GcpCloudLoggingLayerBuilder<W2> {
        GcpCloudLoggingLayerBuilder {
            google_project_id: self.google_project_id,
            with_source_location: self.with_source_location,
            error_reporting: self.error_reporting,
            sink: make_writer,
        }
    }

    /// Switches the layer to write through the Cloud Logging API instead of
    /// JSON lines. The returned builder has [`Self::build`] replaced by
    /// `build_async`, so a sink is chosen exactly once and cannot be
    /// contradicted later.
    ///
    /// Events of the crates the writes travel through are not written: those whose target is
    /// in `h2`, `hyper`, `hyper_util`, `tonic`, `tower`, `rustls`, `gcloud_sdk`,
    /// `opentelemetry`, `opentelemetry_sdk` or `opentelemetry_otlp`. At DEBUG or TRACE every
    /// write would make events that are written in turn, without end. The JSON sink writes
    /// them.
    #[cfg(feature = "logs-api")]
    pub fn with_cloud_logging_api(
        self,
        config: GcpCloudLoggingApiConfig,
    ) -> GcpCloudLoggingLayerBuilder<GcpCloudLoggingApiConfig> {
        GcpCloudLoggingLayerBuilder {
            google_project_id: self.google_project_id,
            with_source_location: self.with_source_location,
            error_reporting: self.error_reporting,
            sink: config,
        }
    }

    /// Switches the layer to send entries through an OpenTelemetry logger
    /// provider, normally the one
    /// [`GcpOtlpExporterBuilder::create_logger_provider`](crate::otlp::GcpOtlpExporterBuilder::create_logger_provider)
    /// builds for the Google Cloud Telemetry API. `log_id` names the log the entries are
    /// written to, as for the Cloud Logging API sink. The returned builder has a
    /// synchronous [`Self::build`], and the caller flushes and shuts the provider down.
    ///
    /// Flushing and shutting down block the calling thread, and need a Tokio runtime thread
    /// free to drive the export, so they wait out their timeout without exporting on a
    /// current-thread runtime. Call them on a multi-threaded runtime, or from a thread outside
    /// the runtime.
    ///
    /// Events of the crates the exports travel through are not sent: those whose target is in
    /// `h2`, `hyper`, `hyper_util`, `tonic`, `tower`, `rustls`, `gcloud_sdk`, `opentelemetry`,
    /// `opentelemetry_sdk` or `opentelemetry_otlp`. At DEBUG or TRACE every export would make
    /// events that are exported in turn, without end. The JSON sink writes them.
    #[cfg(feature = "logs-otlp")]
    pub fn with_otel_logger_provider(
        self,
        provider: &opentelemetry_sdk::logs::SdkLoggerProvider,
        log_id: LogId,
    ) -> GcpCloudLoggingLayerBuilder<OtlpSink> {
        use opentelemetry::logs::LoggerProvider;
        GcpCloudLoggingLayerBuilder {
            google_project_id: self.google_project_id,
            with_source_location: self.with_source_location,
            error_reporting: self.error_reporting,
            sink: OtlpSink::new(
                provider.logger_with_scope(crate::common::instrumentation_scope()),
                log_id,
            ),
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
    ///
    /// # Errors
    ///
    /// [`InvalidParametersError`] for the field `google_project_id` if it is empty or contains
    /// `/` or a control character, before the Cloud Logging API is contacted.
    ///
    /// [`InvalidParametersError`]: crate::errors::GcloudObservabilityError::InvalidParametersError
    pub async fn build_async(
        self,
    ) -> GcloudObservabilityResult<(GcpCloudLoggingLayer, GcpCloudLoggingHandle)> {
        common::validate_google_project_id(&self.google_project_id)?;
        let config = self.sink;
        let resource = config.resource_or_default(&self.google_project_id);
        let log_name = config.log_name(&self.google_project_id);
        let sink = std::sync::Arc::new(api::GcloudLogEntrySink::new().await?);
        let (api_sink, handle) = api::spawn(&config, log_name, resource, sink);
        Ok((
            GcpCloudLoggingLayer {
                google_project_id: self.google_project_id,
                with_source_location: self.with_source_location,
                error_reporting: self.error_reporting,
                sink: LogSink::CloudLoggingApi(api_sink),
                dispatch: OnceLock::new(),
                missing_otel_layer_warned: Once::new(),
            },
            handle,
        ))
    }
}

#[cfg(feature = "logs-otlp")]
impl GcpCloudLoggingLayerBuilder<OtlpSink> {
    /// Builds the layer. Register it on a subscriber after the
    /// `tracing-opentelemetry` layer to get trace correlation.
    ///
    /// # Errors
    ///
    /// [`InvalidParametersError`] for the field `google_project_id` if it is empty or contains
    /// `/` or a control character.
    ///
    /// [`InvalidParametersError`]: crate::errors::GcloudObservabilityError::InvalidParametersError
    pub fn build(self) -> GcloudObservabilityResult<GcpCloudLoggingLayer> {
        common::validate_google_project_id(&self.google_project_id)?;
        Ok(GcpCloudLoggingLayer {
            google_project_id: self.google_project_id,
            with_source_location: self.with_source_location,
            error_reporting: self.error_reporting,
            sink: LogSink::Otlp(self.sink),
            dispatch: OnceLock::new(),
            missing_otel_layer_warned: Once::new(),
        })
    }
}

impl<S> GcpCloudLoggingLayerBuilder<S>
where
    S: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    /// Builds the layer. Register it on a subscriber after the
    /// `tracing-opentelemetry` layer to get trace correlation.
    ///
    /// # Errors
    ///
    /// [`InvalidParametersError`] for the field `google_project_id` if it is empty or contains
    /// `/` or a control character.
    ///
    /// [`InvalidParametersError`]: crate::errors::GcloudObservabilityError::InvalidParametersError
    pub fn build(self) -> GcloudObservabilityResult<GcpCloudLoggingLayer> {
        common::validate_google_project_id(&self.google_project_id)?;
        Ok(GcpCloudLoggingLayer {
            google_project_id: self.google_project_id,
            with_source_location: self.with_source_location,
            error_reporting: self.error_reporting,
            sink: LogSink::Json(JsonSink::new(self.sink)),
            dispatch: OnceLock::new(),
            missing_otel_layer_warned: Once::new(),
        })
    }
}

/// A [`tracing_subscriber::Layer`] that turns every event into one Cloud
/// Logging entry.
pub struct GcpCloudLoggingLayer {
    google_project_id: String,
    with_source_location: bool,
    error_reporting: Option<ServiceContext>,
    sink: LogSink,
    // Captured from `on_register_dispatch`, needed because a nested
    // `tracing::dispatcher::get_default` inside `on_event` is documented to
    // see `Dispatch::none()` under a scoped (`with_default`) subscriber
    // instead of the real one. A wrapper between this layer and the
    // subscriber (`Option`, `Box<dyn Layer>`, a per-layer filter) may not
    // forward `on_register_dispatch`, though, so `on_event` falls back past
    // this - to `get_default` (which still works for a *global*-default
    // subscriber, whose fast path ignores the scoped state) and then to the
    // ambient OpenTelemetry context - when this is empty.
    dispatch: OnceLock<WeakDispatch>,
    missing_otel_layer_warned: Once,
}

/// The OpenTelemetry context active on this thread, if one is and it names a
/// valid span. Meaningful even when the caller reached it with no enclosing
/// tracing span to look up, since `tracing-opentelemetry`'s context
/// activation (on by default) sets this independently of that layer's own
/// dispatch or of `tracing`'s per-thread scoped-dispatch state.
fn current_active_otel_context() -> Option<opentelemetry::Context> {
    use opentelemetry::trace::TraceContextExt;
    let current = opentelemetry::Context::current();
    (current.has_active_span() && current.span().span_context().is_valid()).then_some(current)
}

impl GcpCloudLoggingLayer {
    fn warn_missing_otel_layer(&self) {
        self.missing_otel_layer_warned.call_once(|| {
            eprintln!(
                "gcloud-observability: trace correlation is disabled for this event \
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
        if self.sink.exports_over_network() && is_transport_target(event.metadata().target()) {
            return;
        }

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

        // Three ways to reach the span context, tried in order, because none
        // alone covers every way this layer can be composed:
        // - the dispatch captured in `on_register_dispatch` is `None` when a
        //   wrapper (`Option`, `Box<dyn Layer>`, a per-layer filter) sits
        //   between this layer and the subscriber and does not forward that
        //   call;
        // - `tracing::dispatcher::get_default` recovers it in that case for a
        //   global-default subscriber, since its fast path returns the real
        //   dispatch instead of `Dispatch::none()` even nested inside the
        //   dispatch that delivered this event;
        // - neither sees anything under a scoped (`with_default`) subscriber
        //   with the layer wrapped, so the OpenTelemetry context active on
        //   this thread - set on span entry by `tracing-opentelemetry`'s
        //   context activation, unless the caller disabled it - is the last
        //   resort. It follows the currently entered span rather than an
        //   event's explicit `parent:`.
        let otel_context = resolved_span_id
            .as_ref()
            .and_then(|span_id| {
                self.dispatch
                    .get()
                    .and_then(WeakDispatch::upgrade)
                    .and_then(|dispatch| {
                        tracing_opentelemetry::get_otel_context(span_id, &dispatch)
                    })
                    .or_else(|| {
                        tracing::dispatcher::get_default(|dispatch| {
                            tracing_opentelemetry::get_otel_context(span_id, dispatch)
                        })
                    })
            })
            .or_else(current_active_otel_context);

        let correlation = match otel_context {
            Some(cx) => {
                use opentelemetry::trace::TraceContextExt;
                let span_context = cx.span().span_context().clone();
                span_context.is_valid().then(|| format::TraceCorrelation {
                    trace_id: span_context.trace_id(),
                    span_id: span_context.span_id(),
                    sampled: span_context.is_sampled(),
                })
            }
            None => {
                if resolved_span_id.is_some() {
                    self.warn_missing_otel_layer();
                }
                None
            }
        };

        let record = format::build_log_record(
            event,
            &self.google_project_id,
            self.with_source_location,
            correlation.as_ref(),
            self.error_reporting.as_ref(),
        );

        match &self.sink {
            LogSink::Json(sink) => sink.write(&record.fields, event.metadata()),
            #[cfg(feature = "logs-api")]
            LogSink::CloudLoggingApi(sink) => sink.send(record.fields),
            #[cfg(feature = "logs-otlp")]
            LogSink::Otlp(sink) => sink.send(
                record.fields,
                record.exception.as_ref(),
                correlation.as_ref(),
            ),
        }
    }
}
