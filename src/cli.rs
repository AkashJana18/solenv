//! Command-line interface definition (clap derive).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// A project-local Solana development environment manager.
#[derive(Debug, Parser)]
#[command(name = "solenv", version, about, long_about = None)]
pub struct Cli {
    /// Directory to treat as the project root (defaults to nearest dir
    /// containing solenv.toml, else the current directory).
    #[arg(long, global = true)]
    pub dir: Option<PathBuf>,

    /// Suppress progress/spinner output.
    #[arg(long, short, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a solenv.toml, detecting the current toolchain.
    Init(InitArgs),

    /// Install the pinned toolchain into .solenv/.
    Install(InstallArgs),

    /// Validate the pinned toolchain against the compatibility matrix.
    Check {
        /// Do not attempt to read installed versions; only compare declared.
        #[arg(long)]
        declared_only: bool,
    },

    /// List configured and installed toolchain versions.
    List,

    /// Run a command with the project's pinned toolchain on PATH.
    Run {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND",
            required = true
        )]
        command: Vec<String>,
    },

    /// Diagnose common environment problems.
    Doctor,

    /// Remove installed toolchain versions from .solenv/ (keeps config).
    Clean {
        /// Also remove all cached downloads.
        #[arg(long)]
        cache: bool,
        /// Do not prompt for confirmation.
        #[arg(long)]
        yes: bool,
    },

    /// Uninstall solenv's project-local installations for this project.
    Uninstall {
        /// Do not prompt for confirmation.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Parser)]
pub struct InitArgs {
    /// Write the detected versions without prompting (non-interactive).
    #[arg(long)]
    pub yes: bool,
    /// Override a specific tool version, e.g. --set rust=1.92.0 (repeatable).
    #[arg(long, value_name = "TOOL=VERSION")]
    pub set: Vec<String>,
}

#[derive(Debug, Parser)]
pub struct InstallArgs {
    /// Reinstall even if already installed (force re-extract/copy).
    #[arg(long)]
    pub force: bool,
    /// Which tools to install (rust, solana, anchor, node). Default: all.
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,
}
