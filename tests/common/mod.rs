//! Test support shared by the integration tests that read the JSON sink's output.

use serde_json::Value;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

pub const TEST_PROJECT: &str = "test-project";

/// A writer for the JSON sink that keeps what the layer wrote. Clones share the buffer.
#[derive(Clone, Default)]
pub struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    pub fn lines(&self) -> Vec<Value> {
        let buffer = self
            .0
            .lock()
            .expect("buffer mutex is never poisoned in these tests");
        String::from_utf8_lossy(&buffer)
            .lines()
            .map(|line| serde_json::from_str(line).expect("sink writes one JSON object per line"))
            .collect()
    }
}

pub struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBufferWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("buffer mutex is never poisoned in these tests")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    type Writer = SharedBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferWriter(self.0.clone())
    }
}
