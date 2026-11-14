# One application, two modes

A service usually wants a console layer locally and the JSON layer once deployed, chosen by one `match` rather than duplicated `main` functions.
`APP_MODE` (default `development`) picks the log sink:

- `development`: human-readable console lines;
- `production`: Cloud Logging JSON lines on stdout, which a logging agent on GKE, Cloud Run or GCE collects.

Spans go to the Telemetry API through OTLP in both modes, and the logging layer reads the span context they attach, so every entry carries its `trace` and `spanId`.

```rust,ignore
{{#include ../../../examples/logging/dev_and_prod.rs:app-mode}}
```

`Option<Layer>` implements `Layer`, so each sink is built in the `match` and composed unconditionally, without a boxed trait object or two separate subscriber types.
The inactive one is a `None` that costs nothing:

```rust,ignore
{{#include ../../../examples/logging/dev_and_prod.rs:layers}}
```

```bash
PROJECT_ID=<your-google-project-id> cargo run --example logging-dev-and-prod --features logs,otlp
PROJECT_ID=<your-google-project-id> APP_MODE=production cargo run --example logging-dev-and-prod --features logs,otlp
```

The production mode writes to stdout only. Run one sink in production: a service that also sends entries over the network duplicates them on a host that collects stdout.

The [traces and logs](../traces/traces-and-logs.md) page has the same switch with the development mode sending its entries over OTLP as well.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/logging/dev_and_prod.rs).
