//! Exports through OTLP to the Google Cloud Telemetry API
//! (`telemetry.googleapis.com`), authenticated with Google credentials.

mod auth;
#[cfg(test)]
mod testing;

pub use auth::GcpAuthInterceptor;
