# TLS and crypto features

Cargo provides support for different TLS features for dependencies:

- `tls-roots`: default feature to support native TLS roots;
- `tls-webpki-roots`: feature to switch to the roots of the `webpki-roots` crate;
- `auth-default-crypto`: default feature with the rustls crypto provider that `google-cloud-auth` signs service account keys with.

`gcloud-sdk` needs a TLS root store, so a build with `default-features = false` enables one of the two roots features.
The OTLP exporter follows the same choice: `tls-roots` and `tls-webpki-roots` also enable the matching roots of `opentelemetry-otlp`.

## Crypto provider

The default `auth-default-crypto` feature brings the aws-lc-rs rustls provider.
The OTLP exporter's TLS uses the same provider, so only one is compiled in.
Be aware that aws-lc-rs is built from C sources.

With `default-features = false` and without `auth-default-crypto`, install a rustls `CryptoProvider` before creating an exporter or a logging layer that talks to Google, otherwise it fails with `CryptoProviderMissing` when the credentials are a service account key:

```toml
[dependencies]
rustls = "0.23"
```

```rust,ignore
rustls::crypto::ring::default_provider().install_default().expect("Failed to install rustls crypto provider");
```

An error like

```text
no process-level CryptoProvider available -- call CryptoProvider::install_default() before this point
```

means more than one rustls provider is in the dependencies, and `CryptoProvider::install_default()` has to be called as above before creating the exporter.
