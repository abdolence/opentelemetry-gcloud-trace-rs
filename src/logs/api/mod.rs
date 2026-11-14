//! Cloud Logging API sink: configuration, the client that performs
//! `WriteLogEntries`, and the handle that flushes and stops the background
//! task.
//!
//! Use this sink on hosts with no logging agent to collect stdout. Where an
//! agent is present the JSON sink is cheaper and more robust: it costs no
//! queue, no background task and no egress of its own.

mod batcher;
mod entry;

pub use batcher::GcpCloudLoggingHandle;
pub(crate) use batcher::{spawn, ApiSink, LogEntrySink};

/// The monitored resource a log entry is attributed to, re-exported so callers
/// need not depend on `gcloud-sdk` directly.
pub use gcloud_sdk::google::api::MonitoredResource;

use crate::errors::GcloudObservabilityResult;
use crate::ids::LogId;
use gcloud_sdk::google::logging::v2::logging_service_v2_client::LoggingServiceV2Client;
use gcloud_sdk::google::logging::v2::WriteLogEntriesRequest;
use gcloud_sdk::{GoogleApi, GoogleAuthMiddleware};
use std::time::Duration;

const DEFAULT_BATCH_SIZE: usize = 200;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_QUEUE_CAPACITY: usize = 10_000;
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// How the Cloud Logging API sink writes: which log to write to, what resource
/// the entries are attributed to, and how aggressively they are batched.
pub struct GcpCloudLoggingApiConfig {
    pub(crate) log_id: LogId,
    pub(crate) resource: Option<MonitoredResource>,
    pub(crate) batch_size: usize,
    pub(crate) flush_interval: Duration,
    pub(crate) queue_capacity: usize,
    pub(crate) shutdown_timeout: Duration,
}

impl GcpCloudLoggingApiConfig {
    /// Writes to `projects/{project}/logs/{log_id}`, with `/` in `log_id` URL-encoded as
    /// `%2F`, as Cloud Logging stores it.
    pub fn new(log_id: LogId) -> Self {
        Self {
            log_id,
            resource: None,
            batch_size: DEFAULT_BATCH_SIZE,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }

    /// Attributes every entry to this monitored resource. Defaults to type
    /// `global` labelled with the project id, which is accepted everywhere;
    /// set a specific resource (`gce_instance`, `k8s_container`, …) to have
    /// the entries grouped with the rest of that resource's logs.
    pub fn with_resource(mut self, resource: MonitoredResource) -> Self {
        self.resource = Some(resource);
        self
    }

    /// Writes as soon as this many entries are queued. Defaults to 200.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Writes a partial batch this long after its first entry was queued.
    /// Defaults to two seconds.
    pub fn with_flush_interval(mut self, flush_interval: Duration) -> Self {
        self.flush_interval = flush_interval;
        self
    }

    /// Bounds the queue between the layer and the background task. Events
    /// recorded when the queue is full are dropped rather than blocking the
    /// thread that logged them. Defaults to 10 000.
    pub fn with_queue_capacity(mut self, queue_capacity: usize) -> Self {
        self.queue_capacity = queue_capacity;
        self
    }

    /// Bounds how long [`GcpCloudLoggingHandle::shutdown`] waits for the final
    /// write. Defaults to five seconds.
    pub fn with_shutdown_timeout(mut self, shutdown_timeout: Duration) -> Self {
        self.shutdown_timeout = shutdown_timeout;
        self
    }

    /// The `logName` of every entry, in which `[LOG_ID]` must be URL-encoded. `/` is the only
    /// character of a [`LogId`] that is not URL-safe.
    pub(crate) fn log_name(&self, google_project_id: &str) -> String {
        format!(
            "projects/{google_project_id}/logs/{}",
            self.log_id.to_string().replace('/', "%2F")
        )
    }

    pub(crate) fn resource_or_default(&self, google_project_id: &str) -> MonitoredResource {
        self.resource.clone().unwrap_or_else(|| MonitoredResource {
            r#type: "global".to_string(),
            labels: [("project_id".to_string(), google_project_id.to_string())]
                .into_iter()
                .collect(),
        })
    }
}

pub(crate) struct GcloudLogEntrySink {
    client: GoogleApi<LoggingServiceV2Client<GoogleAuthMiddleware>>,
}

impl GcloudLogEntrySink {
    pub(crate) async fn new() -> GcloudObservabilityResult<Self> {
        let client = GoogleApi::from_function(
            LoggingServiceV2Client::new,
            "https://logging.googleapis.com",
            None,
        )
        .await?;
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl LogEntrySink for GcloudLogEntrySink {
    async fn write(&self, request: WriteLogEntriesRequest) -> Result<(), String> {
        self.client
            .get()
            .write_log_entries(gcloud_sdk::tonic::Request::new(request))
            .await
            .map(|_| ())
            .map_err(|status| status.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_of_a_log_id_is_url_encoded_in_the_log_name() {
        let config = GcpCloudLoggingApiConfig::new(LogId::from_static("orders/checkout-v1.2_a"));

        assert_eq!(
            config.log_name("orders-prod"),
            "projects/orders-prod/logs/orders%2Fcheckout-v1.2_a"
        );
    }
}
