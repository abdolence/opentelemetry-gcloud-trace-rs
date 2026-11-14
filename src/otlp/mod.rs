//! Exports through OTLP to the Google Cloud Telemetry API
//! (`telemetry.googleapis.com`), authenticated with Google credentials.

mod auth;
#[cfg(feature = "logs-otlp")]
mod logs;
#[cfg(test)]
mod testing;
mod trace;

pub use auth::GcpAuthInterceptor;

use crate::common;
use crate::errors::{GcloudTraceError, GcloudTraceSystemError, TraceExportResult};
use gcloud_sdk::tonic::transport::Uri;
use gcloud_sdk::GoogleAuthHeaders;
use opentelemetry::KeyValue;
use opentelemetry_otlp::tonic_types::transport::ClientTlsConfig;
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use rsb_derive::*;
use std::sync::Arc;
use std::time::Duration;

const TELEMETRY_API_ENDPOINT: &str = "https://telemetry.googleapis.com";

/// Builds OpenTelemetry exporters and providers that send spans, and under the
/// `logs-otlp` feature log records, through OTLP to the Google Cloud Telemetry API.
#[derive(Debug, Builder)]
#[non_exhaustive]
pub struct GcpOtlpExporterBuilder {
    /// Set as `gcp.project_id` on the resource of every provider built here, over any
    /// value the resource already has.
    pub google_project_id: String,
    /// The resource of every provider built here, with `gcp.project_id` merged over it.
    /// Defaults to the OpenTelemetry SDK's default resource. A resource already set on a
    /// provider builder passed in is merged under this one, key by key.
    pub resource: Option<Resource>,
    /// Defaults to `https://telemetry.googleapis.com`. Regional endpoints are
    /// `https://telemetry.REGION.rep.googleapis.com`. TLS is verified against the host
    /// of the endpoint.
    pub endpoint: Option<String>,
    /// The longest a batch export may take before the batch is dropped. Shutdown exports every
    /// batch queued ahead of it, one after the other, so it can take this long once per 512
    /// queued spans or log records, rounded up. Defaults to the OpenTelemetry SDK default of
    /// 30 seconds, or `OTEL_BSP_EXPORT_TIMEOUT` for spans and `OTEL_BLRP_EXPORT_TIMEOUT` for
    /// log records when set.
    pub max_export_timeout: Option<Duration>,
    /// The credentials exports are authenticated with. Defaults to the Application Default
    /// Credentials with the `cloud-platform` scope.
    pub auth_headers: Option<Arc<GoogleAuthHeaders>>,
}

impl GcpOtlpExporterBuilder {
    pub async fn for_default_project_id() -> TraceExportResult<Self> {
        Ok(Self::new(common::detect_google_project_id().await?))
    }

    /// An interceptor over [`Self::auth_headers`], the one every exporter built here
    /// authenticates with. Each call starts its own refresh task.
    pub async fn auth_interceptor(&self) -> TraceExportResult<GcpAuthInterceptor> {
        match &self.auth_headers {
            Some(auth_headers) => GcpAuthInterceptor::new(Arc::clone(auth_headers)).await,
            None => GcpAuthInterceptor::from_adc().await,
        }
    }

    async fn configure_exporter<B>(&self, exporter: B) -> TraceExportResult<B>
    where
        B: WithExportConfig + WithTonicConfig,
    {
        let endpoint = self.endpoint.as_deref().unwrap_or(TELEMETRY_API_ENDPOINT);
        let host = endpoint
            .parse::<Uri>()
            .ok()
            .and_then(|uri| uri.host().map(str::to_string))
            .ok_or_else(|| {
                GcloudTraceError::SystemError(GcloudTraceSystemError::new(format!(
                    "The OTLP endpoint `{endpoint}` is not a URI with a host"
                )))
            })?;
        Ok(exporter
            .with_endpoint(endpoint)
            .with_tls_config(
                ClientTlsConfig::new()
                    .with_enabled_roots()
                    .domain_name(host),
            )
            .with_interceptor(self.auth_interceptor().await?))
    }

    // In the order they are merged into a provider, so that `gcp.project_id` always names
    // `google_project_id`.
    fn provider_resources(&self) -> [Resource; 2] {
        [
            self.resource
                .clone()
                .unwrap_or_else(|| Resource::builder().build()),
            Resource::builder_empty()
                .with_attribute(KeyValue::new(
                    "gcp.project_id",
                    self.google_project_id.clone(),
                ))
                .build(),
        ]
    }
}
