//! `solenv install`: validate the toolchain and install it into `.solenv`.

use anyhow::{bail, Result};

use super::context::{env_for, require_config, resolve_root};
use crate::cli::{Cli, InstallArgs};
use crate::compatibility::{self, validate, ToolchainRequest};
use crate::environment::Environment;
use crate::managers::anchor::AnchorManager;
use crate::managers::node::NodeManager;
use crate::managers::rust::RustManager;
use crate::managers::solana::SolanaManager;
use crate::managers::Manager;
use crate::package_manager::resolve as resolve_pm;

pub fn run(cli: &Cli, args: &InstallArgs) -> Result<()> {
    let root = resolve_root(cli)?;
    let cfg = require_config(&root)?;
    let env = env_for(&root);

    let Some(tc) = &cfg.toolchain else {
        bail!(
            "solenv.toml has no [toolchain] section. Add versions, e.g.\n\
             [toolchain]\nrust = \"1.92.0\"\nsolana = \"3.1.10\"\nanchor = \"1.1.2\""
        );
    };

    println!("Installing toolchain for {}", root.display());

    // 1. Validate compatibility first.
    let req: ToolchainRequest = tc.into();
    let violations = validate(&req)?;

    let wanted: Vec<&'static str> = if args.only.is_empty() {
        vec!["rust", "solana", "anchor", "node"]
    } else {
        args.only.iter().map(|s| normalize_tool(s)).collect()
    };

    if !args.force && !violations.is_empty() {
        println!("Compatibility problems were found:\n");
        for v in &violations {
            println!("{}", v.display());
        }
        println!(
            "\nRefusing to install an incompatible toolchain. Use `solenv check` for details, or install anyway with `--force` (not recommended). See:\n  {}",
            compatibility::COMPATIBILITY_MATRIX_URL
        );
        return Ok(());
    } else if !violations.is_empty() {
        println!("Compatibility warnings being ignored (--force):\n");
        for v in &violations {
            println!("{}", v.display());
        }
        println!();
    }

    // 2. Install each tool.
    let rust = RustManager::new();
    let solana = SolanaManager::new();
    let anchor = AnchorManager::new();
    let node = NodeManager::new();

    if wanted.contains(&"rust") {
        if let Some(ver) = &tc.rust {
            install_tool(&rust, &env, ver, args.force)?;
        } else {
            println!("  (rust not pinned; skipping — add rust to [toolchain])");
        }
    }
    if wanted.contains(&"solana") {
        if let Some(ver) = &tc.solana {
            install_tool(&solana, &env, ver, args.force)?;
        } else {
            println!("  (solana not pinned; skipping — add solana to [toolchain])");
        }
    }
    if wanted.contains(&"anchor") {
        if let Some(ver) = &tc.anchor {
            install_tool(&anchor, &env, ver, args.force)?;
        } else {
            println!("  (anchor not pinned; skipping — add anchor to [toolchain])");
        }
    }

    let mut node_bin_dir = None;
    if wanted.contains(&"node") {
        if let Some(ver) = &tc.node {
            install_tool(&node, &env, ver, args.force)?;
            node_bin_dir = node.resolve_bin_dir(&env, ver).ok();
        } else {
            println!("  (node not pinned; skipping — add node to [toolchain])");
        }
    }

    // 3. Provision the package manager.
    if let Some(pm) = &tc.package_manager {
        match resolve_pm(&env, pm, node_bin_dir.as_deref(), true) {
            Ok(Some(_m)) => println!("✓ Package manager {pm} ready"),
            Ok(None) => println!(
                "! Package manager {pm} not directly managed by solenv; using it from PATH if present"
            ),
            Err(e) => println!("! Could not provision {pm}: {e}"),
        }
    }

    println!("\nInstallation complete.");
    println!("Run `solenv check` to verify, and `solenv run <cmd>` to use the toolchain.");

    Ok(())
}

fn normalize_tool(t: &str) -> &'static str {
    match t.trim().to_ascii_lowercase().as_str() {
        "rust" | "rustc" => "rust",
        "solana" | "agave" => "solana",
        "anchor" => "anchor",
        _ => "node",
    }
}

fn install_tool<M: Manager>(m: &M, env: &Environment, version: &str, force: bool) -> Result<()> {
    let resolved = m.resolve(version)?;
    let already = m.is_installed(env, &resolved);
    if already && !force {
        println!("✓ {} {} already installed", m.label(), resolved);
        return Ok(());
    }
    m.install(env, &resolved)?;
    println!("✓ {} {}", m.label(), resolved);
    Ok(())
}
