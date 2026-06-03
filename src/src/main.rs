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

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = cli.run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
