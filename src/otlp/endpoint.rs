use crate::errors::{GcloudObservabilityError, GcloudObservabilityResult};
use gcloud_sdk::tonic::codegen::http::uri::{Scheme, Uri};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

/// The `https` URI the OTLP exporters send to, such as the Telemetry API's global
/// `https://telemetry.googleapis.com` or a regional `https://telemetry.REGION.rep.googleapis.com`
/// (<https://docs.cloud.google.com/stackdriver/docs/reference/telemetry/overview>).
///
/// Plain `http` is refused, because every request carries the credentials' access token.
///
/// ```
/// use gcloud_observability::otlp::TelemetryEndpoint;
///
/// let regional: TelemetryEndpoint = "https://telemetry.europe-west1.rep.googleapis.com".parse()?;
/// assert!("http://telemetry.googleapis.com".parse::<TelemetryEndpoint>().is_err());
/// # Ok::<(), gcloud_observability::errors::GcloudObservabilityError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct TelemetryEndpoint(String);

impl TelemetryEndpoint {
    /// # Errors
    ///
    /// [`GcloudObservabilityError::InvalidParametersError`] for the field `endpoint` when
    /// `endpoint` is not a URI, or is not `https`, or has no host.
    pub fn new(endpoint: impl Into<String>) -> GcloudObservabilityResult<Self> {
        let endpoint = endpoint.into();
        let uri = endpoint.parse::<Uri>().map_err(|error| {
            GcloudObservabilityError::invalid_parameters(
                "endpoint",
                format!("is not a URI ({error}), got {endpoint:?}"),
            )
        })?;
        if uri.scheme() != Some(&Scheme::HTTPS) || uri.host().is_none_or(str::is_empty) {
            return Err(GcloudObservabilityError::invalid_parameters(
                "endpoint",
                format!("must be an https URI with a host, got {endpoint:?}"),
            ));
        }
        Ok(Self(endpoint))
    }
}

impl Display for TelemetryEndpoint {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl FromStr for TelemetryEndpoint {
    type Err = GcloudObservabilityError;

    fn from_str(endpoint: &str) -> Result<Self, Self::Err> {
        Self::new(endpoint)
    }
}

impl TryFrom<String> for TelemetryEndpoint {
    type Error = GcloudObservabilityError;

    fn try_from(endpoint: String) -> Result<Self, Self::Error> {
        Self::new(endpoint)
    }
}

impl Serialize for TelemetryEndpoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_an_https_uri_with_a_host() {
        for accepted in [
            "https://telemetry.googleapis.com",
            "https://telemetry.europe-west1.rep.googleapis.com",
            "https://collector.internal:4317",
        ] {
            assert!(
                TelemetryEndpoint::new(accepted).is_ok(),
                "{accepted:?} is an endpoint"
            );
        }

        for refused in [
            "",
            "not a uri",
            "telemetry.googleapis.com",
            "http://telemetry.googleapis.com",
            "https://",
            "https://:4317",
        ] {
            assert!(
                matches!(
                    TelemetryEndpoint::new(refused),
                    Err(GcloudObservabilityError::InvalidParametersError(ref error))
                        if error.field == "endpoint"
                ),
                "{refused:?} is not an endpoint"
            );
        }
    }

    #[test]
    fn endpoint_is_checked_on_every_way_in() {
        let global = TelemetryEndpoint::new("https://telemetry.googleapis.com").unwrap();

        assert_eq!(
            "https://telemetry.googleapis.com"
                .parse::<TelemetryEndpoint>()
                .unwrap(),
            global
        );
        assert!("http://telemetry.googleapis.com"
            .parse::<TelemetryEndpoint>()
            .is_err());
        assert!(
            TelemetryEndpoint::try_from("http://telemetry.googleapis.com".to_string()).is_err()
        );
        assert!(
            serde_json::from_str::<TelemetryEndpoint>(r#""http://telemetry.googleapis.com""#)
                .is_err()
        );

        let json = serde_json::to_string(&global).unwrap();
        assert_eq!(json, r#""https://telemetry.googleapis.com""#);
        assert_eq!(
            serde_json::from_str::<TelemetryEndpoint>(&json).unwrap(),
            global
        );
    }
}
