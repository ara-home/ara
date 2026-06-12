#![cfg_attr(all(test, feature = "nightly-bench"), feature(test))]
#![cfg_attr(all(test, feature = "nightly-bench"), allow(unused_extern_crates))]
#![allow(clippy::multiple_crate_versions)]
#[cfg(all(test, feature = "nightly-bench"))]
extern crate test;

mod cli;
mod version;

use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = cli.run().await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
