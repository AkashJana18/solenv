//! `solenv uninstall`: remove the project-local environment entirely.

use std::io::Write;
use std::path::Path;

use anyhow::{bail, Result};

use super::context::{env_for, require_config, resolve_root};
use crate::cli::Cli;

pub fn run(cli: &Cli, yes: bool) -> Result<()> {
    let root = resolve_root(cli)?;
    let _cfg = require_config(&root)?;
    let env = env_for(&root);

    let solenv = env.solenv_dir();
    if !solenv.exists() {
        bail!("nothing to uninstall: {} does not exist", solenv.display());
    }

    println!(
        "This will permanently remove the solenv environment at {}",
        solenv.display()
    );
    println!("Your solenv.toml configuration file will be kept.");

    if !yes && !confirm("\nContinue? [y/N] ")? {
        println!("Aborted.");
        return Ok(());
    }

    remove_dir_all(&solenv)?;
    println!("✓ Removed {}", solenv.display());
    println!("solenv.toml was kept.");

    Ok(())
}

/// Cross-platform directory removal with a helpful error.
fn remove_dir_all(path: &Path) -> Result<()> {
    std::fs::remove_dir_all(path)
        .map_err(|e| anyhow::anyhow!("failed to remove {}: {e}", path.display()))
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
