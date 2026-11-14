# Error Reporting

Cloud Error Reporting groups the log entries it recognises as errors, counts them and shows them in one list.
The logging layer writes that format on every [sink](../logging/sinks.md) when it is built with a service context:

```rust,ignore
{{#include ../../../examples/errors/error_reporting.rs:layer}}
```

`ServiceContext::new(service)` names the service errors are grouped under, such as the Cloud Run service or the binary name, and `with_version` sets the deployed version Error Reporting shows beside the error.

Every event at ERROR severity or above becomes an error event, including an event with a `severity` field of `critical`, `alert` or `emergency`. Other events are untouched.
The entry gets:

- `@type`, the `ReportedErrorEvent` type, which makes Error Reporting evaluate the entry;
- `serviceContext`, from the `ServiceContext`;
- `context.reportLocation`, from the file and line of the event, with the module path as the function name;
- `context.httpRequest`, mapped from the `http_request.*` [fields](../logging/fields.md) when the event has them;
- a `message` made of the event message, the error, and a [backtrace](./backtraces.md).

Without `with_error_reporting` an error event is written like any other.

## The error in the message

Pass the error to the event as a `&dyn Error`, so that its `source()` chain is kept:

```rust,ignore
tracing::error!(error = &failure as &dyn std::error::Error, order_id, "checkout failed");
```

The first line of `message` is the event message followed by the `Display` of the error and of each source, outermost first: `checkout failed: payment declined: card expired`.
`error = %err` and `error = ?err` record only the text they format, so the chain stops at the outer error.

The chain stays on that first line, and no `Caused by:` lines follow it: Error Reporting keys a group on the cause such a line names, so unrelated errors that share a root cause would become one group.

## Grouping

Error Reporting groups errors with the same first line and error text, and it does so wherever they were logged: two call sites that log the same header and error are one group.
A different text is a different group.
Keep text that varies between occurrences, such as an order ID, in a field instead of the message.

`context.reportLocation` is always set, so grouping works without a backtrace.
Error Reporting groups by the top frames of a stack trace when it has one.

## OTLP sink

On the [OTLP sink](../logging/otlp.md) the body carries the same keys, so they reach `jsonPayload`, and the severity number is 17 or above.
The sink also sets the `exception.type` and `exception.message` attributes, which Google maps to labels.
A `&dyn Error` has no type name on stable Rust, so `exception.type` is set from an `exception.type` event field when the event has one.

Entries sent over OTLP are grouped by Error Reporting like the others.

Full example available [here](https://github.com/abdolence/gcloud-observability-rs/blob/master/examples/errors/error_reporting.rs).

See Google's [Formatting error messages](https://docs.cloud.google.com/error-reporting/docs/formatting-error-messages) for the format.
