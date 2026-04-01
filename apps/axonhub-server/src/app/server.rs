use anyhow::{Context, Result};
use actix_web::{HttpServer, dev::ServerHandle};
use axonhub_config::load;
use axonhub_http::{
    HttpCorsSettings, HttpMetricsCapability, HttpState, TraceConfig,
    router_with_metrics_and_base_path,
};
use std::net::SocketAddr;
use std::process;

use super::build_info::version;
use super::capabilities::build_server_capabilities;
use super::metrics::MetricsRuntime;

fn runtime_cors_settings(config: &axonhub_config::CorsConfig) -> HttpCorsSettings {
    let max_age_seconds = humantime::parse_duration(&config.max_age)
        .ok()
        .and_then(|duration| duration.as_secs().try_into().ok());

    HttpCorsSettings {
        enabled: config.enabled,
        debug: config.debug,
        allowed_origins: config.allowed_origins.clone(),
        allowed_methods: config.allowed_methods.clone(),
        allowed_headers: config.allowed_headers.clone(),
        exposed_headers: config.exposed_headers.clone(),
        allow_credentials: config.allow_credentials,
        max_age_seconds,
    }
}

pub(crate) async fn start_server() -> Result<()> {
    let loaded = load().unwrap_or_else(|error| {
        eprintln!("Failed to load config: {error}");
        process::exit(1);
    });
    let port: u16 = loaded
        .config
        .server
        .port
        .try_into()
        .context("server.port must be between 1 and 65535")?;

    let address = format!("{}:{port}", loaded.config.server.host);
    let capabilities = build_server_capabilities(
        &loaded.config.db.dialect,
        &loaded.config.db.dsn,
        loaded.config.server.api.auth.allow_no_auth,
        version(),
    );
    let state = HttpState {
        service_name: loaded.config.server.name.clone(),
        version: version().to_owned(),
        config_source: loaded
            .source
            .as_ref()
            .map(|path| path.display().to_string()),
        system_bootstrap: capabilities.system_bootstrap,
        identity: capabilities.identity,
        request_context: capabilities.request_context,
        openai_v1: capabilities.openai_v1,
        admin: capabilities.admin,
        admin_graphql: capabilities.admin_graphql,
        openapi_graphql: capabilities.openapi_graphql,
        provider_edge_admin: capabilities.provider_edge_admin,
        allow_no_auth: loaded.config.server.api.auth.allow_no_auth,
        cors: runtime_cors_settings(&loaded.config.server.cors),
        trace_config: TraceConfig {
            thread_header: Some(loaded.config.server.trace.thread_header.clone()),
            trace_header: Some(loaded.config.server.trace.trace_header.clone()),
            request_header: Some(loaded.config.server.trace.request_header.clone()),
            extra_trace_headers: loaded.config.server.trace.extra_trace_headers.clone(),
            extra_trace_body_fields: loaded.config.server.trace.extra_trace_body_fields.clone(),
            claude_code_trace_enabled: loaded.config.server.trace.claude_code_trace_enabled,
            codex_trace_enabled: loaded.config.server.trace.codex_trace_enabled,
        },
    };

    let metrics_runtime = MetricsRuntime::new(&loaded.config.metrics, &loaded.config.server.name)?;
    let http_metrics = if let Some(metrics_runtime) = metrics_runtime.as_ref() {
        HttpMetricsCapability::Available {
            recorder: metrics_runtime.recorder(),
        }
    } else {
        HttpMetricsCapability::Disabled
    };

    let state_for_server = state.clone();
    let base_path = loaded.config.server.base_path.clone();
    let server = HttpServer::new(move || {
        router_with_metrics_and_base_path(
            state_for_server.clone(),
            http_metrics.clone(),
            &base_path,
        )
    })
    .disable_signals()
    .bind(&address)
    .with_context(|| format!("Failed to bind {address}"))?;

    let listener_address = server.addrs().first().copied().context("No HTTP listener address bound")?;
    let service_name = loaded.config.server.name.clone();

    for line in startup_messages(
        &service_name,
        listener_address,
        loaded.config.metrics.enabled,
    ) {
        println!("{line}");
    }

    let server = server.run();
    let server_handle = server.handle();
    let server_result = run_server_with_shutdown(server, server_handle)
        .await
        .context("HTTP server exited unexpectedly");

    if let Some(metrics_runtime) = metrics_runtime {
        metrics_runtime.shutdown()?;
    }

    server_result
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn run_server_with_shutdown(
    server: actix_web::dev::Server,
    handle: ServerHandle,
) -> std::io::Result<()> {
    tokio::select! {
        result = server => result,
        _ = shutdown_signal() => {
            handle.stop(true).await;
            Ok(())
        }
    }
}

pub(crate) fn startup_messages(
    service_name: &str,
    listener_address: SocketAddr,
    metrics_enabled: bool,
) -> Vec<String> {
    let mut messages = Vec::new();

    if metrics_enabled {
        messages.push("Metrics exporter initialized for Rust server runtime.".to_owned());
    }

    messages.push(format!(
        "{service_name} listening on http://{listener_address}"
    ));

    messages
}
