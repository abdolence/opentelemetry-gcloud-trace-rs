use crate::TraceExportResult;
use gcloud_sdk::google::devtools::cloudtrace::v2::{
    attribute_value as gcp_attribute_value, span as gspan, AttributeValue as GcpAttributeValue,
    BatchWriteSpansRequest, Span as GcpSpan, TruncatableString,
};
use gcloud_sdk::*;
use opentelemetry::sdk::export::trace::SpanData;
use std::ops::Deref;

pub struct GcpCloudTraceExporter {
    client: GoogleApi<
        google::devtools::cloudtrace::v2::trace_service_client::TraceServiceClient<
            GoogleAuthMiddleware,
        >,
    >,
    google_project_id: String,
}

impl GcpCloudTraceExporter {
    pub async fn new(google_project_id: &String) -> TraceExportResult<Self> {
        let client: GoogleApi<
            google::devtools::cloudtrace::v2::trace_service_client::TraceServiceClient<
                GoogleAuthMiddleware,
            >,
        > = GoogleApi::from_function(
            google::devtools::cloudtrace::v2::trace_service_client::TraceServiceClient::new,
            "https://cloudtrace.googleapis.com",
            None,
        )
        .await?;

        Ok(Self {
            client,
            google_project_id: google_project_id.clone(),
        })
    }

    async fn export_batch(&self, batch: Vec<SpanData>) -> TraceExportResult<()> {
        let mut client = self.client.get();

        let batch_request = BatchWriteSpansRequest {
            name: format!("projects/{}", self.google_project_id),
            spans: batch
                .into_iter()
                .map(|span| GcpSpan {
                    name: format!(
                        "projects/{}/traces/{}/spans/{}",
                        self.google_project_id,
                        span.span_context.trace_id(),
                        span.span_context.span_id()
                    ),
                    span_id: span.span_context.span_id().to_string(),
                    parent_span_id: span.parent_span_id.to_string(),
                    display_name: Some(Self::truncatable_string(span.name.deref(), 128)),
                    start_time: Some(prost_types::Timestamp::from(span.start_time)),
                    end_time: Some(prost_types::Timestamp::from(span.end_time)),
                    attributes: Some(Self::convert_span_attrs(&span.attributes)),
                    time_events: Some(Self::convert_time_events(&span.events)),
                    links: Some(Self::convert_links(&span.links)),
                    ..GcpSpan::default()
                })
                .collect(),
            ..BatchWriteSpansRequest::default()
        };

        client
            .batch_write_spans(tonic::Request::new(batch_request))
            .await?;

        Ok(())
    }

    fn truncatable_string(str: &str, max_len: usize) -> TruncatableString {
        if str.len() > max_len {
            let mut truncated_str = str.to_string();
            truncated_str.truncate(max_len);

            TruncatableString {
                value: truncated_str,
                truncated_byte_count: (str.len() - max_len) as i32,
            }
        } else {
            TruncatableString {
                value: str.to_string(),
                truncated_byte_count: 0,
            }
        }
    }

    fn convert_span_attrs(attrs: &opentelemetry::sdk::trace::EvictedHashMap) -> gspan::Attributes {
        const MAX_ATTRS: usize = 32;
        gspan::Attributes {
            attribute_map: attrs
                .iter()
                .take(MAX_ATTRS)
                .map(|(attribute_key, attribute_value)| {
                    (
                        attribute_key.to_string(),
                        Self::convert_span_attr_value(attribute_value),
                    )
                })
                .collect(),
            dropped_attributes_count: if attrs.len() > MAX_ATTRS {
                (attrs.dropped_count() as usize + attrs.len() - MAX_ATTRS) as i32
            } else {
                attrs.dropped_count() as i32
            },
        }
    }

    fn convert_span_attr_value(attr_value: &opentelemetry::Value) -> GcpAttributeValue {
        const MAX_STR_LEN: usize = 256;
        GcpAttributeValue {
            value: Some(match attr_value {
                opentelemetry::Value::I64(value) => gcp_attribute_value::Value::IntValue(*value),
                opentelemetry::Value::F64(value) => gcp_attribute_value::Value::StringValue(
                    Self::truncatable_string(format!("{:.2}", value).as_str(), MAX_STR_LEN),
                ),
                opentelemetry::Value::String(value) => gcp_attribute_value::Value::StringValue(
                    Self::truncatable_string(value, MAX_STR_LEN),
                ),
                opentelemetry::Value::Bool(value) => gcp_attribute_value::Value::BoolValue(*value),
                opentelemetry::Value::Array(value) => {
                    // Arrays aren't supported yet
                    gcp_attribute_value::Value::StringValue(Self::truncatable_string(
                        "array[...]",
                        MAX_STR_LEN,
                    ))
                }
            }),
        }
    }

    fn convert_time_events(
        events: &opentelemetry::sdk::trace::EvictedQueue<opentelemetry::trace::Event>,
    ) -> gspan::TimeEvents {
        const MAX_EVENTS: usize = 128;

        gspan::TimeEvents {
            time_event: events
                .iter()
                .take(MAX_EVENTS)
                .map(|event| Self::convert_time_event(event))
                .collect(),
            dropped_annotations_count: if events.len() > MAX_EVENTS {
                (events.dropped_count() as usize + events.len() - MAX_EVENTS) as i32
            } else {
                events.dropped_count() as i32
            },
            ..gspan::TimeEvents::default()
        }
    }

    fn convert_time_event(event: &opentelemetry::trace::Event) -> gspan::TimeEvent {
        gspan::TimeEvent {
            time: Some(prost_types::Timestamp::from(event.timestamp)),
            value: Some(Self::convert_time_event_value(event)),
            ..gspan::TimeEvent::default()
        }
    }

    fn convert_time_event_value(
        event_value: &opentelemetry::trace::Event,
    ) -> gspan::time_event::Value {
        const MAX_ATTRS: usize = 32;
        gspan::time_event::Value::Annotation(gspan::time_event::Annotation {
            description: Some(Self::truncatable_string(event_value.name.deref(), 256)),
            attributes: Some(gspan::Attributes {
                attribute_map: event_value
                    .attributes
                    .iter()
                    .take(MAX_ATTRS)
                    .map(|kv| (kv.key.to_string(), Self::convert_span_attr_value(&kv.value)))
                    .collect(),
                dropped_attributes_count: if event_value.attributes.len() > MAX_ATTRS {
                    (event_value.dropped_attributes_count as usize + event_value.attributes.len()
                        - MAX_ATTRS) as i32
                } else {
                    event_value.dropped_attributes_count as i32
                },
            }),
        })
    }

    fn convert_links(
        links: &opentelemetry::sdk::trace::EvictedQueue<opentelemetry::trace::Link>,
    ) -> gspan::Links {
    }
}
