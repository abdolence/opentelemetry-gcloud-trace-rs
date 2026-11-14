//! Writes each event's log record as one JSON line to a `MakeWriter`.

use serde_json::{Map, Value};
use std::io::Write;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::fmt::MakeWriter;

pub(crate) struct JsonSink {
    make_writer: BoxMakeWriter,
}

impl JsonSink {
    pub(crate) fn new<W>(make_writer: W) -> Self
    where
        W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
    {
        Self {
            make_writer: BoxMakeWriter::new(make_writer),
        }
    }

    /// Serialises `record` plus a trailing newline into one buffer and issues
    /// a single `write_all`, so concurrent events cannot interleave partial
    /// lines on a shared writer. I/O errors are ignored: a logging sink must
    /// not turn a full disk or a broken pipe into a panic or a lost event
    /// elsewhere in the application.
    pub(crate) fn write(&self, record: &Map<String, Value>, metadata: &tracing::Metadata<'_>) {
        let Ok(mut buffer) = serde_json::to_vec(record) else {
            return;
        };
        buffer.push(b'\n');
        let mut writer = self.make_writer.make_writer_for(metadata);
        let _ = writer.write_all(&buffer);
    }
}
