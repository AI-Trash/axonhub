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
