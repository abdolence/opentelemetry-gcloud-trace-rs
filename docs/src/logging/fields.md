# Field conventions

Event fields map onto Cloud Logging's structured `LogEntry` fields as follows.
Anything else is written verbatim into the JSON payload.

| Event field                                                                                   | Log entry field                                          | Format                                                                                                                                                                                  |
|-----------------------------------------------------------------------------------------------|----------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `severity`                                                                                    | `severity`                                               | One of `DEFAULT DEBUG INFO NOTICE WARNING ERROR CRITICAL ALERT EMERGENCY`, matched case-insensitively. Overrides the level mapping: `TRACE` and `DEBUG` are `DEBUG`, `INFO` is `INFO`, `WARN` is `WARNING`, `ERROR` is `ERROR`; the level maps when the field is absent or not a name |
| `http_request.request_method`, `.request_url`, `.user_agent`, `.remote_ip`, `.server_ip`, `.referer`, `.protocol` | `httpRequest.*`, camel-cased                   | strings                                                                                                                                                                                 |
| `http_request.status`                                                                         | `httpRequest.status`                                     | integer                                                                                                                                                                                 |
| `http_request.request_size`, `.response_size`, `.cache_fill_bytes`                            | `httpRequest.*`                                          | integers                                                                                                                                                                                |
| `http_request.latency`                                                                        | `httpRequest.latency`                                    | a duration string such as `"250ms"` or `"1.5s"`, or a number of seconds                                                                                                                 |
| `http_request.cache_lookup`, `.cache_hit`, `.cache_validated_with_origin_server`              | `httpRequest.*`                                          | booleans                                                                                                                                                                                |
| `labels.<name>`                                                                               | `logging.googleapis.com/labels.<name>`                   | value stringified                                                                                                                                                                       |
| `operation.id`, `.producer`                                                                   | `logging.googleapis.com/operation.{id,producer}`         | strings                                                                                                                                                                                 |
| `operation.first`, `.last`                                                                    | `logging.googleapis.com/operation.{first,last}`          | booleans                                                                                                                                                                                |
| `insert_id`                                                                                   | `logging.googleapis.com/insertId`                        | string                                                                                                                                                                                  |
| source location                                                                               | `logging.googleapis.com/sourceLocation`                  | file and line; on by default, disable with `with_source_location(false)`                                                                                                                |

The field names are not valid Rust identifiers, so they are written as string literals in the macros:

```rust,ignore
info!(
    "labels.tenant" = "acme",
    "http_request.request_method" = "GET",
    "http_request.request_url" = "https://example.test/orders",
    "http_request.status" = 200,
    "Serving a request."
);
```

Only the `httpRequest.*` keys are camel-cased, matching Google's own field names; every other field name is written as given.

On the [Cloud Logging API](./cloud-logging-api.md) and [OTLP](./otlp.md) sinks, a value that does not fit the typed field it is destined for, such as a malformed `latency`, stays in the payload instead of being dropped.

## Errors

An event field passed as `&dyn Error` keeps its whole `source()` chain, one `Caused by:` line per source:

```rust,ignore
tracing::error!(error = &failure as &dyn std::error::Error, "checkout failed");
```

`error = %err` and `error = ?err` record only the text they format.
See [Error Reporting](../errors/error-reporting.md) for how an error event is grouped.
