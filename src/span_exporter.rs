use crate::errors::*;
use crate::google_trace_exporter_client::GcpCloudTraceExporterClient;
use crate::TraceExportResult;
use futures::future::{BoxFuture, TryFutureExt};
use gcloud_sdk::GoogleEnvironment;
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

    pub async fn for_default_project_id() -> TraceExportResult<Self> {
        let detected_project_id = GoogleEnvironment::detect_google_project_id().await.ok_or_else(||
            GcloudTraceError::SystemError(
                GcloudTraceSystemError::new(
                    "No Google Project ID detected. Please specify it explicitly using env variable: PROJECT_ID or define it as default project for your service accounts".to_string()
                )
            )
        )?;
        Self::new(detected_project_id.as_str()).await
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
