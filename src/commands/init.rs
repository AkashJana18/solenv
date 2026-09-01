//! `solenv init`: detect toolchain, ask, and write `solenv.toml`.

use std::io::Write;

use anyhow::{Context, Result};

use crate::cli::{Cli, InitArgs};
use crate::config::{default_config, detect_from_dir, load, SolenvConfig};

use super::context::resolve_root;

pub fn run(cli: &Cli, args: &InitArgs) -> Result<()> {
    let root = resolve_root(cli)?;
    let config_path = root.join("solenv.toml");

    if config_path.exists() {
        let existing = load(&config_path)?;
        println!("solenv.toml already exists at {}", config_path.display());
        println!("Existing toolchain:");
        print_toolchain(&existing);
        // Re-write only if --yes forcing; otherwise show current.
        if !args.yes {
            println!("\nUse `solenv install` to install, or edit solenv.toml directly.");
            return Ok(());
        }
    }

    println!("Initializing solenv in {}", root.display());

    // Detect installed toolchain + Anchor.toml.
    let mut detected = detect_from_dir(&root);

    // Apply --set overrides.
    for pair in &args.set {
        let (tool, ver) = pair
            .split_once('=')
            .with_context(|| format!("--set expects TOOL=VERSION, got {pair:?}"))?;
        let tool = tool.to_ascii_lowercase();
        match tool.as_str() {
            "rust" => detected.rust = Some(ver.trim().to_string()),
            "solana" => detected.solana = Some(ver.trim().to_string()),
            "anchor" => detected.anchor = Some(ver.trim().to_string()),
            "node" => detected.node = Some(ver.trim().to_string()),
            "package_manager" | "package-manager" => {
                detected.package_manager = Some(ver.trim().to_string())
            }
            other => anyhow::bail!(
                "unknown tool {other:?}; expected rust, solana, anchor, node or package_manager"
            ),
        }
    }

    println!("\nDetected toolchain:");
    if detected.rust.is_some() {
        println!("  Rust        {}", detected.rust.as_deref().unwrap_or("-"));
    }
    if detected.solana.is_some() {
        println!(
            "  Agave       {}",
            detected.solana.as_deref().unwrap_or("-")
        );
    }
    if detected.anchor.is_some() {
        println!(
            "  Anchor      {}",
            detected.anchor.as_deref().unwrap_or("-")
        );
    }
    if detected.node.is_some() {
        println!("  Node        {}", detected.node.as_deref().unwrap_or("-"));
    }
    if detected.package_manager.is_some() {
        println!(
            "  pkg manager {}",
            detected.package_manager.as_deref().unwrap_or("-")
        );
    }

    if !args.yes && !confirm("\nWrite these versions into solenv.toml? [y/N] ")? {
        println!("Aborted. Nothing written.");
        return Ok(());
    }

    let cfg = default_config(&detected);
    let text = cfg.to_toml().context("failed to serialize config")?;
    std::fs::write(&config_path, text)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    println!("✓ Created {}", config_path.display());
    println!("Next: run `solenv install` to set up the toolchain, then `solenv check`.");

    Ok(())
}

pub fn print_toolchain(cfg: &SolenvConfig) {
    let Some(tc) = &cfg.toolchain else {
        println!("  (no [toolchain] section)");
        return;
    };
    if let Some(v) = &tc.rust {
        println!("  Rust   {v}");
    }
    if let Some(v) = &tc.solana {
        println!("  Agave  {v}");
    }
    if let Some(v) = &tc.anchor {
        println!("  Anchor {v}");
    }
    if let Some(v) = &tc.node {
        println!("  Node   {v}");
    }
    if let Some(v) = &tc.package_manager {
        println!("  pkg    {v}");
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    let t = line.trim().to_ascii_lowercase();
    Ok(t == "y" || t == "yes")
}
