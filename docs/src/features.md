# Feature flags

| Feature               | Default | Provides                                                                                                         |
|-----------------------|---------|------------------------------------------------------------------------------------------------------------------|
| `logs`                | yes     | The logging layer with the JSON lines sink, and [Error Reporting](./errors/error-reporting.md) for every sink    |
| `logs-api`            | no      | The [Cloud Logging API sink](./logging/cloud-logging-api.md); includes `logs`                                    |
| `logs-otlp`           | no      | The [OTLP sink](./logging/otlp.md); includes `logs` and `otlp`                                                   |
| `otlp`                | no      | The [OTLP exporter](./traces/otlp.md) for the Telemetry API, and `GcpAuthInterceptor` for [any OTLP exporter](./metrics.md) |
| `trace-api`           | no      | The [Cloud Trace API exporter](./traces/cloud-trace-api.md)                                                      |
| `tls-roots`           | yes     | Native TLS root certificates, see [TLS and crypto features](./reference/tls.md)                                  |
| `tls-webpki-roots`    | no      | TLS root certificates of the `webpki-roots` crate                                                                |
| `auth-default-crypto` | yes     | The rustls crypto provider that service account keys are signed with                                             |

The modules follow the features: `logs`, `otlp` and `trace_api`.
`ids` holds `LogId`, which the `logs-api` and `logs-otlp` sinks take, and `errors` holds the error types.
Nothing is re-exported at the crate root.

Choose the features by goal:

- logs to stdout for an agent: the defaults;
- logs through the Cloud Logging API: `logs-api`;
- logs and spans through the Telemetry API: `logs-otlp`;
- spans through the Telemetry API: `otlp`;
- spans through the Cloud Trace API: `trace-api`, see [Choosing an exporter](./traces/choosing.md).

With `default-features = false`, enable a TLS roots feature and, for service account keys, `auth-default-crypto`:

```toml
[dependencies]
gcloud-observability = { version = "0.31", default-features = false, features = ["logs", "otlp", "tls-webpki-roots", "auth-default-crypto"] }
```
