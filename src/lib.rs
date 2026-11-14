//! # Google Cloud Trace and Cloud Logging for OpenTelemetry and tracing
//!
//! The crate covers two uses, separately or together:
//! - an OpenTelemetry span exporter for Google Cloud Trace,
//!   `trace_api::GcpCloudTraceExporterBuilder` under the `trace-api` feature;
//! - a `tracing-subscriber` layer for the `tracing` crate that writes
//!   structured Google Cloud Logging entries, correlated with the trace and
//!   span that emitted them through the public `tracing-opentelemetry`
//!   context API; see the [`logs`] module.
//!
//! The `logs` feature (default) enables the logging layer with JSON lines
//! written to stdout; `logs-api` adds a sink writing through the Cloud
//! Logging API.
//!
//! ## Performance
//!
//! For optimal performance, a batch exporter is recommended as the simple exporter will export
//! each span synchronously on drop. You can enable the `rt-tokio`, `rt-tokio-current-thread`
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
//! let gcp_trace_exporter = trace_api::GcpCloudTraceExporterBuilder::new(config_env_var("PROJECT_ID")?)
//!
//! let tracer_provider = gcp_trace_exporter.create_provider().await?;
//! let tracer: opentelemetry_sdk::trace::Tracer = gcp_trace_exporter.install(&tracer_provider).await?;
//!
//! opentelemetry::global::set_tracer_provider(tracer_provider.clone());
//!
//! tracer.in_span("doing_work_parent", |cx| {
//!   // ...
//! });
//!
//! tracer_provider.shutdown()?;
//! ```
//! ## Configuration
//!
//! You can specify trace configuration using `create_provider_from_builder`:
//!
//! ```ignore
//!    gcp_trace_exporter.create_provider_from_builder (
//!       TracerProvider::builder()
//!          .with_sampler(Sampler::AlwaysOn)
//!          .with_id_generator(RandomIdGenerator::default())
//!    )
//! ```
//!
//! you can specify resource using `with_resource`:
//! ```ignore
//!    let resources = Resource::new(vec![KeyValue::new("service.name", "my-service")]);
//!    trace_api::GcpCloudTraceExporterBuilder::new(google_project_id).with_resource(resource).await?;
//! ```
//!
//! and the longest a batch export may take using `with_max_export_timeout`:
//! ```ignore
//!    trace_api::GcpCloudTraceExporterBuilder::new(google_project_id).with_max_export_timeout(Duration::from_secs(3));
//! ```
//!
//! Have a look at full examples in the `examples` directory.
//!

#![allow(unused_parens, clippy::new_without_default, clippy::needless_update)]

pub mod errors;

#[cfg(feature = "logs")]
pub mod logs;

#[cfg(feature = "trace-api")]
pub mod trace_api;
