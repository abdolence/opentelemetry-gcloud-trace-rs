# Cloud Logging API

The `logs-api` feature adds a sink that writes through the Cloud Logging API (`logging.googleapis.com`), for hosts with no logging agent to collect stdout.
Entries are queued and written in batches by a background task on the Tokio runtime that `build_async` was called on.

```rust,ignore
{{#include ../../../examples/logging/cloud_logging_api.rs:layer}}
```

`GcpCloudLoggingApiConfig::new` takes a `LogId`, the log the entries go to: 1 to 511 letters, digits and `/ _ - .`.
`LogId::from_static` checks a constant at compile time. A `/` is URL-encoded as `%2F` in the `logName`, as Cloud Logging stores it.

| Setting                  | Default                                                      |
|--------------------------|--------------------------------------------------------------|
| `with_resource`          | the `global` monitored resource, labelled with the project ID |
| `with_batch_size`        | 200 entries                                                  |
| `with_flush_interval`    | 2 seconds                                                    |
| `with_queue_capacity`    | 10 000 entries                                               |
| `with_shutdown_timeout`  | 5 seconds                                                    |

A monitored resource is the resource an entry is attributed to, such as `gce_instance` or `k8s_container`. Set one with `with_resource` to have entries grouped with the rest of that resource's logs.

A batch is written when `batch_size` entries are queued, or `flush_interval` after its first entry.
When the queue is full, new entries are dropped instead of blocking the thread that logged them, and a message on stderr reports the drops.

## Shutting down

`build_async` returns the layer and a `GcpCloudLoggingHandle`.
Call `handle.shutdown().await` before the process ends: it closes the queue and waits, bounded by the shutdown timeout, for the final write.
Dropping the handle without it leaves the queued entries unwritten.
`flush` and `shutdown` are async and block no thread, so they work on a current-thread runtime.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/logging/cloud_logging_api.rs).
