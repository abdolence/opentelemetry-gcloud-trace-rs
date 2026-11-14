//! Validated identifiers the builders take: the Google Cloud project and, for the Cloud Logging
//! API and OTLP log sinks, the log the entries go to.
//!
//! Each is checked once, where the value enters, so that a malformed one fails at startup with
//! [`GcloudObservabilityError::InvalidParametersError`]. Used unchecked, it would surface only as
//! failed exports in a background task, or on the JSON sink not at all, as entries that never
//! link to their trace.

use crate::errors::{GcloudObservabilityError, GcloudObservabilityResult};
use serde::{Deserialize, Serialize, Serializer};
use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

const PROJECT_ID_RULE: &str = "must be 6 to 30 lowercase letters, digits and hyphens, starting with a letter and not ending with a hyphen, optionally after a domain and a colon";

/// A Google Cloud project ID, such as `orders-prod`.
///
/// It is 6 to 30 lowercase letters, digits and hyphens, starts with a letter and does not end
/// with a hyphen. A legacy domain-scoped ID puts its domain and a colon in front of that, as in
/// `example.com:orders-prod`. A project number is not a project ID and is refused.
///
/// ```
/// use gcloud_observability::ids::GoogleProjectId;
///
/// const ORDERS: GoogleProjectId = GoogleProjectId::from_static("orders-prod");
///
/// let configured: GoogleProjectId = "orders-prod".parse()?;
/// assert_eq!(configured, ORDERS);
/// assert!("Orders_Prod".parse::<GoogleProjectId>().is_err());
/// # Ok::<(), gcloud_observability::errors::GcloudObservabilityError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct GoogleProjectId(Cow<'static, str>);

impl GoogleProjectId {
    /// # Errors
    ///
    /// [`GcloudObservabilityError::InvalidParametersError`] for the field `google_project_id`
    /// when `id` breaks the rules in the [type docs](Self).
    pub fn new(id: impl Into<String>) -> GcloudObservabilityResult<Self> {
        let id = id.into();
        if is_project_id(&id) {
            Ok(Self(Cow::Owned(id)))
        } else {
            Err(GcloudObservabilityError::invalid_parameters(
                "google_project_id",
                format!("{PROJECT_ID_RULE}, got {id:?}"),
            ))
        }
    }

    /// Checks `id` at compile time when used in a `const`, where an invalid one fails the
    /// build:
    ///
    /// ```compile_fail
    /// use gcloud_observability::ids::GoogleProjectId;
    ///
    /// const ORDERS: GoogleProjectId = GoogleProjectId::from_static("Orders_Prod");
    /// ```
    ///
    /// # Panics
    ///
    /// When called at run time with an `id` that [`Self::new`] refuses.
    pub const fn from_static(id: &'static str) -> Self {
        if !is_project_id(id) {
            panic!("{}", PROJECT_ID_RULE);
        }
        Self(Cow::Borrowed(id))
    }

    pub(crate) async fn detect() -> GcloudObservabilityResult<Self> {
        let detected = gcloud_sdk::GoogleEnvironment::detect_google_project_id()
            .await
            .ok_or_else(|| {
                GcloudObservabilityError::system(
                    "No Google Project ID detected. Please specify it explicitly using env variable: PROJECT_ID or define it as default project for your service accounts",
                )
            })?;
        Self::new(detected)
    }
}

impl Display for GoogleProjectId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&*self.0, f)
    }
}

impl FromStr for GoogleProjectId {
    type Err = GcloudObservabilityError;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl TryFrom<String> for GoogleProjectId {
    type Error = GcloudObservabilityError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(id)
    }
}

impl Serialize for GoogleProjectId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
const LOG_ID_RULE: &str = "must be 1 to 511 letters, digits and any of / _ - .";

/// The ID of the Cloud Logging log entries are written to, such as `orders` or
/// `orders/checkout`: 1 to 511 letters, digits, `/`, `_`, `-` and `.`.
///
/// ```
/// use gcloud_observability::ids::LogId;
///
/// const CHECKOUT: LogId = LogId::from_static("orders/checkout");
///
/// let configured: LogId = "orders/checkout".parse()?;
/// assert_eq!(configured, CHECKOUT);
/// assert!("orders checkout".parse::<LogId>().is_err());
/// # Ok::<(), gcloud_observability::errors::GcloudObservabilityError>(())
/// ```
#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct LogId(Cow<'static, str>);

#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
impl LogId {
    /// # Errors
    ///
    /// [`GcloudObservabilityError::InvalidParametersError`] for the field `log_id` when `id`
    /// breaks the rules in the [type docs](Self).
    pub fn new(id: impl Into<String>) -> GcloudObservabilityResult<Self> {
        let id = id.into();
        if is_log_id(&id) {
            Ok(Self(Cow::Owned(id)))
        } else {
            Err(GcloudObservabilityError::invalid_parameters(
                "log_id",
                format!("{LOG_ID_RULE}, got {id:?}"),
            ))
        }
    }

    /// Checks `id` at compile time when used in a `const`, where an invalid one fails the
    /// build.
    ///
    /// # Panics
    ///
    /// When called at run time with an `id` that [`Self::new`] refuses.
    pub const fn from_static(id: &'static str) -> Self {
        if !is_log_id(id) {
            panic!("{}", LOG_ID_RULE);
        }
        Self(Cow::Borrowed(id))
    }
}

#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
impl Display for LogId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&*self.0, f)
    }
}

#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
impl FromStr for LogId {
    type Err = GcloudObservabilityError;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
impl TryFrom<String> for LogId {
    type Error = GcloudObservabilityError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(id)
    }
}

#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
impl Serialize for LogId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

// Google's rules for a project ID, from "Before you begin" at
// https://docs.cloud.google.com/resource-manager/docs/creating-managing-projects. The
// domain-scoped form is described at
// https://docs.cloud.google.com/artifact-registry/docs/docker/names#domain, which gives no rule
// for the domain itself, so it is held to the lowercase characters of a DNS name.
const fn is_project_id(id: &str) -> bool {
    const DOMAIN_CHARACTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-.";
    let id = id.as_bytes();
    match position_of(b':', id) {
        Some(colon) => {
            let (domain, _) = id.split_at(colon);
            let (_, name) = id.split_at(colon + 1);
            !domain.is_empty()
                && consists_of(domain, DOMAIN_CHARACTERS)
                && is_unscoped_project_id(name)
        }
        None => is_unscoped_project_id(id),
    }
}

const fn is_unscoped_project_id(id: &[u8]) -> bool {
    const PROJECT_ID_CHARACTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-";
    let [first, .., last] = id else {
        return false;
    };
    id.len() >= 6
        && id.len() <= 30
        && first.is_ascii_lowercase()
        && *last != b'-'
        && consists_of(id, PROJECT_ID_CHARACTERS)
}

// Cloud Logging's rule for `[LOG_ID]` in `logName`, which "must be less than 512 characters
// long": https://docs.cloud.google.com/logging/docs/reference/v2/rest/v2/LogEntry
#[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
const fn is_log_id(id: &str) -> bool {
    const LOG_ID_CHARACTERS: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789/_-.";
    !id.is_empty() && id.len() < 512 && consists_of(id.as_bytes(), LOG_ID_CHARACTERS)
}

// The slice methods that would do the next two are not `const`, and `from_static` checks at
// compile time.

const fn consists_of(bytes: &[u8], allowed: &[u8]) -> bool {
    let mut rest = bytes;
    while let [byte, tail @ ..] = rest {
        if position_of(*byte, allowed).is_none() {
            return false;
        }
        rest = tail;
    }
    true
}

const fn position_of(byte: u8, bytes: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == byte {
            return Some(index);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused_field<T: std::fmt::Debug>(result: GcloudObservabilityResult<T>) -> String {
        match result {
            Err(GcloudObservabilityError::InvalidParametersError(error)) => error.field,
            other => panic!("expected an InvalidParametersError, got {other:?}"),
        }
    }

    #[test]
    fn project_id_follows_googles_naming_rules() {
        let thirty = "a".repeat(30);
        for accepted in [
            "orders",
            thirty.as_str(),
            "orders-prod-2",
            "example.com:orders-prod",
        ] {
            assert!(
                GoogleProjectId::new(accepted).is_ok(),
                "{accepted:?} is a project ID"
            );
        }

        let thirty_one = "a".repeat(31);
        for refused in [
            "",
            "order",
            thirty_one.as_str(),
            "2orders",
            "Orders-prod",
            "orders_prod",
            "orders-prod-",
            "123456789012",
            ":orders-prod",
            "example.com:order",
            "Example.com:orders-prod",
        ] {
            assert_eq!(
                refused_field(GoogleProjectId::new(refused)),
                "google_project_id",
                "{refused:?} is not a project ID"
            );
        }
    }

    #[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
    #[test]
    fn log_id_follows_cloud_loggings_naming_rules() {
        let longest = "a".repeat(511);
        for accepted in ["o", "Orders/check-out_v1.2", longest.as_str()] {
            assert!(LogId::new(accepted).is_ok(), "{accepted:?} is a log ID");
        }

        let too_long = "a".repeat(512);
        for refused in [
            "",
            too_long.as_str(),
            "orders log",
            "orders:log",
            "orders%2Flog",
        ] {
            assert_eq!(
                refused_field(LogId::new(refused)),
                "log_id",
                "{refused:?} is not a log ID"
            );
        }
    }

    #[test]
    fn project_id_is_checked_on_every_way_in() {
        let orders = GoogleProjectId::from_static("orders-prod");

        assert_eq!("orders-prod".parse::<GoogleProjectId>().unwrap(), orders);
        assert_eq!(
            GoogleProjectId::try_from("orders-prod".to_string()).unwrap(),
            orders
        );
        assert!("Orders".parse::<GoogleProjectId>().is_err());
        assert!(GoogleProjectId::try_from("Orders".to_string()).is_err());
        assert!(serde_json::from_str::<GoogleProjectId>(r#""Orders""#).is_err());

        let json = serde_json::to_string(&orders).unwrap();
        assert_eq!(json, r#""orders-prod""#);
        assert_eq!(
            serde_json::from_str::<GoogleProjectId>(&json).unwrap(),
            orders
        );
    }

    #[cfg(any(feature = "logs-api", feature = "logs-otlp"))]
    #[test]
    fn log_id_is_checked_on_every_way_in() {
        let checkout = LogId::from_static("orders/checkout");

        assert_eq!("orders/checkout".parse::<LogId>().unwrap(), checkout);
        assert_eq!(
            LogId::try_from("orders/checkout".to_string()).unwrap(),
            checkout
        );
        assert!("orders checkout".parse::<LogId>().is_err());
        assert!(LogId::try_from("orders checkout".to_string()).is_err());
        assert!(serde_json::from_str::<LogId>(r#""orders checkout""#).is_err());

        let json = serde_json::to_string(&checkout).unwrap();
        assert_eq!(json, r#""orders/checkout""#);
        assert_eq!(serde_json::from_str::<LogId>(&json).unwrap(), checkout);
    }
}
