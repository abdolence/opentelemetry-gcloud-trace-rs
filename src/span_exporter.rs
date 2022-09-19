use crate::google_trace_exporter_client::GcpCloudTraceExporterClient;
use crate::TraceExportResult;
use futures_util::future::{BoxFuture, TryFutureExt};
use opentelemetry::sdk::export::trace::{ExportResult, SpanData, SpanExporter};
use std::fmt::Formatter;

pub struct GcpCloudTraceExporter {
    gcp_export_client: GcpCloudTraceExporterClient,
}

impl GcpCloudTraceExporter {
    pub async fn new(google_project_id: &str) -> TraceExportResult<Self> {
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

impl SpanExporter for GcpCloudTraceExporter {
    fn export(&mut self, batch: Vec<SpanData>) -> BoxFuture<'static, ExportResult> {
        let client = self.gcp_export_client.clone();
        Box::pin(gcp_export(client, batch))
    }
}

async fn gcp_export(client: GcpCloudTraceExporterClient, batch: Vec<SpanData>) -> ExportResult {
    client.export_batch(batch).map_err(|e| e.into()).await
}
