use anyhow::Result;
use clap::{Parser, Subcommand};

mod analyze;
mod gc;
pub(crate) mod install;
mod prompt;
mod run;

#[derive(Parser)]
#[command(name = "ara", version, about = "Ara package manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Install project dependencies
    Install {
        #[arg(long)]
        non_interactive: bool,
    },
    /// Run a script in a sandboxed environment
    Run {
        script: String,
        #[arg(long, default_value = "runtime")]
        profile: String,
    },
    /// Analyze a package for security patterns
    Analyze {
        #[arg(default_value = ".")]
        path: String,
    },
    /// Full security audit of a package
    Audit {
        #[arg(default_value = ".")]
        path: String,
    },
    /// Build the project (not yet implemented)
    Build,
    /// Publish the project (not yet implemented)
    Publish,
    /// Run garbage collection on the store (not yet implemented)
    Gc,
    /// Trust a package (not yet implemented)
    Trust { package: String },
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        match &self.command {
            Commands::Install { non_interactive } => install::cmd_install(*non_interactive),
            Commands::Run { script, profile } => run::cmd_run(script, profile),
            Commands::Analyze { path } => analyze::cmd_analyze(path),
            Commands::Audit { path } => analyze::cmd_audit(path),
            Commands::Build => {
                eprintln!("ara build: not yet implemented");
                Ok(())
            }
            Commands::Publish => {
                eprintln!("ara publish: not yet implemented");
                Ok(())
            }
            Commands::Gc => gc::cmd_gc(),
            Commands::Trust { package: _ } => {
                eprintln!("ara trust: not yet implemented");
                Ok(())
            }
        }
    }
}
