//! `solenv run`: execute a command with the project's pinned toolchain.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{bail, Result};

use super::context::{env_for, require_config, resolve_root};
use crate::cli::Cli;
use crate::environment::Environment;
use crate::managers::anchor::AnchorManager;
use crate::managers::node::NodeManager;
use crate::managers::rust::RustManager;
use crate::managers::solana::SolanaManager;
use crate::managers::Manager;
use crate::package_manager;
use crate::process::{path_prepend_many, resolve_in_dirs, run_scoped};

pub fn run(cli: &Cli, command: &[String]) -> Result<i32> {
    if command.is_empty() {
        bail!("`solenv run` requires a command, e.g. `solenv run anchor build`");
    }
    let root = resolve_root(cli)?;
    let cfg = require_config(&root)?;
    let env = env_for(&root);
    let tc = cfg.toolchain.clone().unwrap_or_default();

    let search_dirs = build_search_dirs(&env, &tc)?;
    let extra_env = build_extra_env(&tc, &search_dirs);

    let resolver = |c: &str| resolve_in_dirs(c.as_ref(), &search_dirs);

    // Special-case package managers so shims (corepack) are used.
    let pm_name = tc.package_manager.clone().unwrap_or_default();
    let cmd0 = command[0].clone();
    let is_pm = matches!(pm_name.as_str(), "pnpm" | "yarn" | "bun" | "npm")
        && matches!(cmd0.as_str(), "pnpm" | "yarn" | "bun" | "npm" | "npx");

    if is_pm {
        let node_bin = find_node_bin(&env, &tc);
        let resolved = package_manager::resolve(&env, &cmd0, node_bin.as_deref(), true)?;
        match resolved {
            Some(m) => {
                let dirs = {
                    let mut d = search_dirs.clone();
                    if let Some(bd) = &m.bin_dir {
                        d.push(bd.clone());
                    }
                    d
                };
                return package_manager::run(&env, &m, &command[1..], &dirs, &extra_env);
            }
            None => {
                // fall through to generic resolution
            }
        }
    }

    // Generic run.
    let program = match resolver(&cmd0) {
        Some(p) => p,
        None => cmd0.clone().into(),
    };
    run_scoped(&program, command, &search_dirs, &extra_env)
}

/// Build the ordered list of bin dirs for all pinned, installed tools. If a
/// pinned tool is missing, error with an actionable message.
fn build_search_dirs(env: &Environment, tc: &crate::config::Toolchain) -> Result<Vec<PathBuf>> {
    let rust = RustManager::new();
    let solana = SolanaManager::new();
    let anchor = AnchorManager::new();
    let node = NodeManager::new();

    let mut dirs: Vec<PathBuf> = Vec::new();

    // Rust first so rustc/cargo resolve to the pinned toolchain.
    if let Some(v) = &tc.rust {
        let vr = rust.resolve(v)?;
        if rust.is_installed(env, &vr) {
            dirs.push(rust.resolve_bin_dir(env, &vr).unwrap_or_default());
        } else if rust.resolve_bin_dir(env, &vr).is_ok() {
            dirs.push(rust.resolve_bin_dir(env, &vr).unwrap());
        } else {
            bail!(
                "Rust toolchain {} is not installed. Run `solenv install` first.",
                v
            );
        }
    }

    if let Some(v) = &tc.solana {
        let vr = solana.resolve(v)?;
        if !solana.is_installed(env, &vr) {
            bail!(
                "Solana/Agave {} is not installed. Run `solenv install` first.",
                v
            );
        }
        dirs.push(solana.resolve_bin_dir(env, &vr)?);
    }

    if let Some(v) = &tc.anchor {
        let vr = anchor.resolve(v)?;
        if !anchor.is_installed(env, &vr) {
            bail!("Anchor {} is not installed. Run `solenv install` first.", v);
        }
        dirs.push(anchor.resolve_bin_dir(env, &vr)?);
    }

    if let Some(v) = &tc.node {
        let vr = node.resolve(v)?;
        if !node.is_installed(env, &vr) {
            bail!("Node {} is not installed. Run `solenv install` first.", v);
        }
        dirs.push(node.resolve_bin_dir(env, &vr)?);
    }

    Ok(dirs)
}

/// Extra environment variables applied to every `solenv run` invocation.
fn build_extra_env(
    tc: &crate::config::Toolchain,
    search_dirs: &[PathBuf],
) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    // Pin rustup so cargo/rustc use the project toolchain even without a
    // rust-toolchain.toml.
    if let Some(rust) = &tc.rust {
        if !rust.is_empty() {
            m.insert("RUSTUP_TOOLCHAIN".to_string(), rust.clone());
        }
    }
    // Solana tooling expects the platform-tools to be discoverable; expose
    // SOLANA_* vars commonly leveraged by anchor/cargo-build-sbf.
    if !search_dirs.is_empty() {
        if let Ok(existing) = std::env::var("PATH") {
            let p = path_prepend_many(search_dirs, Some(&existing));
            m.insert("PATH".to_string(), p);
        } else {
            m.insert("PATH".to_string(), path_prepend_many(search_dirs, None));
        }
    }
    m
}

fn find_node_bin(env: &Environment, tc: &crate::config::Toolchain) -> Option<PathBuf> {
    let node = NodeManager::new();
    let v = tc.node.as_ref()?;
    let resolved = NodeManager::resolve_version(v).ok()?;
    node.resolve_bin_dir(env, &resolved).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_env_sets_path_and_rustup() {
        let tc = crate::config::Toolchain {
            rust: Some("1.92.0".into()),
            ..Default::default()
        };
        let dirs = vec![PathBuf::from("/env/bin")];
        let m = build_extra_env(&tc, &dirs);
        assert_eq!(
            m.get("RUSTUP_TOOLCHAIN").map(|s| s.as_str()),
            Some("1.92.0")
        );
        assert!(m
            .get("PATH")
            .map(|s| s.contains("/env/bin"))
            .unwrap_or(false));
    }
}
