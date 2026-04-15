pub(crate) mod build_info;
mod capabilities;
mod cli;
mod metrics;
mod server;
mod services;
mod tracing;

use anyhow::Result;
use std::env;

pub(crate) async fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    cli::run(&args).await
}

#[cfg(any())]
pub(crate) mod test_support;

#[cfg(any())]
mod tests;

#[cfg(test)]
pub(crate) use tracing::{
    trace_exporter_invalid_type_fail_open_inner,
    trace_exporter_stdout_emits_http_request_span_inner,
};
