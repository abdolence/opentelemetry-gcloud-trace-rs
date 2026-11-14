//! This crate is deprecated and not intended to be used.
//!
//! Renamed to [gcloud-observability](https://crates.io/crates/gcloud-observability), which is no
//! longer only about traces: it also covers Cloud Logging and Error Reporting. This crate
//! re-exports it, so a version bump alone builds, but the 0.31 changes (features, module paths)
//! still apply.
//!
//! - Repository: <https://github.com/abdolence/gcloud-observability-rs>
//! - Migration guide:
//!   <https://github.com/abdolence/gcloud-observability-rs/blob/master/MIGRATION.md>

pub use gcloud_observability::*;
