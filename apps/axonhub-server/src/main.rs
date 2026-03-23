mod app;
mod foundation;

use std::process;

#[tokio::main]
async fn main() {
    if let Err(error) = app::run().await {
        eprintln!("{error}");
        process::exit(1);
    }
}
