use opentelemetry::trace::*;
use opentelemetry::KeyValue;

use opentelemetry_gcloud_trace::*;

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tracer = GcpCloudTraceExporterBuilder::for_default_project_id()
        .await? // or GcpCloudTraceExporterBuilder::new(config_env_var("PROJECT_ID")?)
        .install()
        .await?;

    tracer.in_span("my_parent_work", |cx| {
        let span = cx.span();
        span.set_attribute(KeyValue::new("http.client_ip", "42.42.42.42"));
        span.set_attribute(KeyValue::new(
            "my_test_arr",
            opentelemetry::Value::Array(vec![42i64, 42i64].into()),
        ));
        span.add_event(
            "test-event",
            vec![KeyValue::new("test_event_attr", "test-event-value")],
        );
        tracer.in_span("my_child_work", |cx| {
            println!(
                "Do printing, nothing more here. Please check your Google Cloud Trace dashboard."
            );

            cx.span().add_event(
                "test-child-event",
                vec![KeyValue::new("test_event_attr", "test-event-value")],
            );
        })
    });

    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}
