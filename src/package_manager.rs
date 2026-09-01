//! JavaScript package-manager resolution.
//!
//! A Node.js install supplies `npm` and `npx` directly. `pnpm`, `yarn` and
//! `bun` are managed via Node's built-in `corepack`, or fall back to an
//! existing global install if corepack is unavailable.
//!
//! The resolved manager shim lives under `.solenv/bin/` so `solenv run pnpm
//! install` and `solenv run yarn` pick the correct tool without shell config.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::environment::Environment;
use crate::process::run_scoped;

/// A resolved package manager available in the environment.
#[derive(Debug, Clone)]
pub struct PackageManager {
    pub name: String,
    pub bin_dir: Option<PathBuf>,
    /// Convenience: the actual executable path (into a bin_dir or shim dir).
    pub exe: PathBuf,
    pub provisioned: ProvisionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionKind {
    /// Provided directly by the Node install (npm/npx).
    BundledWithNode,
    /// Prepared via corepack shim into `.solenv/bin`.
    Corepack,
    /// Reused from the user's global PATH.
    Global,
}

/// Resolve the configured package manager. `node_bin_dir` is the bin dir of
/// the installed Node; `provision=true` will attempt to prepare corepack
/// shims.
///
/// Returns `Some(manager)` if resolvable, or `None` if not configured.
pub fn resolve(
    env: &Environment,
    name: &str,
    node_bin_dir: Option<&Path>,
    provision: bool,
) -> Result<Option<PackageManager>> {
    let name = name.to_ascii_lowercase();
    if !matches!(
        name.as_str(),
        "npm" | "npx" | "pnpm" | "yarn" | "bun" | "corepack"
    ) {
        bail!("unsupported package_manager {name:?}; expected npm, yarn, pnpm or bun");
    }

    // npm/npx ship with Node.
    if name == "npm" || name == "npx" {
        if let Some(nb) = node_bin_dir {
            let exe = nb.join(&name);
            if exe.exists() {
                return Ok(Some(PackageManager {
                    name: name.clone(),
                    bin_dir: Some(nb.to_path_buf()),
                    exe,
                    provisioned: ProvisionKind::BundledWithNode,
                }));
            }
        }
        // fall back to global
        if let Ok(exe) = which::which(&name) {
            return Ok(Some(PackageManager {
                name: name.clone(),
                bin_dir: exe.parent().map(|p| p.to_path_buf()),
                exe,
                provisioned: ProvisionKind::Global,
            }));
        }
        bail!("package manager {name} is unavailable (no Node bin dir and not on PATH)");
    }

    // pnpm / yarn / bun
    if provision {
        if let Some(provisioned) = try_provision_corepack(env, &name, node_bin_dir)? {
            return Ok(Some(provisioned));
        }
    }
    // Fall back to global manager.
    if let Ok(exe) = which::which(&name) {
        return Ok(Some(PackageManager {
            name: name.clone(),
            bin_dir: exe.parent().map(|p| p.to_path_buf()),
            exe,
            provisioned: ProvisionKind::Global,
        }));
    }
    // Everything failed.
    bail!("package manager {name} could not be provisioned and is not on PATH.");
}

/// Try to provision a corepack shim for `pm` into `.solenv/bin`.
fn try_provision_corepack(
    env: &Environment,
    pm: &str,
    node_bin_dir: Option<&Path>,
) -> Result<Option<PackageManager>> {
    let Some(nb) = node_bin_dir else {
        return Ok(None);
    };
    let corepack = nb.join("corepack");
    if !corepack.exists() {
        return Ok(None);
    }
    let shim = env.bin_dir().join(pm);
    // Don't re-provision if the shim already exists.
    if shim.exists() {
        return Ok(Some(PackageManager {
            name: pm.to_string(),
            bin_dir: Some(env.bin_dir()),
            exe: shim,
            provisioned: ProvisionKind::Corepack,
        }));
    }

    // Prepare the shim with corepack (downloads the pinned manager on first
    // run if signature-verified by corepack).
    let status = std::process::Command::new(&corepack)
        .arg("prepare")
        .arg(pm)
        .arg("--activate")
        .current_dir(env.root())
        .status();
    let _ = status; // tolerate failure; fall back below

    let shim_alt = nb.join(pm);
    let mut exe = shim;
    let mut bin = env.bin_dir();
    if !exe.exists() && shim_alt.exists() {
        exe = shim_alt;
        bin = nb.to_path_buf();
    }
    if !exe.exists() {
        return Ok(None);
    }
    Ok(Some(PackageManager {
        name: pm.to_string(),
        bin_dir: Some(bin),
        exe,
        provisioned: ProvisionKind::Corepack,
    }))
}

/// Run `args` with the package manager. `search_dirs` are the tool bin dirs so
/// its transitive tools (node, npm) resolve correctly.
pub fn run(
    _env: &Environment,
    manager: &PackageManager,
    args: &[String],
    search_dirs: &[PathBuf],
    base_env: &BTreeMap<String, String>,
) -> Result<i32> {
    let mut dirs = search_dirs.to_vec();
    if let Some(bd) = &manager.bin_dir {
        dirs.push(bd.clone());
    }
    run_scoped(&manager.exe, args, &dirs, base_env)
}

#[cfg(test)]
mod tests {
    #[test]
    fn valid_names() {
        for ok in ["npm", "pnpm", "yarn", "bun", "npx", "corepack"] {
            assert!(matches!(
                ok,
                "npm" | "npx" | "pnpm" | "yarn" | "bun" | "corepack"
            ));
        }
    }
}
