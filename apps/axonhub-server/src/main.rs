mod app;
mod foundation;

use std::process;
use tracing::error;

#[tokio::main]
async fn main() {
    if let Err(error) = app::run().await {
        error!(error = %error, "axonhub terminated with an error");
        eprintln!("{error}");
        process::exit(1);
    }
}

#[cfg(test)]
#[test]
fn trace_exporter_stdout_emits_http_request_span() {
    app::trace_exporter_stdout_emits_http_request_span_inner();
}

#[cfg(test)]
#[test]
fn trace_exporter_invalid_type_fail_open() {
    app::trace_exporter_invalid_type_fail_open_inner();
}

#[cfg(test)]
#[test]
fn build_upstream_headers_injects_w3c_trace_headers() {
    foundation::openai_v1::build_upstream_headers_injects_w3c_trace_headers_inner();
}

#[cfg(test)]
#[test]
fn openai_v1_execution_span_avoids_sensitive_fields() {
    foundation::openai_v1::openai_v1_execution_span_avoids_sensitive_fields_inner();
}

#[cfg(test)]
#[test]
fn seaorm_run_sync_preserves_trace_context_across_bridge() {
    foundation::seaorm::seaorm_run_sync_preserves_trace_context_across_bridge_inner();
}

#[cfg(test)]
#[test]
fn schema_ownership_contract_limits_raw_sql_usage() {
    foundation::schema_governance::schema_ownership::schema_ownership_contract_limits_raw_sql_usage_inner();
}

