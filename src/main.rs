#![allow(clippy::multiple_crate_versions)]
use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = ara::cli::Cli::parse();
    if let Err(e) = cli.run().await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
