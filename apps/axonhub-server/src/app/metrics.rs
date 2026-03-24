use anyhow::{Context, Result};
use axonhub_config::{MetricsConfig, MetricsExporterConfig};
use axonhub_http::HttpMetricsRecorder;
use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, MeterProvider as _};
use opentelemetry::KeyValue;
use opentelemetry_otlp::{MetricExporter, WithExportConfig};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider, Temporality};
use opentelemetry_sdk::Resource;
use opentelemetry_stdout::MetricExporter as StdoutMetricExporter;
use std::sync::Arc;
use std::time::Duration;

const METRICS_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) struct MetricsRuntime {
    provider: SdkMeterProvider,
    recorder: Arc<OpenTelemetryHttpMetricsRecorder>,
}

impl MetricsRuntime {
    pub(crate) fn new(config: &MetricsConfig, meter_name: &str) -> Result<Option<Self>> {
        if !config.enabled {
            return Ok(None);
        }

        let provider = build_meter_provider(&config.exporter, meter_name)?;
        global::set_meter_provider(provider.clone());

        let meter = provider.meter("axonhub");
        let recorder = Arc::new(OpenTelemetryHttpMetricsRecorder::new(&meter)?);

        Ok(Some(Self { provider, recorder }))
    }

    pub(crate) fn recorder(&self) -> Arc<dyn HttpMetricsRecorder> {
        self.recorder.clone()
    }

    pub(crate) fn shutdown(self) -> Result<()> {
        self.provider
            .force_flush()
            .context("failed to flush metrics provider")?;
        self.provider
            .shutdown()
            .context("failed to shut down metrics provider")
    }
}

fn build_meter_provider(
    config: &MetricsExporterConfig,
    service_name: &str,
) -> Result<SdkMeterProvider> {
    let resource = Resource::builder_empty()
        .with_attributes([KeyValue::new("service.name", service_name.to_owned())])
        .build();
    match config.exporter_type.as_str() {
        "stdout" => Ok(SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(
                PeriodicReader::builder(
                    StdoutMetricExporter::builder()
                        .with_temporality(Temporality::Cumulative)
                        .build(),
                )
                .with_interval(METRICS_INTERVAL)
                .build(),
            )
            .build()),
        "otlpgrpc" => Ok(SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(
                PeriodicReader::builder(
                    MetricExporter::builder()
                        .with_tonic()
                        .with_endpoint(exporter_endpoint(config, "http://localhost:4317"))
                        .with_temporality(Temporality::Cumulative)
                        .build()
                        .context("failed to create otlpgrpc metrics exporter")?,
                )
                .with_interval(METRICS_INTERVAL)
                .build(),
            )
            .build()),
        "otlphttp" => Ok(SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(
                PeriodicReader::builder(
                    MetricExporter::builder()
                        .with_http()
                        .with_endpoint(exporter_endpoint(
                            config,
                            "http://localhost:4318/v1/metrics",
                        ))
                        .with_temporality(Temporality::Cumulative)
                        .build()
                        .context("failed to create otlphttp metrics exporter")?,
                )
                .with_interval(METRICS_INTERVAL)
                .build(),
            )
            .build()),
        other => anyhow::bail!("unsupported metrics exporter type: {other:?}"),
    }
}

fn exporter_endpoint(config: &MetricsExporterConfig, fallback: &str) -> String {
    if config.endpoint.trim().is_empty() {
        fallback.to_owned()
    } else {
        config.endpoint.trim().to_owned()
    }
}

struct OpenTelemetryHttpMetricsRecorder {
    request_count: Counter<u64>,
    request_duration_seconds: Histogram<f64>,
}

impl OpenTelemetryHttpMetricsRecorder {
    fn new(meter: &opentelemetry::metrics::Meter) -> Result<Self> {
        let request_count = meter
            .u64_counter("http_request_count")
            .with_description("Number of HTTP requests")
            .with_unit("requests")
            .build();

        let request_duration_seconds = meter
            .f64_histogram("http_request_duration_seconds")
            .with_description("HTTP request duration in seconds")
            .with_unit("seconds")
            .build();

        Ok(Self {
            request_count,
            request_duration_seconds,
        })
    }
}

impl HttpMetricsRecorder for OpenTelemetryHttpMetricsRecorder {
    fn record_http_request(&self, method: &str, path: &str, status_code: u16, duration: Duration) {
        let attributes = [
            KeyValue::new("method", method.to_owned()),
            KeyValue::new("path", path.to_owned()),
            KeyValue::new("status_code", i64::from(status_code)),
        ];

        self.request_count.add(1, &attributes);
        self.request_duration_seconds
            .record(duration.as_secs_f64(), &attributes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_metrics_skip_runtime_creation() {
        let runtime = MetricsRuntime::new(
            &MetricsConfig {
                enabled: false,
                exporter: MetricsExporterConfig::default(),
            },
            "AxonHub",
        )
        .unwrap();

        assert!(runtime.is_none());
    }

    #[test]
    fn unsupported_exporter_type_errors() {
        let error = MetricsRuntime::new(
            &MetricsConfig {
                enabled: true,
                exporter: MetricsExporterConfig {
                    exporter_type: "bogus".to_owned(),
                    endpoint: String::new(),
                    insecure: false,
                },
            },
            "AxonHub",
        );

        let error = match error {
            Ok(_) => panic!("expected unsupported metrics exporter error"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("unsupported metrics exporter type"));
    }

    #[test]
    fn stdout_metrics_runtime_creates_and_shuts_down() {
        let runtime = MetricsRuntime::new(
            &MetricsConfig {
                enabled: true,
                exporter: MetricsExporterConfig {
                    exporter_type: "stdout".to_owned(),
                    endpoint: String::new(),
                    insecure: false,
                },
            },
            "AxonHub",
        )
        .unwrap()
        .expect("metrics runtime should be created");

        runtime
            .recorder()
            .record_http_request("GET", "/health", 200, Duration::from_millis(15));

        runtime.shutdown().unwrap();
    }
}
