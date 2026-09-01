//! `solenv check`: show the environment and validate compatibility.

use anyhow::Result;

use super::context::{env_for, require_config, resolve_root};
use crate::cli::Cli;
use crate::compatibility::{self, build_recommendation, validate, ToolchainRequest};
use crate::managers::anchor::AnchorManager;
use crate::managers::node::NodeManager;
use crate::managers::rust::RustManager;
use crate::managers::solana::SolanaManager;
use crate::managers::Manager;

struct Row {
    name: String,
    version: String,
    installed: bool,
}

pub fn run(cli: &Cli) -> Result<()> {
    let root = resolve_root(cli)?;
    let cfg = require_config(&root)?;
    let env = env_for(&root);

    let tc = cfg.toolchain.clone().unwrap_or_default();

    let rust = RustManager::new();
    let solana = SolanaManager::new();
    let anchor = AnchorManager::new();
    let node = NodeManager::new();

    let mut rows: Vec<Row> = Vec::new();

    let rust_ver = tc.rust.clone().unwrap_or_default();
    let (rust_resolved, rust_installed) = resolve_tool(&rust, &env, &rust_ver);
    rows.push(Row {
        name: "Rust".to_string(),
        version: rust_resolved,
        installed: rust_installed,
    });

    let solana_ver = tc.solana.clone().unwrap_or_default();
    let (solana_resolved, solana_installed) = resolve_tool(&solana, &env, &solana_ver);
    rows.push(Row {
        name: "Agave".to_string(),
        version: solana_resolved,
        installed: solana_installed,
    });

    let anchor_ver = tc.anchor.clone().unwrap_or_default();
    let (anchor_resolved, anchor_installed) = resolve_tool(&anchor, &env, &anchor_ver);
    rows.push(Row {
        name: "Anchor".to_string(),
        version: anchor_resolved,
        installed: anchor_installed,
    });

    let node_spec = tc.node.clone().unwrap_or_default();
    let (node_resolved, node_installed) = resolve_tool(&node, &env, &node_spec);
    rows.push(Row {
        name: "Node".to_string(),
        version: node_resolved,
        installed: node_installed,
    });

    let pm = tc
        .package_manager
        .clone()
        .unwrap_or_else(|| "npm".to_string());
    let pm_version = detect_pm_version(&pm);
    let pm_installed = !pm_version.is_empty();
    rows.push(Row {
        name: pm,
        version: if pm_installed {
            pm_version
        } else {
            "not found".to_string()
        },
        installed: pm_installed,
    });

    // ---- Tool table ----
    println!("Solana Environment");
    println!("{}", "─".repeat(30));
    for r in &rows {
        let mark = if r.installed { "✓" } else { "✗" };
        println!("{:<12} {:<16} {}", r.name, r.version, mark);
    }
    println!("{}", "─".repeat(30));

    // ---- Compatibility ----
    let req: ToolchainRequest = (&tc).into();
    let violations = validate(&req)?;

    println!();
    println!("Compatibility");
    println!("{}", "─".repeat(30));

    if violations.is_empty() {
        if req.anchor.is_some() && req.solana.is_some() {
            println!("✓ Anchor / Agave");
        }
        if req.anchor.is_some() && req.rust.is_some() {
            println!("✓ Rust / Solana");
        }
        if req.anchor.is_some() && req.node.is_some() {
            println!("✓ Node / Anchor");
        }
        let all_installed = rows
            .iter()
            .filter(|r| r.name != "pnpm" && r.name != "npm" && r.name != "yarn" && r.name != "bun")
            .all(|r| r.installed || is_global_ok(&r.name));
        if all_installed || violations.is_empty() {
            println!("\nEnvironment is healthy.");
        }
    } else {
        for v in &violations {
            println!("{}", v.display());
        }
        println!("\n✗ Incompatible toolchain");
        let rec = build_recommendation(&req);
        println!("Recommended:");
        for (tool, val) in rec {
            if tool != "platform_tools" {
                println!("  {} {}", tool, val);
            }
        }
        println!("\nSee:");
        println!("  {}", compatibility::COMPATIBILITY_MATRIX_URL);
        println!("\nRun `solenv doctor` for detailed diagnostics.");
    }

    Ok(())
}

/// A version is "ok" if it's present globally even if not in `.solenv`
/// (solenv can still operate via orchestration). For MVP we treat absence as
/// requiring `solenv install`.
fn is_global_ok(_name: &str) -> bool {
    false
}

/// Resolve a pin to a concrete version and report whether it's installed.
/// Unpinned / unresolved pins display as "not pinned".
fn resolve_tool<M: Manager>(
    m: &M,
    env: &crate::environment::Environment,
    spec: &str,
) -> (String, bool) {
    if spec.is_empty() {
        return ("not pinned".to_string(), false);
    }
    match m.resolve(spec) {
        Ok(v) => (v.clone(), m.is_installed(env, &v)),
        Err(_) => (format!("{spec} (unresolved)"), false),
    }
}

fn detect_pm_version(pm: &str) -> String {
    match std::process::Command::new(pm).arg("--version").output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_tool_unpinned() {
        let m = RustManager::new();
        let env = crate::environment::Environment::new(std::path::PathBuf::from("/tmp/none"));
        let (v, inst) = resolve_tool(&m, &env, "");
        assert_eq!(v, "not pinned");
        assert!(!inst);
    }
}
