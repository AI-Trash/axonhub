use anyhow::{Context, Result};
use axonhub_config::load;
use axonhub_http::{router, router_with_metrics, HttpMetricsCapability, HttpState, TraceConfig};
use axum::Router;
use std::net::SocketAddr;
use std::process;

use super::build_info::version;
use super::capabilities::{
    build_admin_capability, build_admin_graphql_capability, build_identity_capability,
    build_openai_v1_capability, build_openapi_graphql_capability,
    build_provider_edge_admin_capability, build_system_bootstrap_capability,
    build_request_context_capability,
};
use super::metrics::MetricsRuntime;

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
    let state = HttpState {
        service_name: loaded.config.server.name.clone(),
        version: version().to_owned(),
        config_source: loaded
            .source
            .as_ref()
            .map(|path| path.display().to_string()),
        system_bootstrap: build_system_bootstrap_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
            version(),
        ),
        identity: build_identity_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
            loaded.config.server.api.auth.allow_no_auth,
        ),
        request_context: build_request_context_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
            loaded.config.server.api.auth.allow_no_auth,
        ),
        openai_v1: build_openai_v1_capability(&loaded.config.db.dialect, &loaded.config.db.dsn),
        admin: build_admin_capability(&loaded.config.db.dialect, &loaded.config.db.dsn),
        admin_graphql: build_admin_graphql_capability(&loaded.config.db.dialect, &loaded.config.db.dsn),
        openapi_graphql: build_openapi_graphql_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
        ),
        provider_edge_admin: build_provider_edge_admin_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
        ),
        allow_no_auth: loaded.config.server.api.auth.allow_no_auth,
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
    let app = if let Some(metrics_runtime) = metrics_runtime.as_ref() {
        mount_base_path(
            router_with_metrics(
                state,
                HttpMetricsCapability::Available {
                    recorder: metrics_runtime.recorder(),
                },
            ),
            &loaded.config.server.base_path,
        )
    } else {
        mount_base_path(router(state), &loaded.config.server.base_path)
    };
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .with_context(|| format!("Failed to bind {address}"))?;

    let listener_address = listener.local_addr()?;
    let service_name = loaded.config.server.name.clone();

    for line in startup_messages(&service_name, listener_address, loaded.config.metrics.enabled) {
        println!("{line}");
    }

    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server exited unexpectedly");

    if let Some(metrics_runtime) = metrics_runtime {
        metrics_runtime.shutdown()?;
    }

    server_result
}

pub(crate) fn mount_base_path(app: Router, base_path: &str) -> Router {
    let normalized = base_path.trim();
    if normalized.is_empty() || normalized == "/" {
        return app;
    }

    let prefixed = format!("/{}", normalized.trim_matches('/'));
    Router::new().nest(&prefixed, app)
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
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
