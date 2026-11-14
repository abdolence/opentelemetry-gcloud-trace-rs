//! OTLP sink: hands each event to an OpenTelemetry [`SdkLogger`], which exports it to the
//! Google Cloud Telemetry API when the provider was built by
//! [`crate::otlp::GcpOtlpExporterBuilder::create_logger_provider`].

mod record;

use crate::ids::LogId;
use crate::logs::error_reporting::ExceptionDetails;
use crate::logs::format::TraceCorrelation;
use opentelemetry::logs::Logger;
use opentelemetry_sdk::logs::SdkLogger;
use serde_json::{Map, Value};

/// The OpenTelemetry logger a [`crate::logs::GcpCloudLoggingLayer`] sends its entries through,
/// and the log they belong to. Created by
/// [`crate::logs::GcpCloudLoggingLayerBuilder::with_otel_logger_provider`].
pub struct OtlpSink {
    logger: SdkLogger,
    log_id: LogId,
}

impl OtlpSink {
    pub(crate) fn new(logger: SdkLogger, log_id: LogId) -> Self {
        Self { logger, log_id }
    }

    pub(crate) fn send(
        &self,
        record: Map<String, Value>,
        exception: Option<&ExceptionDetails>,
        correlation: Option<&TraceCorrelation>,
    ) {
        let mut log_record = self.logger.create_log_record();
        record::populate(
            &mut log_record,
            record,
            exception,
            correlation,
            &self.log_id,
        );
        self.logger.emit(log_record);
    }
}
