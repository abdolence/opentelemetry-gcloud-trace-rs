use crate::google_trace_exporter_client::GcpCloudTraceExporterClient;
use crate::TraceExportResult;
use futures::future::{BoxFuture, TryFutureExt};
use futures::FutureExt;
use opentelemetry_sdk::{
    export::trace::{ExportResult, SpanData, SpanExporter},
    Resource,
};
use std::fmt::Formatter;
use std::sync::Arc;

pub struct GcpCloudTraceExporter {
    gcp_export_client: Arc<GcpCloudTraceExporterClient>,
}

impl GcpCloudTraceExporter {
    pub async fn new(google_project_id: &str, resource: Resource) -> TraceExportResult<Self> {
        Ok(Self {
            gcp_export_client: Arc::new(
                GcpCloudTraceExporterClient::new(google_project_id, resource).await?,
            ),
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
        async move { client.export_batch(batch).map_err(|e| e.into()).await }.boxed()
    }
}
