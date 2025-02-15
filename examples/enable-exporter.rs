use opentelemetry::trace::*;

use opentelemetry_gcloud_trace::*;

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let gcp_trace_exporter = GcpCloudTraceExporterBuilder::for_default_project_id()
        .await?
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    "simple-app",
                )])
                .build(),
        ); // or GcpCloudTraceExporterBuilder::new(config_env_var("PROJECT_ID")?)
    let tracer_provider = gcp_trace_exporter.create_provider().await?;
    let tracer: opentelemetry_sdk::trace::Tracer =
        gcp_trace_exporter.install(&tracer_provider).await?;

    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    tracer.in_span("my_parent_work", |cx| {
        let span = cx.span();
        span.set_attribute(opentelemetry::KeyValue::new(
            "http.client_ip",
            "42.42.42.42",
        ));
        span.set_attribute(opentelemetry::KeyValue::new(
            "my_test_arr",
            opentelemetry::Value::Array(vec![42i64, 42i64].into()),
        ));
        span.add_event(
            "test-event",
            vec![opentelemetry::KeyValue::new(
                "test_event_attr",
                "test-event-value",
            )],
        );
        tracer.in_span("my_child_work", |cx| {
            println!(
                "Do printing, nothing more here. Please check your Google Cloud Trace dashboard."
            );

            cx.span().add_event(
                "test-child-event",
                vec![opentelemetry::KeyValue::new(
                    "test_event_attr",
                    "test-event-value",
                )],
            );
        })
    });

    tracer_provider.shutdown()?;

    Ok(())
}
