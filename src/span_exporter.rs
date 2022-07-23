use crate::google_trace_exporter_client::GcpCloudTraceExporterClient;
use crate::TraceExportResult;
use futures_util::future::TryFutureExt;
use opentelemetry::sdk::export::trace::{ExportResult, SpanData, SpanExporter};
use std::fmt::Formatter;

pub struct GcpCloudTraceExporter {
    gcp_export_client: GcpCloudTraceExporterClient,
}

impl GcpCloudTraceExporter {
    pub async fn new(google_project_id: &String) -> TraceExportResult<Self> {
        Ok(Self {
            gcp_export_client: GcpCloudTraceExporterClient::new(google_project_id).await?,
        })
    }
}

impl std::fmt::Debug for GcpCloudTraceExporter {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "GcpCloudTraceExporter")
    }
}

#[async_trait::async_trait]
impl SpanExporter for GcpCloudTraceExporter {
    async fn export(&mut self, batch: Vec<SpanData>) -> ExportResult {
        self.gcp_export_client
            .export_batch(batch)
            .map_err(|e| e.into())
            .await
    }
}
