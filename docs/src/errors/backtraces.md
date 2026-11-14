# Backtraces

An error event carries a backtrace when one is captured.
The layer captures it with `std::backtrace::Backtrace::capture()` at the place the event is logged, so a capture costs nothing unless `RUST_BACKTRACE` or `RUST_LIB_BACKTRACE` enables it:

```bash
RUST_BACKTRACE=1 PROJECT_ID=<your-google-project-id> cargo run --example errors-error-reporting --features logs-api
```

Set one of the two variables in the environment of the deployed service to get backtraces in Error Reporting.
The layer never forces a capture, so without them the entry has no backtrace.

The backtrace is rendered with `Display`, as `std::backtrace` frames, and follows the first line of `message`.
Error Reporting has accepted Rust `std::backtrace` frames since 2026-08-13, see its [release notes](https://docs.cloud.google.com/error-reporting/docs/release-notes), and groups errors by their top frames.
The trace is placed in `message` only, never in `stack_trace`, because Error Reporting reads `stack_trace` before `message` and the first present wins, which would hide the first line.

Error Reporting's release note asks for debug symbols to be enabled.
In a release build, `debug = "line-tables-only"` in `[profile.release]` keeps the file and line numbers of the frames.
