use gcloud_sdk::error::Error;
use opentelemetry_sdk::ExportError;
use rsb_derive::*;

pub type BoxedError = Box<dyn std::error::Error + Send + Sync>;

pub type GcloudObservabilityResult<E> = Result<E, GcloudObservabilityError>;

#[derive(Debug)]
pub enum GcloudObservabilityError {
    SystemError(GcloudObservabilitySystemError),
    NetworkError(GcloudObservabilityNetworkError),
}

impl std::fmt::Display for GcloudObservabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            GcloudObservabilityError::SystemError(ref err) => err.fmt(f),
            GcloudObservabilityError::NetworkError(ref err) => err.fmt(f),
        }
    }
}

impl std::error::Error for GcloudObservabilityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            GcloudObservabilityError::SystemError(ref err) => err.source(),
            GcloudObservabilityError::NetworkError(ref err) => err.source(),
        }
    }
}

#[derive(Debug, Builder)]
pub struct GcloudObservabilitySystemError {
    pub message: String,
    pub root_cause: Option<BoxedError>,
}

impl std::fmt::Display for GcloudObservabilitySystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "System error: {:?}", self.message)
    }
}

impl std::error::Error for GcloudObservabilitySystemError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.root_cause
            .as_ref()
            .map(|cause| &**cause as &(dyn std::error::Error + 'static))
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Builder)]
pub struct GcloudObservabilityNetworkError {
    pub message: String,
}

impl std::fmt::Display for GcloudObservabilityNetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Network error: {}", self.message)
    }
}

impl std::error::Error for GcloudObservabilityNetworkError {}

impl From<gcloud_sdk::error::Error> for GcloudObservabilityError {
    fn from(gcloud_error: Error) -> Self {
        GcloudObservabilityError::SystemError(
            GcloudObservabilitySystemError::new("Google SDK error".to_string())
                .with_root_cause(Box::new(gcloud_error)),
        )
    }
}

impl From<gcloud_sdk::tonic::Status> for GcloudObservabilityError {
    fn from(status: gcloud_sdk::tonic::Status) -> Self {
        GcloudObservabilityError::NetworkError(GcloudObservabilityNetworkError::new(format!(
            "{status}"
        )))
    }
}

#[cfg(feature = "otlp")]
impl From<opentelemetry_otlp::ExporterBuildError> for GcloudObservabilityError {
    fn from(error: opentelemetry_otlp::ExporterBuildError) -> Self {
        GcloudObservabilityError::SystemError(
            GcloudObservabilitySystemError::new("OTLP exporter error".to_string())
                .with_root_cause(Box::new(error)),
        )
    }
}

impl ExportError for GcloudObservabilityError {
    fn exporter_name(&self) -> &'static str {
        "GoogleCloudTraceExporter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcloud_sdk::error::ErrorKind;
    use std::error::Error as _;

    #[test]
    fn source_of_a_system_error_is_its_root_cause() {
        let error = GcloudObservabilityError::from(Error::from(ErrorKind::CryptoProviderMissing));

        let source = error.source().expect("the root cause is the source");

        assert!(source.is::<Error>());
        assert!(source.source().is_none());
    }

    #[test]
    fn network_error_has_no_source() {
        let error = GcloudObservabilityError::from(gcloud_sdk::tonic::Status::unavailable("down"));

        assert!(error.source().is_none());
    }

    #[test]
    fn system_error_without_a_root_cause_has_no_source() {
        let error = GcloudObservabilityError::SystemError(GcloudObservabilitySystemError::new(
            "no project".to_string(),
        ));

        assert!(error.source().is_none());
    }
}
