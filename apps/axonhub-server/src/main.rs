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
fn openai_v1_runtime_contract_preserved() {
    foundation::openai_v1_runtime_contract_preserved_inner();
}

#[cfg(test)]
#[test]
fn runtime_query_semantics_preserved_after_rewrite() {
    foundation::openai_v1_runtime_contract_preserved_inner();
}

#[cfg(test)]
#[test]
fn parity_oracle_helpers_preserve_contract() {
    app::parity_oracle_helpers_preserve_contract_inner();
}
