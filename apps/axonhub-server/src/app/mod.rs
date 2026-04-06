mod build_info;
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

#[cfg(test)]
mod parity_oracle;

#[cfg(test)]
mod tests;
