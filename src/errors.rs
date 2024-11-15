use gcloud_sdk::error::Error;
use opentelemetry::trace::ExportError;
use rsb_derive::*;

pub type BoxedError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug)]
pub enum GcloudTraceError {
    SystemError(GcloudTraceSystemError),
    NetworkError(GcloudTraceNetworkError),
}

impl std::fmt::Display for GcloudTraceError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            GcloudTraceError::SystemError(ref err) => err.fmt(f),
            GcloudTraceError::NetworkError(ref err) => err.fmt(f),
        }
    }
}

impl std::error::Error for GcloudTraceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            GcloudTraceError::SystemError(ref err) => Some(err),
            GcloudTraceError::NetworkError(ref err) => Some(err),
        }
    }
}

#[derive(Debug, Builder)]
pub struct GcloudTraceSystemError {
    pub message: String,
    pub root_cause: Option<BoxedError>,
}

impl std::fmt::Display for GcloudTraceSystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "System error: {:?}", self.message)
    }
}

impl std::error::Error for GcloudTraceSystemError {}

#[derive(Debug, Eq, PartialEq, Clone, Builder)]
pub struct GcloudTraceNetworkError {
    pub message: String,
}

impl std::fmt::Display for GcloudTraceNetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Network error: {}", self.message)
    }
}

impl std::error::Error for GcloudTraceNetworkError {}

impl From<gcloud_sdk::error::Error> for GcloudTraceError {
    fn from(gcloud_error: Error) -> Self {
        GcloudTraceError::SystemError(
            GcloudTraceSystemError::new(format!("Google SDK error: {gcloud_error}"))
                .with_root_cause(Box::new(gcloud_error)),
        )
    }
}

impl From<gcloud_sdk::tonic::Status> for GcloudTraceError {
    fn from(status: gcloud_sdk::tonic::Status) -> Self {
        GcloudTraceError::NetworkError(GcloudTraceNetworkError::new(format!("{status}")))
    }
}

impl ExportError for GcloudTraceError {
    fn exporter_name(&self) -> &'static str {
        "GoogleCloudTraceExporter"
    }
}
