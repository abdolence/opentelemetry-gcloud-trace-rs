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
use crate::errors::{
    GcloudObservabilityError, GcloudObservabilityResult, GcloudObservabilitySystemError,
};
use gcloud_sdk::tonic::transport::Uri;
use gcloud_sdk::GoogleAuthHeaders;
use opentelemetry::KeyValue;
use opentelemetry_otlp::tonic_types::transport::ClientTlsConfig;
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use rsb_derive::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

const TELEMETRY_API_ENDPOINT: &str = "https://telemetry.googleapis.com";

/// Builds OpenTelemetry exporters and providers that send spans, and under the
/// `logs-otlp` feature log records, through OTLP to the Google Cloud Telemetry API.
#[derive(Debug, Builder)]
#[non_exhaustive]
pub struct GcpOtlpExporterBuilder {
    /// Set as `gcp.project_id` on the resource of every provider built here, over any
    /// value the resource already has.
    pub google_project_id: String,
    /// Merged key by key over the resource of the provider builder, and under
    /// `gcp.project_id`. While unset, the provider builder's resource stands;
    /// `create_provider` and `create_logger_provider` start from the OpenTelemetry SDK's
    /// default resource. A builder passed to `create_provider_from_builder` or
    /// `create_logger_provider_from_builder` without a resource of its own does not get that
    /// default: its resource is this one and `gcp.project_id`, so set `service.name` here or
    /// on the builder.
    pub resource: Option<Resource>,
    /// Defaults to `https://telemetry.googleapis.com`. Regional endpoints are
    /// `https://telemetry.REGION.rep.googleapis.com`. TLS is verified against the host
    /// of the endpoint.
    pub endpoint: Option<String>,
    /// The longest a batch export may take before the batch is dropped. It is the timeout of
    /// every export request, and for spans also the batch processor's bound. Shutdown exports
    /// every batch queued ahead of it, one after the other, so it can take this long once per
    /// 512 queued spans or log records, rounded up. While unset, a request times out after the
    /// OTLP exporter's 10 seconds, or `OTEL_EXPORTER_OTLP_TIMEOUT` when set, and the span
    /// processor after 30 seconds, or `OTEL_BSP_EXPORT_TIMEOUT`.
    pub max_export_timeout: Option<Duration>,
    /// The credentials exports are authenticated with. Defaults to the Application Default
    /// Credentials with the `cloud-platform` scope. Read by the first exporter or
    /// [`Self::auth_interceptor`] call; every later one shares those credentials.
    pub auth_headers: Option<Arc<GoogleAuthHeaders>>,
    #[default = "OnceCell::new()"]
    shared_auth_interceptor: OnceCell<GcpAuthInterceptor>,
}

impl GcpOtlpExporterBuilder {
    pub async fn for_default_project_id() -> GcloudObservabilityResult<Self> {
        Ok(Self::new(common::detect_google_project_id().await?))
    }

    /// The interceptor every exporter built here authenticates with, created over
    /// [`Self::auth_headers`] on the first call and cloned after it. All of them share one
    /// set of credentials and one refresh task, which ends once this builder and every
    /// exporter and interceptor built from it are dropped.
    pub async fn auth_interceptor(&self) -> GcloudObservabilityResult<GcpAuthInterceptor> {
        self.shared_auth_interceptor
            .get_or_try_init(|| async {
                match &self.auth_headers {
                    Some(auth_headers) => GcpAuthInterceptor::new(Arc::clone(auth_headers)).await,
                    None => GcpAuthInterceptor::from_adc().await,
                }
            })
            .await
            .cloned()
    }

    async fn configure_exporter<B>(&self, exporter: B) -> GcloudObservabilityResult<B>
    where
        B: WithExportConfig + WithTonicConfig,
    {
        let endpoint = self.endpoint.as_deref().unwrap_or(TELEMETRY_API_ENDPOINT);
        let host = endpoint
            .parse::<Uri>()
            .ok()
            .and_then(|uri| uri.host().map(str::to_string))
            .ok_or_else(|| {
                GcloudObservabilityError::SystemError(GcloudObservabilitySystemError::new(format!(
                    "The OTLP endpoint `{endpoint}` is not a URI with a host"
                )))
            })?;
        let exporter = exporter
            .with_endpoint(endpoint)
            .with_tls_config(
                ClientTlsConfig::new()
                    .with_enabled_roots()
                    .domain_name(host),
            )
            .with_interceptor(self.auth_interceptor().await?);
        Ok(match self.max_export_timeout {
            Some(max_export_timeout) => exporter.with_timeout(max_export_timeout),
            None => exporter,
        })
    }

    // In the order they are merged into a provider, so that `gcp.project_id` always names
    // `google_project_id`.
    fn provider_resources(&self) -> impl Iterator<Item = Resource> {
        let project = Resource::builder_empty()
            .with_attribute(KeyValue::new(
                "gcp.project_id",
                self.google_project_id.clone(),
            ))
            .build();
        self.resource.clone().into_iter().chain([project])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otlp::testing::ScriptedAccessToken;

    #[tokio::test]
    async fn exporters_of_one_builder_share_one_refresh_task() {
        let auth_headers = ScriptedAccessToken::new("first-token").auth_headers();
        let builder = GcpOtlpExporterBuilder::new("orders-project".to_string())
            .with_auth_headers(Arc::clone(&auth_headers));

        let _spans = builder.auth_interceptor().await.unwrap();
        let _logs = builder.auth_interceptor().await.unwrap();

        // Held here, by the builder and by the one refresh task.
        assert_eq!(Arc::strong_count(&auth_headers), 3);
    }
}
