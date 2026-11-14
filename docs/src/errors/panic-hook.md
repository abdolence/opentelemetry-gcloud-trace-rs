# Panic hook

`install_panic_hook()` emits one `tracing` error event for every panic, then calls the hook that was installed before it, so the default message on stderr stays.
Call it once, early in `main`, after the subscriber is set:

```rust,ignore
{{#include ../../../examples/errors/panic_hook.rs:layer}}
```

The event carries the panic message and `panic.location`, and `context.reportLocation` names that location rather than the hook.
It reaches [Error Reporting](./error-reporting.md) when the layer was built with `with_error_reporting`.
The [backtrace](./backtraces.md) is captured inside the hook and needs `RUST_BACKTRACE` or `RUST_LIB_BACKTRACE`.

## Flushing

The JSON lines sink writes the event before the hook returns.
The [Cloud Logging API](../logging/cloud-logging-api.md) and [OTLP](../logging/otlp.md) sinks only queue it and send it in the background, so it is lost when the process ends first: after `panic = "abort"`, or when a panic unwinds out of `main` and nothing flushes on the way.

Shut down the `GcpCloudLoggingHandle` or the logger provider before the process ends.
The example catches the panic with `std::panic::catch_unwind` and shuts the handle down afterwards.
A guard that shuts the logger provider down when it is dropped covers a panic that unwinds out of `main`.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/errors/panic_hook.rs).
