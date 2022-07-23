// use opentelemetry::{
//     global::handle_error,
//     sdk::{
//         export::{
//             trace::{ExportResult, SpanData, SpanExporter},
//             ExportError,
//         },
//         trace::EvictedHashMap,
//     },
//     trace::TraceError,
//     Key, Value,
// };
// use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
// use opentelemetry_semantic_conventions::trace::*;

pub mod errors;
mod trace_exporter;

use crate::errors::GcloudTraceError;
pub use trace_exporter::*;

pub type TraceExportResult<E> = Result<E, GcloudTraceError>;
