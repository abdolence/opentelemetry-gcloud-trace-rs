//! # Google Cloud observability for Rust
//!
//! Sends what a service emits through `tracing` and OpenTelemetry to Google Cloud: Cloud
//! Logging, Cloud Trace and Error Reporting. Formerly `opentelemetry-gcloud-trace`.
//!
//! The guide, which covers each goal below, is at
//! <https://gcloud-observability-rust.abdolence.dev/>.
//!
//! ## What each feature gives
//!
//! - `logs` (default): the [`logs`] module, a `tracing-subscriber` layer that writes one
//!   structured Cloud Logging entry per event, correlated with the trace and span that
//!   emitted it. It writes JSON lines to stdout, which the Cloud Logging agent of GKE,
//!   Cloud Run and GCE collects. Error Reporting support, with backtraces and a panic hook,
//!   is part of it and works on every sink.
//! - `logs-api`: a sink for the layer that writes through the Cloud Logging API, for hosts with
//!   no logging agent.
//! - `otlp`: the [`otlp`] module, OpenTelemetry exporters for the Google Cloud Telemetry API
//!   (`telemetry.googleapis.com`), which takes OTLP, the OpenTelemetry wire protocol, and
//!   [`otlp::GcpAuthInterceptor`], which authenticates any OTLP exporter with Google
//!   Application Default Credentials, metrics exporters included.
//! - `logs-otlp`: a sink for the layer that sends entries over OTLP to the Telemetry API; it
//!   includes `logs` and `otlp`.
//! - `trace-api`: the [`trace_api`] module, an OpenTelemetry span exporter for the Cloud Trace
//!   API v2 (`cloudtrace.googleapis.com`).
//! - `tls-roots` (default), `tls-webpki-roots` and `auth-default-crypto` (default) choose the
//!   TLS root certificates and the rustls crypto provider.
//!
//! No exporter is enabled by default. Nothing is re-exported at the crate root; [`errors`]
//! holds the error types, and [`ids`] the `LogId` the Cloud Logging API and OTLP sinks take.
//!
//! ## Example
//!
//! Spans exported through OTLP, and every event written as a Cloud Logging JSON line carrying
//! the trace and span it happened in:
//!
//! ```ignore
//! use gcloud_observability::logs::GcpCloudLoggingLayerBuilder;
//! use gcloud_observability::otlp::GcpOtlpExporterBuilder;
//! use tracing_subscriber::layer::SubscriberExt;
//! use tracing_subscriber::{EnvFilter, Registry};
//!
//! let otlp = GcpOtlpExporterBuilder::new(google_project_id.clone());
//! let tracer_provider = otlp.create_provider().await?;
//! let tracer = otlp.install(&tracer_provider);
//!
//! let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id).build()?;
//!
//! // The OpenTelemetry layer goes first: the logging layer reads the span context it attaches.
//! let subscriber = Registry::default()
//!     .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
//!     .with(tracing_opentelemetry::layer().with_tracer(tracer))
//!     .with(log_layer);
//! tracing::subscriber::set_global_default(subscriber)?;
//!
//! // Exports the spans still queued; blocks, so it needs a multi-threaded Tokio runtime.
//! tracer_provider.shutdown()?;
//! ```
//!
//! Full examples, one for each goal, are in the `examples` directory.

#![allow(unused_parens, clippy::new_without_default, clippy::needless_update)]

#[cfg(any(feature = "logs", feature = "trace-api", feature = "otlp"))]
mod common;
pub mod errors;
#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
pub mod ids;

#[cfg(feature = "logs")]
pub mod logs;

#[cfg(feature = "otlp")]
pub mod otlp;

#[cfg(feature = "trace-api")]
pub mod trace_api;
