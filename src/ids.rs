//! The validated identifier the Cloud Logging API and OTLP log sinks take: the log the entries go
//! to.
//!
//! It is checked once, where the value enters, so that a malformed one fails at startup with
//! [`GcloudObservabilityError::InvalidParametersError`]. Used unchecked, it would surface only
//! as failed exports in a background task.

use crate::errors::{GcloudObservabilityError, GcloudObservabilityResult};
use serde::{Deserialize, Serialize, Serializer};
use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct LogId(Cow<'static, str>);

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

impl Display for LogId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&*self.0, f)
    }
}

impl FromStr for LogId {
    type Err = GcloudObservabilityError;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl TryFrom<String> for LogId {
    type Error = GcloudObservabilityError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(id)
    }
}

impl Serialize for LogId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

// Cloud Logging's rule for `[LOG_ID]` in `logName`, which "must be less than 512 characters
// long": https://docs.cloud.google.com/logging/docs/reference/v2/rest/v2/LogEntry
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
