use gcloud_sdk::error::Error;
use opentelemetry_sdk::ExportError;
use rsb_derive::*;

pub type BoxedError = Box<dyn std::error::Error + Send + Sync>;

pub type GcloudObservabilityResult<E> = Result<E, GcloudObservabilityError>;

#[derive(Debug)]
pub enum GcloudObservabilityError {
    SystemError(GcloudObservabilitySystemError),
    NetworkError(GcloudObservabilityNetworkError),
    /// A value refused before it was used, such as a malformed project ID.
    InvalidParametersError(GcloudObservabilityInvalidParametersError),
}

impl GcloudObservabilityError {
    pub(crate) fn system(message: impl Into<String>) -> Self {
        Self::SystemError(GcloudObservabilitySystemError::new(message.into()))
    }

    pub(crate) fn system_caused_by(
        message: impl Into<String>,
        root_cause: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::SystemError(
            GcloudObservabilitySystemError::new(message.into())
                .with_root_cause(Box::new(root_cause)),
        )
    }

    pub(crate) fn invalid_parameters(field: impl Into<String>, error: impl Into<String>) -> Self {
        Self::InvalidParametersError(GcloudObservabilityInvalidParametersError::new(
            field.into(),
            error.into(),
        ))
    }
}

impl std::fmt::Display for GcloudObservabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            GcloudObservabilityError::SystemError(ref err) => err.fmt(f),
            GcloudObservabilityError::NetworkError(ref err) => err.fmt(f),
            GcloudObservabilityError::InvalidParametersError(ref err) => err.fmt(f),
        }
    }
}

impl std::error::Error for GcloudObservabilityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            GcloudObservabilityError::SystemError(ref err) => err.source(),
            GcloudObservabilityError::NetworkError(ref err) => err.source(),
            GcloudObservabilityError::InvalidParametersError(ref err) => err.source(),
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

#[derive(Debug, Eq, PartialEq, Clone, Builder)]
pub struct GcloudObservabilityInvalidParametersError {
    /// The parameter, named as the builders name it: `google_project_id`, `log_id` or
    /// `endpoint`.
    pub field: String,
    /// Why it was refused.
    pub error: String,
}

impl std::fmt::Display for GcloudObservabilityInvalidParametersError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Invalid parameters error: {} {}", self.field, self.error)
    }
}

impl std::error::Error for GcloudObservabilityInvalidParametersError {}

impl From<gcloud_sdk::error::Error> for GcloudObservabilityError {
    fn from(gcloud_error: Error) -> Self {
        GcloudObservabilityError::system_caused_by("Google SDK error", gcloud_error)
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
        GcloudObservabilityError::system_caused_by("OTLP exporter error", error)
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
