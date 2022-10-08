use opentelemetry::trace::*;
use opentelemetry::KeyValue;

use opentelemetry_gcloud_trace::*;

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tracer: opentelemetry::sdk::trace::Tracer =
        GcpCloudTraceExporterBuilder::for_default_project_id()
            .await? // or GcpCloudTraceExporterBuilder::new(config_env_var("PROJECT_ID")?)
            .install_simple()
            .await?;

    tracer.in_span("doing_work_parent", |cx| {
        cx.span()
            .set_attribute(KeyValue::new("http.client_ip", "42.42.42.42"));
        cx.span().add_event(
            "test-event",
            vec![KeyValue::new("test-event-attr", "test-event-value")],
        );

        tracer.in_span("doing_work_child", |cx| {
            println!("Doing printing, nothing more here");

            cx.span().add_event(
                "test-child-event",
                vec![KeyValue::new("test-event-attr", "test-event-value")],
            );
        })
    });

    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}
