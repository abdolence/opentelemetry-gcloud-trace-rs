use crate::google_trace_exporter_client::GcpCloudTraceExporterClient;
use crate::TraceExportResult;
use futures::future::TryFutureExt;
use futures::FutureExt;
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use opentelemetry_sdk::{
    trace::{SpanData, SpanExporter},
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
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let client = self.gcp_export_client.clone();
        async move {
            client
                .export_batch(batch)
                .map_err(|e| OTelSdkError::InternalFailure(e.to_string()))
                .await
        }
        .boxed()
    }
}
