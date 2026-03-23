mod build_info;
mod capabilities;
mod cli;
mod server;

use anyhow::Result;
use std::env;

pub(crate) async fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    cli::run(&args).await
}

#[cfg(test)]
mod tests;
