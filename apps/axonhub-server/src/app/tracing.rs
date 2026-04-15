use anyhow::{anyhow, Context, Result};
use axonhub_config::{DbConfig, LogConfig, TracesConfig, TracesExporterConfig};
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::sync::OnceLock;
use tracing_log::LogTracer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};

static TRACING_INITIALIZED: OnceLock<()> = OnceLock::new();
static LOG_TRACER_INITIALIZED: OnceLock<()> = OnceLock::new();

struct TraceExporterInit {
    provider: Option<SdkTracerProvider>,
    warning: Option<String>,
}

pub(crate) struct TracingRuntime {
    tracer_provider: Option<SdkTracerProvider>,
}

impl TracingRuntime {
    pub(crate) fn shutdown(self) -> Result<()> {
        if let Some(tracer_provider) = self.tracer_provider {
            tracer_provider
                .force_flush()
                .context("failed to flush trace provider")?;
            tracer_provider
                .shutdown()
                .context("failed to shut down trace provider")?;
        }

        Ok(())
    }
}

pub(crate) fn init_tracing(
    log: &LogConfig,
    db: &DbConfig,
    traces: &TracesConfig,
    service_name: &str,
) -> Result<TracingRuntime> {
    if TRACING_INITIALIZED.get().is_some() {
        return Ok(TracingRuntime {
            tracer_provider: None,
        });
    }

    let filter = build_env_filter(log, db)?;
    let format = tracing_format(log);
    let trace_exporter = build_tracer_provider(traces, service_name);
    let otel_layer = trace_exporter
        .provider
        .as_ref()
        .map(|provider| tracing_opentelemetry::layer().with_tracer(provider.tracer("axonhub")));
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(format)
        .with(otel_layer);

    if LOG_TRACER_INITIALIZED.get().is_none() && LogTracer::init().is_ok() {
        let _ = LOG_TRACER_INITIALIZED.set(());
    }
    tracing::subscriber::set_global_default(subscriber).map_err(|error| anyhow!(error.to_string()))?;
    let _ = TRACING_INITIALIZED.set(());

    if let Some(provider) = trace_exporter.provider.as_ref() {
        let _ = global::set_tracer_provider(provider.clone());
    }

    if let Some(warning) = trace_exporter.warning.as_deref() {
        tracing::warn!(%warning, "trace exporter initialization failed; continuing without trace export");
    }

    tracing::info!(
        service.name = %service_name,
        log.level = %normalize_level(log.level.as_str()),
        log.encoding = %log.encoding,
        db.debug = db.debug,
        "tracing initialized"
    );
    Ok(TracingRuntime {
        tracer_provider: trace_exporter.provider,
    })
}

fn build_tracer_provider(traces: &TracesConfig, service_name: &str) -> TraceExporterInit {
    if !traces.enabled {
        return TraceExporterInit {
            provider: None,
            warning: None,
        };
    }

    match build_tracer_provider_inner(&traces.exporter, service_name) {
        Ok(provider) => TraceExporterInit {
            provider: Some(provider),
            warning: None,
        },
        Err(error) => TraceExporterInit {
            provider: None,
            warning: Some(format!(
                "failed to initialize {:?} trace exporter: {error}",
                traces.exporter.exporter_type
            )),
        },
    }
}

fn build_tracer_provider_inner(
    config: &TracesExporterConfig,
    service_name: &str,
) -> Result<SdkTracerProvider> {
    let resource = Resource::builder_empty()
        .with_attributes([KeyValue::new("service.name", service_name.to_owned())])
        .build();

    match config.exporter_type.as_str() {
        "stdout" => Ok(SdkTracerProvider::builder()
            .with_resource(resource)
            .with_simple_exporter(opentelemetry_stdout::SpanExporter::default())
            .build()),
        "otlpgrpc" => Ok(SdkTracerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(
                SpanExporter::builder()
                    .with_tonic()
                    .with_endpoint(exporter_endpoint(config, "http://localhost:4317"))
                    .build()
                    .context("failed to create otlpgrpc trace exporter")?,
            )
            .build()),
        "otlphttp" => Ok(SdkTracerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(
                SpanExporter::builder()
                    .with_http()
                    .with_endpoint(exporter_endpoint(config, "http://localhost:4318/v1/traces"))
                    .build()
                    .context("failed to create otlphttp trace exporter")?,
            )
            .build()),
        other => anyhow::bail!("unsupported traces exporter type: {other:?}"),
    }
}

fn exporter_endpoint(config: &TracesExporterConfig, fallback: &str) -> String {
    if config.endpoint.trim().is_empty() {
        fallback.to_owned()
    } else {
        config.endpoint.trim().to_owned()
    }
}

fn build_env_filter(log: &LogConfig, db: &DbConfig) -> Result<EnvFilter> {
    EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_filter_directives(log, db)))
        .context("invalid tracing filter directives")
}

fn default_filter_directives(log: &LogConfig, db: &DbConfig) -> String {
    let mut directives = vec![
        normalize_level(log.level.as_str()).to_owned(),
        "actix_web=info".to_owned(),
        "actix_server=info".to_owned(),
    ];

    if log.debug || db.debug {
        directives.push("sqlx=debug".to_owned());
        directives.push("sea_orm=debug".to_owned());
    }

    directives.join(",")
}

fn tracing_format<S>(
    log: &LogConfig,
) -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    let base = fmt::layer()
        .with_target(log.debug)
        .with_thread_ids(log.debug)
        .with_thread_names(log.debug)
        .with_span_events(FmtSpan::CLOSE);

    if log.encoding.trim().eq_ignore_ascii_case("console") {
        base.compact().boxed()
    } else {
        base.json().boxed()
    }
}

fn normalize_level(level: &str) -> &'static str {
    match level.trim().to_ascii_lowercase().as_str() {
        "panic" | "fatal" | "error" => "error",
        "warn" | "warning" => "warn",
        "debug" => "debug",
        _ => "info",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::{Method, StatusCode};
    use actix_web::test as actix_test;
    use axonhub_http::{
        router, AdminCapability, AdminGraphqlCapability, HttpCorsSettings, HttpState,
        IdentityCapability, OauthProviderAdminCapability, OpenAiV1Capability,
        OpenApiGraphqlCapability, RequestContextCapability, SystemBootstrapCapability,
        TraceConfig,
    };
    use std::sync::{Mutex, MutexGuard};

    fn tracing_test_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn disabled_test_cors() -> HttpCorsSettings {
        HttpCorsSettings::default()
    }

    fn test_http_state() -> HttpState {
        HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Unsupported {
                message: "unsupported".to_owned(),
            },
            identity: IdentityCapability::Unsupported {
                message: "unsupported".to_owned(),
            },
            request_context: RequestContextCapability::Unsupported {
                message: "unsupported".to_owned(),
            },
            openai_v1: OpenAiV1Capability::Unsupported {
                message: "unsupported".to_owned(),
            },
            admin: AdminCapability::Unsupported {
                message: "unsupported".to_owned(),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "unsupported".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "unsupported".to_owned(),
            },
            oauth_provider_admin: OauthProviderAdminCapability::Unsupported {
                message: "unsupported".to_owned(),
            },
            allow_no_auth: false,
            cors: disabled_test_cors(),
            request_timeout: Some(std::time::Duration::from_secs(30)),
            llm_request_timeout: Some(std::time::Duration::from_secs(600)),
            trace_config: TraceConfig::default(),
        }
    }

    pub(crate) fn trace_exporter_stdout_emits_http_request_span_inner() {
        let _guard = tracing_test_lock();
        let traces = TracesConfig {
            enabled: true,
            exporter: TracesExporterConfig {
                exporter_type: "stdout".to_owned(),
                endpoint: String::new(),
                insecure: false,
            },
        };

        let runtime = init_tracing(&LogConfig::default(), &DbConfig::default(), &traces, "AxonHub")
            .expect("tracing runtime");

        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        tokio.block_on(async {
            let app = actix_test::init_service(router(test_http_state())).await;
            let response = actix_test::call_service(
                &app,
                actix_test::TestRequest::default()
                    .method(Method::GET)
                    .uri("/health")
                    .to_request(),
            )
            .await;

            assert_eq!(response.status(), StatusCode::OK);
        });

        runtime.shutdown().expect("shutdown tracing runtime");
    }

    pub(crate) fn trace_exporter_invalid_type_fail_open_inner() {
        let _guard = tracing_test_lock();
        let traces = TracesConfig {
            enabled: true,
            exporter: TracesExporterConfig {
                exporter_type: "bogus".to_owned(),
                endpoint: String::new(),
                insecure: false,
            },
        };

        let runtime = init_tracing(&LogConfig::default(), &DbConfig::default(), &traces, "AxonHub")
            .expect("invalid trace exporter should fail open");

        runtime.shutdown().expect("shutdown tracing runtime");
    }

    #[test]
    fn default_filter_enables_sql_debug_when_db_debug_enabled() {
        let log = LogConfig::default();
        let db = DbConfig {
            debug: true,
            ..DbConfig::default()
        };

        let directives = default_filter_directives(&log, &db);

        assert!(directives.contains("sqlx=debug"));
        assert!(directives.contains("sea_orm=debug"));
    }

    #[test]
    fn default_filter_normalizes_warning_alias() {
        let log = LogConfig {
            level: "warning".to_owned(),
            ..LogConfig::default()
        };

        let directives = default_filter_directives(&log, &DbConfig::default());

        assert!(directives.starts_with("warn,"));
    }
}

#[cfg(test)]
pub(crate) fn trace_exporter_stdout_emits_http_request_span_inner() {
    tests::trace_exporter_stdout_emits_http_request_span_inner();
}

#[cfg(test)]
pub(crate) fn trace_exporter_invalid_type_fail_open_inner() {
    tests::trace_exporter_invalid_type_fail_open_inner();
}
