use crate::TraceExportResult;
use gcloud_sdk::google::devtools::cloudtrace::v2::{
    attribute_value as gcp_attribute_value, span as gspan, AttributeValue as GcpAttributeValue,
    BatchWriteSpansRequest, Span as GcpSpan, TruncatableString,
};
use gcloud_sdk::google::rpc::{Code as GcpStatusCode, Status as GcpStatus};
use gcloud_sdk::*;
use opentelemetry::KeyValue;
use opentelemetry_sdk::export::trace::SpanData;
use std::ops::Deref;

#[derive(Clone)]
pub struct GcpCloudTraceExporterClient {
    client: GoogleApi<
        google::devtools::cloudtrace::v2::trace_service_client::TraceServiceClient<
            GoogleAuthMiddleware,
        >,
    >,
    google_project_id: String,
}

impl GcpCloudTraceExporterClient {
    pub async fn new(google_project_id: &str) -> TraceExportResult<Self> {
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
            google_project_id: google_project_id.to_string(),
        })
    }

    pub async fn export_batch(&self, batch: Vec<SpanData>) -> TraceExportResult<()> {
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
                    status: Self::convert_status(&span),
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

    fn convert_span_attrs(attrs: &Vec<KeyValue>) -> gspan::Attributes {
        const MAX_ATTRS: usize = 32;
        gspan::Attributes {
            attribute_map: attrs
                .iter()
                .take(MAX_ATTRS)
                .map(|attribute| {
                    (
                        attribute.key.to_string(),
                        Self::convert_span_attr_value(&attribute.value),
                    )
                })
                .collect(),
            dropped_attributes_count: if attrs.len() > MAX_ATTRS {
                (attrs.len() - MAX_ATTRS) as i32
            } else {
                0
            },
        }
    }

    fn convert_span_attr_value(attr_value: &opentelemetry::Value) -> GcpAttributeValue {
        const MAX_STR_LEN: usize = 256;
        GcpAttributeValue {
            value: Some(match attr_value {
                opentelemetry::Value::I64(value) => gcp_attribute_value::Value::IntValue(*value),
                opentelemetry::Value::F64(value) => gcp_attribute_value::Value::StringValue(
                    Self::truncatable_string(format!("{value:.2}").as_str(), MAX_STR_LEN),
                ),
                opentelemetry::Value::String(value) => gcp_attribute_value::Value::StringValue(
                    Self::truncatable_string(value.as_str(), MAX_STR_LEN),
                ),
                opentelemetry::Value::Bool(value) => gcp_attribute_value::Value::BoolValue(*value),
                opentelemetry::Value::Array(_) => {
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
        events: &opentelemetry_sdk::trace::EvictedQueue<opentelemetry::trace::Event>,
    ) -> gspan::TimeEvents {
        const MAX_EVENTS: usize = 128;

        gspan::TimeEvents {
            time_event: events
                .iter()
                .take(MAX_EVENTS)
                .map(Self::convert_time_event)
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
        links: &opentelemetry_sdk::trace::EvictedQueue<opentelemetry::trace::Link>,
    ) -> gspan::Links {
        const MAX_LINKS: usize = 128;

        gspan::Links {
            link: links
                .iter()
                .take(MAX_LINKS)
                .map(Self::convert_link)
                .collect(),
            dropped_links_count: if links.len() > MAX_LINKS {
                (links.dropped_count() as usize + links.len() - MAX_LINKS) as i32
            } else {
                links.dropped_count() as i32
            },
            ..gspan::Links::default()
        }
    }

    fn convert_link(link: &opentelemetry::trace::Link) -> gspan::Link {
        gspan::Link {
            trace_id: link.span_context.trace_id().to_string(),
            span_id: link.span_context.span_id().to_string(),
            ..gspan::Link::default()
        }
    }

    fn convert_status(span: &SpanData) -> Option<GcpStatus> {
        match span.status {
            opentelemetry::trace::Status::Unset => None,
            opentelemetry::trace::Status::Ok => Some(GcpStatus {
                code: GcpStatusCode::Ok.into(),
                ..GcpStatus::default()
            }),
            opentelemetry::trace::Status::Error { ref description } => Some(GcpStatus {
                code: GcpStatusCode::Unavailable.into(),
                message: description.to_string(),
                ..GcpStatus::default()
            }),
        }
    }
}
