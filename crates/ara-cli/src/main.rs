#![allow(clippy::multiple_crate_versions)]

use ara_cli::cli::Cli;
use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = cli.run().await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
