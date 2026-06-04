#![cfg_attr(all(test, feature = "nightly-bench"), feature(test))]
#![cfg_attr(all(test, feature = "nightly-bench"), allow(unused_extern_crates))]
#[cfg(all(test, feature = "nightly-bench"))]
extern crate test;

mod analysis;
mod cli;
mod lockfile;
mod manifest;
mod resolver;
mod sandbox;
mod source;
mod store;
mod types;
mod util;
mod version;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = cli.run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
