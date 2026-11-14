use crate::errors::{GcloudTraceError, GcloudTraceSystemError, TraceExportResult};
use gcloud_sdk::tonic::metadata::{KeyAndValueRef, MetadataMap};
use gcloud_sdk::tonic::service::Interceptor;
use gcloud_sdk::tonic::{Request, Status};
use gcloud_sdk::{GoogleAuthHeaders, HeaderMap};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::watch;

// Polling is cheap: the credentials answer from their cache until they hold a new token.
const AUTH_HEADERS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// A tonic [`Interceptor`] that sets the headers of Google credentials on every request:
/// `authorization`, and whatever else the credentials add, such as `x-goog-user-project`
/// for a quota project.
///
/// [`super::GcpOtlpExporterBuilder`] authenticates its exporters with one. Pass one to
/// `with_interceptor` to authenticate an OTLP exporter built elsewhere, such as a
/// metrics exporter.
///
/// An interceptor runs synchronously while Google tokens are refreshed asynchronously, so
/// a background task polls the credentials and publishes their headers when they change,
/// and each request copies the latest ones. The task runs on the Tokio runtime the
/// interceptor is created on, and ends when the last clone is dropped. A failed refresh
/// is logged and keeps the headers published last.
#[derive(Clone)]
pub struct GcpAuthInterceptor {
    headers: watch::Receiver<MetadataMap>,
}

impl GcpAuthInterceptor {
    /// Fetches the first headers of `auth_headers` and starts the task that keeps them
    /// current.
    ///
    /// Fails when called outside a Tokio runtime, or when the credentials serve no
    /// headers.
    pub async fn new(auth_headers: impl Into<Arc<GoogleAuthHeaders>>) -> TraceExportResult<Self> {
        let runtime = Handle::try_current().map_err(|error| {
            GcloudTraceError::SystemError(
                GcloudTraceSystemError::new(
                    "GcpAuthInterceptor refreshes its credentials on a Tokio runtime, and none is running".to_string(),
                )
                .with_root_cause(Box::new(error)),
            )
        })?;
        let auth_headers = auth_headers.into();
        let served = auth_headers.headers().await?;
        let (sender, headers) =
            watch::channel(MetadataMap::from_headers(HeaderMap::clone(&served)));
        runtime.spawn(keep_current(auth_headers, served, sender));
        Ok(Self { headers })
    }

    /// [`Self::new`] with the Application Default Credentials and the `cloud-platform`
    /// scope.
    pub async fn from_adc() -> TraceExportResult<Self> {
        Self::new(GoogleAuthHeaders::from_adc().await?).await
    }
}

impl std::fmt::Debug for GcpAuthInterceptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcpAuthInterceptor").finish_non_exhaustive()
    }
}

impl Interceptor for GcpAuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let metadata = request.metadata_mut();
        for entry in self.headers.borrow().iter() {
            match entry {
                KeyAndValueRef::Ascii(key, value) => {
                    metadata.insert(key.clone(), value.clone());
                }
                KeyAndValueRef::Binary(key, value) => {
                    metadata.insert_bin(key.clone(), value.clone());
                }
            }
        }
        Ok(request)
    }
}

async fn keep_current(
    auth_headers: Arc<GoogleAuthHeaders>,
    mut served: Arc<HeaderMap>,
    sender: watch::Sender<MetadataMap>,
) {
    let refresh = async {
        loop {
            tokio::time::sleep(AUTH_HEADERS_REFRESH_INTERVAL).await;
            match auth_headers.headers().await {
                // Unchanged credentials answer with the map they served last.
                Ok(headers) if Arc::ptr_eq(&headers, &served) => {}
                Ok(headers) => {
                    sender.send_replace(MetadataMap::from_headers(HeaderMap::clone(&headers)));
                    served = headers;
                }
                Err(error) => tracing::warn!(
                    %error,
                    "Refreshing the Google credentials of OTLP exports failed; the last headers stay in use"
                ),
            }
        }
    };
    tokio::select! {
        () = sender.closed() => {}
        () = refresh => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otlp::testing::ScriptedAccessToken;

    #[tokio::test]
    async fn requests_carry_the_credentials_authorization_as_a_sensitive_header() {
        let auth_headers = ScriptedAccessToken::new("first-token").auth_headers();
        let mut interceptor = GcpAuthInterceptor::new(auth_headers).await.unwrap();

        let request = interceptor.call(Request::new(())).unwrap();

        let authorization = request
            .metadata()
            .get("authorization")
            .expect("the credentials' authorization header is set");
        assert_eq!(authorization, "Bearer first-token");
        assert!(authorization.is_sensitive());
    }

    #[tokio::test(start_paused = true)]
    async fn requests_carry_the_rotated_token_after_a_refresh() {
        let credentials = ScriptedAccessToken::new("first-token");
        let mut interceptor = GcpAuthInterceptor::new(credentials.auth_headers())
            .await
            .unwrap();

        credentials.rotate("second-token");
        tokio::time::sleep(AUTH_HEADERS_REFRESH_INTERVAL + Duration::from_secs(1)).await;

        let request = interceptor.call(Request::new(())).unwrap();
        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .map(|value| value.to_str().unwrap()),
            Some("Bearer second-token")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_task_ends_with_the_last_interceptor_clone() {
        let auth_headers = ScriptedAccessToken::new("first-token").auth_headers();
        let interceptor = GcpAuthInterceptor::new(Arc::clone(&auth_headers))
            .await
            .unwrap();
        let clone = interceptor.clone();

        drop(interceptor);
        drop(clone);
        tokio::time::sleep(AUTH_HEADERS_REFRESH_INTERVAL * 3).await;

        // The refresh task holds the only other reference to the credentials.
        assert_eq!(Arc::strong_count(&auth_headers), 1);
    }

    #[tokio::test]
    async fn construction_fails_when_the_credentials_serve_no_headers() {
        let auth_headers = ScriptedAccessToken::default().auth_headers();

        let result = GcpAuthInterceptor::new(auth_headers).await;

        assert!(matches!(result, Err(GcloudTraceError::SystemError(_))));
    }
}
