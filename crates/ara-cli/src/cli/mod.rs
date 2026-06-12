use anyhow::Result;
use clap::{Parser, Subcommand};

mod analyze;
mod gc;
pub mod install;
mod prompt;
mod run;
mod x;

#[derive(Parser)]
#[command(name = "ara", version = crate::version::VERSION, about = "Ara package manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Install project dependencies
    Install {
        /// Package specifiers to install directly (name, name@version, git URL, etc.)
        deps: Vec<String>,
        /// Save as dev dependency
        #[arg(long)]
        save_dev: bool,
        /// Save as peer dependency
        #[arg(long)]
        save_peer: bool,
        /// Save as optional dependency
        #[arg(long)]
        save_optional: bool,
        /// Version range strategy: "exact" (default), "caret" (^), or "patch" (~)
        #[arg(long)]
        range: Option<String>,
        /// Force re-download even if cached
        #[arg(long)]
        force: bool,
        /// Bypass cache for mutable references (branches, tags)
        #[arg(long)]
        refresh: bool,
        /// Fail if package is not in cache
        #[arg(long)]
        offline: bool,
        #[arg(long)]
        non_interactive: bool,
        /// Generate package-lock.json (temporary compat for deploy platforms)
        #[arg(long)]
        package_lock: bool,
    },
    /// Add project dependencies
    Add {
        /// Package specifiers to install directly (name, name@version, git URL, etc.)
        #[arg(required = true)]
        deps: Vec<String>,
        /// Save as dev dependency
        #[arg(long)]
        save_dev: bool,
        /// Save as peer dependency
        #[arg(long)]
        save_peer: bool,
        /// Save as optional dependency
        #[arg(long)]
        save_optional: bool,
        /// Version range strategy: "exact" (default), "caret" (^), or "patch" (~)
        #[arg(long)]
        range: Option<String>,
        /// Force re-download even if cached
        #[arg(long)]
        force: bool,
        /// Bypass cache for mutable references (branches, tags)
        #[arg(long)]
        refresh: bool,
        /// Fail if package is not in cache
        #[arg(long)]
        offline: bool,
        #[arg(long)]
        non_interactive: bool,
        /// Generate package-lock.json (temporary compat for deploy platforms)
        #[arg(long)]
        package_lock: bool,
    },
    /// Execute a package binary (like npx or pnpm dlx)
    X {
        /// Package to execute (e.g. create-next-app@latest)
        package: String,
        /// Arguments to pass to the package
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
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
    /// Run garbage collection on the store
    Gc {
        /// Show what would be removed without deleting
        #[arg(long)]
        dry_run: bool,
        /// Remove all objects not referenced by any lockfile (full sweep)
        #[arg(long)]
        aggressive: bool,
    },
    /// Trust a package (not yet implemented)
    Trust { package: String },
}

impl Cli {
    pub async fn run(&self) -> Result<()> {
        match &self.command {
            Commands::Install {
                deps,
                save_dev,
                save_peer,
                save_optional,
                range,
                force,
                refresh,
                offline,
                non_interactive,
                package_lock,
            } => {
                if !deps.is_empty() {
                    install::cmd_install_specs(
                        deps,
                        *save_dev,
                        *save_peer,
                        *save_optional,
                        range.as_deref(),
                        *force,
                        *refresh,
                        *offline,
                        *non_interactive,
                        *package_lock,
                    )
                    .await
                } else {
                    install::cmd_install(*non_interactive, *package_lock).await
                }
            }
            Commands::Add {
                deps,
                save_dev,
                save_peer,
                save_optional,
                range,
                force,
                refresh,
                offline,
                non_interactive,
                package_lock,
            } => {
                install::cmd_install_specs(
                    deps,
                    *save_dev,
                    *save_peer,
                    *save_optional,
                    range.as_deref(),
                    *force,
                    *refresh,
                    *offline,
                    *non_interactive,
                    *package_lock,
                )
                .await
            }
            Commands::X { package, args } => x::cmd_x(package, args).await,
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
            Commands::Gc {
                dry_run,
                aggressive,
            } => {
                if *aggressive {
                    gc::cmd_gc_aggressive()
                } else if *dry_run {
                    gc::cmd_gc_dry_run()
                } else {
                    gc::cmd_gc()
                }
            }
            Commands::Trust { package: _ } => {
                eprintln!("ara trust: not yet implemented");
                Ok(())
            }
        }
    }
}
