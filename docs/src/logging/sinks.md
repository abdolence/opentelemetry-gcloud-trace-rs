# Sinks

A layer writes its entries to one sink, chosen when the layer is built:

| Sink                | Feature     | Use it when                                                                          | Build method                      |
|---------------------|-------------|--------------------------------------------------------------------------------------|-----------------------------------|
| [JSON lines](./json-lines.md)                   | `logs` (default) | the host runs the Cloud Logging agent over stdout: GKE, Cloud Run, GCE with the Ops Agent | `build()`       |
| [Cloud Logging API](./cloud-logging-api.md)     | `logs-api`       | the host has no logging agent: a laptop, a VM without the Ops Agent, a batch job            | `build_async().await` |
| [OTLP](./otlp.md)                               | `logs-otlp`      | logs go to the Telemetry API beside the spans, with one set of credentials                  | `build()`       |

The JSON lines sink costs no queue, no background task and no network traffic of its own.
Where an agent is present it is the cheaper and simpler choice.

Every sink builds its entry from the same event record, so the [field conventions](./fields.md), [Error Reporting](../errors/error-reporting.md) and trace correlation behave the same on each.

The two network sinks, the Cloud Logging API and OTLP, skip events whose target is in `h2`, `hyper`, `hyper_util`, `tonic`, `tower`, `rustls`, `gcloud_sdk`, `opentelemetry`, `opentelemetry_sdk` or `opentelemetry_otlp`.
At DEBUG or TRACE every export makes events in those crates, and an entry made of one is another export to make.
The JSON lines sink writes them.
