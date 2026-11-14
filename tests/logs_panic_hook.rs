#![cfg(feature = "logs")]

mod common;

use common::{SharedBuffer, TEST_PROJECT};
use gcloud_observability::logs::{install_panic_hook, GcpCloudLoggingLayerBuilder, ServiceContext};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;

static PREVIOUS_HOOK_CALLS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn panic_emits_one_error_event_and_calls_the_previous_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {
        PREVIOUS_HOOK_CALLS.fetch_add(1, Ordering::SeqCst);
    }));
    install_panic_hook();

    let buffer = SharedBuffer::default();
    let layer = GcpCloudLoggingLayerBuilder::new(TEST_PROJECT)
        .with_json_writer(buffer.clone())
        .with_error_reporting(ServiceContext::new("orders"))
        .build()
        .unwrap();
    let panic_line = line!() + 3;
    let result = tracing::subscriber::with_default(Registry::default().with(layer), || {
        std::panic::catch_unwind(|| {
            panic!("inventory went negative");
        })
    });
    let _ = std::panic::take_hook();
    std::panic::set_hook(original_hook);

    assert!(result.is_err());
    assert_eq!(PREVIOUS_HOOK_CALLS.load(Ordering::SeqCst), 1);
    let lines = buffer.lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["severity"], "ERROR");
    assert!(lines[0]["message"]
        .as_str()
        .is_some_and(|message| message.contains("inventory went negative")));
    let location = lines[0]["panic.location"]
        .as_str()
        .expect("the panic location is a field");
    assert!(location.contains("logs_panic_hook.rs") && location.contains(&panic_line.to_string()));
    let report_location = &lines[0]["context"]["reportLocation"];
    assert!(report_location["filePath"]
        .as_str()
        .is_some_and(|path| path.ends_with("logs_panic_hook.rs")));
    assert_eq!(report_location["lineNumber"], panic_line);
}
