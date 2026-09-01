//! `solenv list`: show configured and installed toolchain versions.

use anyhow::Result;

use super::context::{env_for, require_config, resolve_root};
use crate::cli::Cli;
use crate::managers::anchor::AnchorManager;
use crate::managers::node::NodeManager;
use crate::managers::rust::RustManager;
use crate::managers::solana::SolanaManager;
use crate::managers::Manager;

pub fn run(cli: &Cli) -> Result<()> {
    let root = resolve_root(cli)?;
    let cfg = require_config(&root)?;
    let env = env_for(&root);
    let tc = cfg.toolchain.clone().unwrap_or_default();

    let rust = RustManager::new();
    let solana = SolanaManager::new();
    let anchor = AnchorManager::new();
    let node = NodeManager::new();

    println!("Configured toolchain (solenv.toml)");
    println!("{}", "─".repeat(32));
    let entries: Vec<(&'static str, &'static str, String)> = vec![
        ("rust", "Rust", tc.rust.clone().unwrap_or_default()),
        (
            "solana",
            "Agave/Solana",
            tc.solana.clone().unwrap_or_default(),
        ),
        ("anchor", "Anchor", tc.anchor.clone().unwrap_or_default()),
        ("node", "Node", tc.node.clone().unwrap_or_default()),
    ];
    for (tool, label, ver) in &entries {
        let m: &dyn Manager = match *tool {
            "rust" => &rust,
            "solana" => &solana,
            "anchor" => &anchor,
            "node" => &node,
            _ => unreachable!(),
        };
        let resolved = m.resolve(ver).unwrap_or_else(|_| ver.clone());
        let installed = !ver.is_empty() && m.is_installed(&env, &resolved);
        let mark = if ver.is_empty() {
            "not pinned".to_string()
        } else if installed {
            "installed".to_string()
        } else {
            "not installed".to_string()
        };
        println!("{:<14} {:<16} ({})", label, resolved, mark);
    }

    if let Some(pm) = &tc.package_manager {
        println!("{:<14} {:<16}", "package_manager", pm);
    }

    println!();
    println!("Installed versions in .solenv");
    println!("{}", "─".repeat(32));
    let tools: Vec<(&'static str, &'static str)> = vec![
        ("rust", "rust"),
        ("solana", "solana"),
        ("anchor", "anchor"),
        ("node", "node"),
    ];
    let mut any = false;
    for (key, label) in tools {
        let versions = env.installed_versions(key);
        if !versions.is_empty() {
            any = true;
            println!("{:<14} {}", label, versions.join(", "));
        }
    }
    if !any {
        println!("  (nothing installed yet — run `solenv install`)");
    }

    Ok(())
}

#[allow(dead_code)]
fn _ctx() -> anyhow::Result<()> {
    Ok(())
}
