# JSON lines

The default sink writes each entry as one JSON line to stdout.
GKE, Cloud Run and a GCE instance with the Cloud Logging agent parse structured JSON from stdout, so the layer only has to serialise each event: the keys above map onto Cloud Logging's `LogEntry` fields.

```rust,ignore
let log_layer = GcpCloudLoggingLayerBuilder::new(google_project_id).build()?;
```

`with_json_writer` takes any `tracing_subscriber::fmt::MakeWriter` in place of stdout.
Each line is written with a single `write_all`, so concurrent events do not interleave partial lines on a shared writer.
I/O errors are ignored: a full disk or a broken pipe does not turn into a panic.

On a host without the agent the lines only print. Use the [Cloud Logging API](./cloud-logging-api.md) or [OTLP](./otlp.md) sink there.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/logging/json.rs).
