# Troubleshooting

## Could not resolve host for telemetry.googleapis.com

DNS filters such as dnscrypt-proxy, Pi-hole and corporate blocklists often block every `telemetry.*` name.
The example blocklist of dnscrypt-proxy, which OpenWrt enables, is one of them.
The Telemetry API and its regional endpoints then fail with "could not resolve host".

Allowlist `telemetry.googleapis.com` and `telemetry.*.rep.googleapis.com`.
The upstream report is [openwrt/packages#31291](https://github.com/openwrt/packages/issues/31291).

## No trace or span on the log entries

The logging layer is registered before `tracing_opentelemetry::layer()`, or that layer is missing.
Register the OpenTelemetry layer first, see [The logging layer](../logging/layer.md#layer-ordering).
The layer prints a warning to stderr the first time it finds a span without context.

## CryptoProviderMissing

The credentials are a service account key and no rustls crypto provider is installed.
Enable `auth-default-crypto`, or install a provider, see [TLS and crypto features](./tls.md).

## Entries or spans are missing at the end of a run

Queued entries and spans are exported in the background.
Shut down the tracer provider, the logger provider or the `GcpCloudLoggingHandle` before the process ends.
A provider shutdown on a current-thread Tokio runtime does not export, see [Limitations](./limitations.md).

## InvalidParametersError

A value was refused before it was used. The field names it: `google_project_id` (empty, or containing `/` or a control character), `log_id` (1 to 511 letters, digits and `/ _ - .`) or `endpoint` (an `https` URI with a host).

## Metrics are refused with InvalidArgument

The resource lacks `service.instance.id` or a real `cloud.region`, see [Metrics over OTLP](../metrics.md#resource-attributes).

## Permission denied

Check the API is enabled and the role is granted for the service, see [Authentication](../authentication.md).
