//! Anchor CLI manager.
//!
//! Anchor provides prebuilt CLI binaries and a version manager (`avm`). For
//! a project-local, non-global install we download the official prebuilt
//! binary from `solana-foundation/anchor` Releases, verify its SHA-256 against
//! the official digest published by GitHub Releases, and place it under
//! `.solenv/versions/anchor/<ver>/bin/anchor`.
//!
//! This gives the same result as `avm install <ver>` but keeps the toolchain
//! out of the user's global `~/.avm`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::download;
use crate::environment::Environment;
use crate::managers::{default_client, Manager};
use crate::platform;
use crate::process::{path_prepend, run_scoped};

const ANCHOR_REPO: &str = "solana-foundation/anchor";
const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug)]
pub struct AnchorManager;

impl Default for AnchorManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AnchorManager {
    pub fn new() -> Self {
        AnchorManager
    }

    fn asset_name(version: &str) -> String {
        // e.g. anchor-1.1.2-aarch64-apple-darwin
        let triple = platform::host_triple().expect("unsupported platform");
        format!("anchor-{version}-{triple}")
    }

    fn release_url(version: &str) -> String {
        format!("{GITHUB_API}/repos/{ANCHOR_REPO}/releases/tags/v{version}")
    }

    fn download_url(version: &str, asset: &str) -> String {
        format!("https://github.com/{ANCHOR_REPO}/releases/download/v{version}/{asset}")
    }

    fn official_checksum(version: &str, asset: &str) -> Result<String> {
        let client = default_client();
        let url = Self::release_url(version);
        let resp = client
            .get(&url)
            .send()
            .with_context(|| format!("failed to query GitHub release {url}"))?;
        if !resp.status().is_success() {
            bail!(
                "release v{version} not found on {ANCHOR_REPO} (HTTP {}). Some Anchor versions predate prebuilt binaries; build from source if needed.",
                resp.status()
            );
        }
        let release: Release = resp
            .json()
            .context("failed to parse GitHub release metadata")?;
        release
            .assets
            .iter()
            .find(|a| a.name == asset)
            .and_then(|a| a.digest.strip_prefix("sha256:").map(|s| s.to_string()))
            .with_context(|| format!("no official checksum found for asset {asset}"))
    }

    fn binary_path(env: &Environment, version: &str) -> PathBuf {
        env.tool_bin_dir("anchor", version).join("anchor")
    }

    fn download_path(env: &Environment, version: &str) -> PathBuf {
        env.downloads_dir()
            .join("anchor")
            .join(format!("{version}.bin"))
    }
}

impl Manager for AnchorManager {
    fn name(&self) -> &'static str {
        "anchor"
    }
    fn label(&self) -> &'static str {
        "Anchor"
    }

    fn resolve(&self, spec: &str) -> Result<String> {
        crate::managers::resolve_github_version(ANCHOR_REPO, spec)
    }

    fn install(&self, env: &Environment, version: &str) -> Result<()> {
        if self.is_installed(env, version) {
            return Ok(());
        }
        env.ensure_dirs()?;
        let asset = Self::asset_name(version);
        let url = Self::download_url(version, &asset);
        let download = Self::download_path(env, version);
        let bin = Self::binary_path(env, version);
        let checksum = Self::official_checksum(version, &asset)?;

        let need_download = if download.exists() {
            !download::verify_sha256(&download, &checksum).unwrap_or(false)
        } else {
            true
        };
        if need_download {
            println!("  Downloading Anchor {} ...", version);
            let client = default_client();
            download::download_verified(&client, &url, &download, Some(&checksum))
                .with_context(|| "failed to download Anchor (network or checksum error)")?;
        }

        if let Some(parent) = bin.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::copy(&download, &bin)
            .with_context(|| format!("failed to copy Anchor into {}", bin.display()))?;
        make_executable(&bin)?;

        env.record_installed(self.name(), version, Some("bin".into()))?;
        Ok(())
    }

    fn is_installed(&self, env: &Environment, version: &str) -> bool {
        env.is_installed(self.name(), version)
    }

    fn resolve_bin_dir(&self, env: &Environment, version: &str) -> Result<PathBuf> {
        let dir = env.tool_bin_dir(self.name(), version);
        if !dir.join("anchor").exists() {
            bail!(
                "Anchor {} is not installed in this environment. Run `solenv install`.",
                version
            );
        }
        Ok(dir)
    }

    fn run(
        &self,
        env: &Environment,
        version: &str,
        args: &[String],
        base_env: &std::collections::BTreeMap<String, String>,
    ) -> Result<i32> {
        if args.is_empty() {
            bail!("no command given for anchor");
        }
        let bin = self.resolve_bin_dir(env, version)?;
        let program = bin.join(&args[0]);
        let mut extra_env = base_env.clone();
        if let Ok(existing) = std::env::var("PATH") {
            extra_env.insert("PATH".into(), path_prepend(&bin, Some(&existing)));
        } else {
            extra_env.insert("PATH".into(), bin.display().to_string());
        }
        let dirs = vec![bin];
        run_scoped(&program, args, &dirs, &extra_env)
    }
}

fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    digest: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_name_shape() {
        let name = AnchorManager::asset_name("1.1.2");
        assert!(name.starts_with("anchor-1.1.2-"));
    }

    #[test]
    fn download_url_shape() {
        let url = AnchorManager::download_url("1.1.2", "anchor-1.1.2-aarch64-apple-darwin");
        assert_eq!(
            url,
            "https://github.com/solana-foundation/anchor/releases/download/v1.1.2/anchor-1.1.2-aarch64-apple-darwin"
        );
    }
}
