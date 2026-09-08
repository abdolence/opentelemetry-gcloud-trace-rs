//! Bounded queue and background batching task behind the Cloud Logging API
//! sink.
//!
//! Recording an event must never block the thread that logged it and must
//! never log through `tracing` (which would re-enter this layer), so the queue
//! is bounded and lossy at the producer end and every diagnostic goes straight
//! to stderr, rate-limited to one line per flush interval.

use super::{entry, GcpCloudLoggingApiConfig};
use gcloud_sdk::google::api::MonitoredResource;
use gcloud_sdk::google::logging::v2::{LogEntry, WriteLogEntriesRequest};
use serde_json::{Map, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// One write of a collected batch. Implemented by the Cloud Logging client in
/// production and by a recording fake in tests, so the batching rules can be
/// exercised without a network or credentials.
#[async_trait::async_trait]
pub(crate) trait LogEntrySink: Send + Sync + 'static {
    async fn write(&self, request: WriteLogEntriesRequest) -> Result<(), String>;
}

/// Boxed so the queue's per-message footprint stays a pointer: a `LogEntry` is
/// several hundred bytes and the queue holds thousands of them.
enum BatchMessage {
    Entry(Box<LogEntry>),
    Flush(oneshot::Sender<()>),
    Shutdown(oneshot::Sender<()>),
}

pub(crate) struct ApiSink {
    sender: mpsc::Sender<BatchMessage>,
    dropped: Arc<AtomicU64>,
}

impl ApiSink {
    /// Queues one record without blocking. An event recorded while the queue
    /// is full is counted and discarded: stalling the caller until Cloud
    /// Logging catches up would turn a logging outage into an application one.
    pub(crate) fn send(&self, record: Map<String, Value>) {
        let entry = entry::build_log_entry(record);
        if self
            .sender
            .try_send(BatchMessage::Entry(Box::new(entry)))
            .is_err()
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Flushes and stops the background task that writes log entries to the Cloud
/// Logging API.
///
/// Cloning shares one task and one queue. Dropping every clone without calling
/// [`Self::shutdown`] leaves whatever is still queued unwritten.
#[derive(Clone)]
pub struct GcpCloudLoggingHandle {
    sender: mpsc::Sender<BatchMessage>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    shutdown_timeout: Duration,
}

impl GcpCloudLoggingHandle {
    /// Writes everything queued before this call and returns once the Cloud
    /// Logging API has answered, whether it accepted the batch or not.
    ///
    /// The marker travels through the same queue as the entries, so it cannot
    /// overtake them however full the queue is. Entries queued after this call
    /// are not waited for.
    pub async fn flush(&self) {
        let (ack_sender, ack) = oneshot::channel();
        if self
            .sender
            .send(BatchMessage::Flush(ack_sender))
            .await
            .is_ok()
        {
            let _ = ack.await;
        }
    }

    /// Stops accepting entries, writes what is queued, and waits for the
    /// background task to finish, giving up after the configured shutdown
    /// timeout. Entries recorded after this call are dropped.
    ///
    /// Calling it twice is harmless; the second call returns at once.
    pub async fn shutdown(&self) {
        let deadline = Instant::now() + self.shutdown_timeout;
        let (ack_sender, ack) = oneshot::channel();
        let queued = tokio::time::timeout_at(
            deadline,
            self.sender.send(BatchMessage::Shutdown(ack_sender)),
        )
        .await;
        if matches!(queued, Ok(Ok(()))) {
            let _ = tokio::time::timeout_at(deadline, ack).await;
        }
        let task = self.task.lock().await.take();
        if let Some(task) = task {
            // On timeout the join handle is dropped rather than aborted: the
            // task may still be mid-write, and detaching it gives those
            // entries a chance to land.
            let _ = tokio::time::timeout_at(deadline, task).await;
        }
    }
}

/// Starts the batching task on the current Tokio runtime.
pub(crate) fn spawn(
    config: &GcpCloudLoggingApiConfig,
    log_name: String,
    resource: MonitoredResource,
    sink: Arc<dyn LogEntrySink>,
) -> (ApiSink, GcpCloudLoggingHandle) {
    let (sender, receiver) = mpsc::channel(config.queue_capacity.max(1));
    let dropped = Arc::new(AtomicU64::new(0));
    let batcher = Batcher {
        sink,
        log_name,
        resource,
        batch_size: config.batch_size.max(1),
        flush_interval: config.flush_interval,
        dropped: Arc::clone(&dropped),
        reported_drops: 0,
        last_drop_report: None,
        last_failure_report: None,
        batch: Vec::with_capacity(config.batch_size.max(1)),
    };
    let task = tokio::spawn(batcher.run(receiver));
    (
        ApiSink {
            sender: sender.clone(),
            dropped,
        },
        GcpCloudLoggingHandle {
            sender,
            task: Arc::new(Mutex::new(Some(task))),
            shutdown_timeout: config.shutdown_timeout,
        },
    )
}

struct Batcher {
    sink: Arc<dyn LogEntrySink>,
    log_name: String,
    resource: MonitoredResource,
    batch_size: usize,
    flush_interval: Duration,
    dropped: Arc<AtomicU64>,
    reported_drops: u64,
    last_drop_report: Option<Instant>,
    last_failure_report: Option<Instant>,
    batch: Vec<LogEntry>,
}

impl Batcher {
    async fn run(mut self, mut receiver: mpsc::Receiver<BatchMessage>) {
        // `None` while no batch is open: an idle task must wait for an entry
        // indefinitely rather than wake once per flush interval forever.
        let mut deadline: Option<Instant> = None;

        loop {
            let message = match deadline {
                Some(at) => match tokio::time::timeout_at(at, receiver.recv()).await {
                    Ok(message) => message,
                    Err(_) => {
                        self.write_batch().await;
                        deadline = None;
                        continue;
                    }
                },
                None => receiver.recv().await,
            };

            match message {
                None => break,
                Some(BatchMessage::Entry(logentry)) => {
                    if self.batch.is_empty() {
                        deadline = Some(Instant::now() + self.flush_interval);
                    }
                    self.batch.push(*logentry);
                    if self.batch.len() >= self.batch_size {
                        self.write_batch().await;
                        deadline = None;
                    }
                }
                Some(BatchMessage::Flush(ack)) => {
                    self.write_batch().await;
                    deadline = None;
                    let _ = ack.send(());
                }
                Some(BatchMessage::Shutdown(ack)) => {
                    self.drain_and_stop(&mut receiver, ack).await;
                    return;
                }
            }
        }

        self.write_batch().await;
    }

    /// Closes the queue and writes whatever was already in it, so a shutdown
    /// observes every entry recorded before it was requested.
    async fn drain_and_stop(
        &mut self,
        receiver: &mut mpsc::Receiver<BatchMessage>,
        ack: oneshot::Sender<()>,
    ) {
        receiver.close();
        let mut pending_acks = vec![ack];
        while let Ok(message) = receiver.try_recv() {
            match message {
                BatchMessage::Entry(logentry) => {
                    self.batch.push(*logentry);
                    if self.batch.len() >= self.batch_size {
                        self.write_batch().await;
                    }
                }
                BatchMessage::Flush(ack) | BatchMessage::Shutdown(ack) => pending_acks.push(ack),
            }
        }
        self.write_batch().await;
        for ack in pending_acks {
            let _ = ack.send(());
        }
    }

    async fn write_batch(&mut self) {
        self.report_drops();
        if self.batch.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.batch);
        let count = entries.len();
        let request = WriteLogEntriesRequest {
            log_name: self.log_name.clone(),
            resource: Some(self.resource.clone()),
            entries,
            partial_success: true,
            ..WriteLogEntriesRequest::default()
        };
        // The client retries on its own; once it gives up there is nothing
        // useful left to do with the batch but drop it, and a stderr line is
        // the only report that cannot re-enter this layer.
        if let Err(message) = self.sink.write(request).await {
            if report_due(&mut self.last_failure_report, self.flush_interval) {
                eprintln!(
                    "opentelemetry-gcloud-trace: discarded a batch of {count} log entries: {message}"
                );
            }
        }
    }

    fn report_drops(&mut self) {
        let dropped = self.dropped.load(Ordering::Relaxed);
        let unreported = dropped.saturating_sub(self.reported_drops);
        if unreported > 0 && report_due(&mut self.last_drop_report, self.flush_interval) {
            eprintln!(
                "opentelemetry-gcloud-trace: dropped {unreported} log entries, \
                 the Cloud Logging queue is full"
            );
            self.reported_drops = dropped;
        }
    }
}

/// Keeps stderr diagnostics to one line per flush interval, so a sustained
/// outage cannot itself become the flood.
fn report_due(last: &mut Option<Instant>, interval: Duration) -> bool {
    let now = Instant::now();
    if last.is_some_and(|at| now.duration_since(at) < interval) {
        return false;
    }
    *last = Some(now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcloud_sdk::google::api::MonitoredResource;
    use serde_json::json;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct RecordingSink {
        batches: StdMutex<Vec<WriteLogEntriesRequest>>,
        fail: bool,
        delay: Option<Duration>,
    }

    impl RecordingSink {
        fn shared() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn batch_sizes(&self) -> Vec<usize> {
            self.batches
                .lock()
                .expect("test sink mutex is never poisoned")
                .iter()
                .map(|request| request.entries.len())
                .collect()
        }

        fn requests(&self) -> Vec<WriteLogEntriesRequest> {
            self.batches
                .lock()
                .expect("test sink mutex is never poisoned")
                .clone()
        }
    }

    #[async_trait::async_trait]
    impl LogEntrySink for RecordingSink {
        async fn write(&self, request: WriteLogEntriesRequest) -> Result<(), String> {
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            self.batches
                .lock()
                .expect("test sink mutex is never poisoned")
                .push(request);
            if self.fail {
                Err("write refused".to_string())
            } else {
                Ok(())
            }
        }
    }

    fn config() -> GcpCloudLoggingApiConfig {
        GcpCloudLoggingApiConfig::new("test-log")
            .with_batch_size(3)
            .with_flush_interval(Duration::from_secs(2))
            .with_queue_capacity(64)
            .with_shutdown_timeout(Duration::from_secs(5))
    }

    fn resource() -> MonitoredResource {
        MonitoredResource {
            r#type: "global".to_string(),
            labels: [("project_id".to_string(), "p".to_string())]
                .into_iter()
                .collect(),
        }
    }

    fn start(
        config: &GcpCloudLoggingApiConfig,
        sink: Arc<RecordingSink>,
    ) -> (ApiSink, GcpCloudLoggingHandle) {
        spawn(
            config,
            "projects/p/logs/test-log".to_string(),
            resource(),
            sink,
        )
    }

    fn record(message: &str) -> Map<String, Value> {
        match json!({ "message": message, "severity": "INFO" }) {
            Value::Object(map) => map,
            other => panic!("test record must be an object, got {other}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_batch_is_written_without_waiting_for_the_interval() {
        let sink = RecordingSink::shared();
        let config = config().with_flush_interval(Duration::from_secs(3600));
        let (api_sink, handle) = start(&config, Arc::clone(&sink));

        for index in 0..7 {
            api_sink.send(record(&format!("event {index}")));
        }
        handle.flush().await;

        assert_eq!(sink.batch_sizes(), vec![3, 3, 1]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_partial_batch_is_written_after_the_flush_interval() {
        let sink = RecordingSink::shared();
        let config = config();
        let (api_sink, _handle) = start(&config, Arc::clone(&sink));

        api_sink.send(record("first"));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(sink.batch_sizes(), Vec::<usize>::new());

        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(sink.batch_sizes(), vec![1]);
    }

    #[tokio::test(start_paused = true)]
    async fn entries_over_the_queue_capacity_are_counted_as_dropped() {
        let sink = Arc::new(RecordingSink {
            delay: Some(Duration::from_secs(3600)),
            ..RecordingSink::default()
        });
        let config = config().with_queue_capacity(4).with_batch_size(1);
        let (api_sink, _handle) = start(&config, Arc::clone(&sink));

        for index in 0..40 {
            api_sink.send(record(&format!("event {index}")));
        }

        assert!(
            api_sink.dropped.load(Ordering::Relaxed) > 0,
            "a queue of 4 cannot hold 40 entries while the sink is blocked"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn flush_writes_everything_queued_before_it() {
        let sink = RecordingSink::shared();
        let config = config().with_flush_interval(Duration::from_secs(3600));
        let (api_sink, handle) = start(&config, Arc::clone(&sink));

        api_sink.send(record("before flush"));
        handle.flush().await;

        assert_eq!(sink.batch_sizes(), vec![1]);
        let request = sink.requests().remove(0);
        assert_eq!(request.log_name, "projects/p/logs/test-log");
        assert_eq!(request.resource, Some(resource()));
        assert!(request.partial_success);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_writes_what_is_queued() {
        let sink = RecordingSink::shared();
        let config = config().with_flush_interval(Duration::from_secs(3600));
        let (api_sink, handle) = start(&config, Arc::clone(&sink));

        api_sink.send(record("before shutdown"));
        handle.shutdown().await;

        assert_eq!(sink.batch_sizes(), vec![1]);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_gives_up_after_the_shutdown_timeout() {
        let sink = Arc::new(RecordingSink {
            delay: Some(Duration::from_secs(3600)),
            ..RecordingSink::default()
        });
        let config = config()
            .with_flush_interval(Duration::from_secs(3600))
            .with_shutdown_timeout(Duration::from_secs(1));
        let (api_sink, handle) = start(&config, Arc::clone(&sink));

        api_sink.send(record("never written"));
        let started = tokio::time::Instant::now();
        handle.shutdown().await;

        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_secs(1) && waited < Duration::from_secs(60),
            "shutdown waited {waited:?}, expected roughly the 1s timeout"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_batch_is_discarded_and_the_task_keeps_running() {
        let sink = Arc::new(RecordingSink {
            fail: true,
            ..RecordingSink::default()
        });
        let config = config().with_flush_interval(Duration::from_secs(3600));
        let (api_sink, handle) = start(&config, Arc::clone(&sink));

        api_sink.send(record("first"));
        handle.flush().await;
        api_sink.send(record("second"));
        handle.flush().await;

        assert_eq!(sink.batch_sizes(), vec![1, 1]);
    }
}
