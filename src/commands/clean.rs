//! `solenv clean`: remove installed toolchain versions (keeps solenv.toml).

use std::io::Write;
use std::time::Instant;

use anyhow::{bail, Result};

use super::context::{env_for, require_config, resolve_root};
use crate::cli::Cli;

pub fn run(cli: &Cli, clean_cache: bool, force: bool) -> Result<()> {
    let root = resolve_root(cli)?;
    let _cfg = require_config(&root)?;
    let env = env_for(&root);

    let solenv = env.solenv_dir();
    if !solenv.exists() {
        bail!("nothing to clean: {} does not exist", solenv.display());
    }

    if !force
        && !confirm(
            "Remove installed toolchain versions under .solenv/versions (keeps solenv.toml)? [y/N] ",
        )? {
            println!("Aborted.");
            return Ok(());
        }

    let start = Instant::now();
    let mut removed = 0usize;

    let versions = env.versions_dir();
    if versions.exists() {
        for tool in std::fs::read_dir(&versions)? {
            let tool = tool?;
            if tool.path().is_dir() {
                for ver in std::fs::read_dir(tool.path())? {
                    let ver = ver?;
                    let p = ver.path();
                    println!("  removing {}", p.display());
                    std::fs::remove_dir_all(&p)?;
                    removed += 1;
                }
            }
        }
    }

    if clean_cache {
        let cache = env.cache_dir();
        if cache.exists() {
            println!("  removing cache {}", cache.display());
            std::fs::remove_dir_all(&cache)?;
            std::fs::create_dir_all(&cache)?;
        }
    }

    // Reset state to empty.
    let empty = crate::environment::State::default();
    env.save_state(&empty)?;

    println!(
        "✓ removed {removed} installed version(s) in {:.2}s",
        start.elapsed().as_secs_f32()
    );
    println!("Configuration solenv.toml was kept. Run `solenv install` to reinstall.");

    Ok(())
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
