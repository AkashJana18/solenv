//! solenv — a Python-venv-like project-local Solana toolchain manager.

use clap::Parser;
use solenv::cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    if let Err(e) = dispatch(&cli) {
        eprintln!("\n{}", solenv::errors::render(&e));
        std::process::exit(1);
    }
}

fn dispatch(cli: &Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Init(args) => solenv::commands::init::run(cli, args),
        Command::Install(args) => solenv::commands::install::run(cli, args),
        Command::Check { .. } => solenv::commands::check::run(cli),
        Command::List => solenv::commands::list::run(cli),
        Command::Run { command } => {
            let code = solenv::commands::run::run(cli, command)?;
            std::process::exit(code);
        }
        Command::Doctor => solenv::commands::doctor::run(cli),
        Command::Clean { cache, yes } => solenv::commands::clean::run(cli, *cache, *yes),
        Command::Uninstall { yes } => solenv::commands::uninstall::run(cli, *yes),
    }
}
