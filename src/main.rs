use clap::Parser;

fn main() {
    let cli = ara::cli::Cli::parse();
    if let Err(e) = cli.run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
