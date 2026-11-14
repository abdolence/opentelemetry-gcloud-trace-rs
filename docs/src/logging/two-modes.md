# One application, two modes

A service usually wants a console layer locally and the JSON layer once deployed, chosen by one `match` rather than duplicated `main` functions.
`Option<Layer>` implements `Layer`, so exactly one sink is active per mode, without a boxed trait object or two separate subscriber types:

```rust,ignore
{{#include ../../../examples/logging/json.rs:layers}}
```

The inactive layer is a `None` that costs nothing.
Trace correlation keeps working with the logging layer wrapped in `Option`, with no extra step.

The example reads the mode from `APP_MODE`, default `development`:

```bash
PROJECT_ID=<your-google-project-id> APP_MODE=production cargo run --example logging-json --features logs,otlp
```

Run one sink in production. Writing every event to the terminal and to a network sink, as the development mode of the `traces-and-logs-cloud-trace-api` example does to show the correlation from a laptop, duplicates entries on a host that collects stdout.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/logging/json.rs).
