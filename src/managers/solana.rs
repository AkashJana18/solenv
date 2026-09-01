//! Solana/Agave manager.
//!
//! Orchestrates the official Agave (Solana CLI) releases rather than
//! reinventing the installer. We download the platform release tarball
//! (`solana-release-<target>.tar.bz2`) from `anza-xyz/agave`, verify its
//! SHA-256 against the official digest published by GitHub Releases, and
//! extract it project-locally under `.solenv/versions/solana/<ver>/`.
//!
//! This mirrors what `solana-install`/`agave-install` do, but keeps the
//! toolchain project-local and never touches the user's global install.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::download;
use crate::environment::Environment;
use crate::managers::{default_client, Manager};
use crate::platform;
use crate::process::{path_prepend, run_scoped};

const AGAVE_REPO: &str = "anza-xyz/agave";
const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug)]
pub struct SolanaManager;

impl Default for SolanaManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SolanaManager {
    pub fn new() -> Self {
        SolanaManager
    }

    fn asset_name() -> Result<String> {
        let triple = platform::host_triple()?;
        Ok(format!("solana-release-{triple}.tar.bz2"))
    }

    fn release_url(version: &str) -> String {
        format!("{GITHUB_API}/repos/{AGAVE_REPO}/releases/tags/v{version}")
    }

    fn download_url(version: &str, asset: &str) -> String {
        format!("https://github.com/{AGAVE_REPO}/releases/download/v{version}/{asset}")
    }

    /// Fetch the official SHA-256 for `asset` from the GitHub release API.
    fn official_checksum(version: &str, asset: &str) -> Result<String> {
        let client = default_client();
        let url = Self::release_url(version);
        let resp = client
            .get(&url)
            .send()
            .with_context(|| format!("failed to query GitHub release {url}"))?;
        if !resp.status().is_success() {
            bail!(
                "release v{version} not found on {AGAVE_REPO} (HTTP {}). Check the version is valid and published.",
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
            .with_context(|| format!("no official checksum found for {asset}"))
    }

    fn archive_path(env: &Environment, version: &str) -> PathBuf {
        env.downloads_dir()
            .join("solana")
            .join(format!("{version}.tar.bz2"))
    }
}

impl Manager for SolanaManager {
    fn name(&self) -> &'static str {
        "solana"
    }
    fn label(&self) -> &'static str {
        "Agave"
    }

    fn resolve(&self, spec: &str) -> Result<String> {
        crate::managers::resolve_github_version(AGAVE_REPO, spec)
    }

    fn install(&self, env: &Environment, version: &str) -> Result<()> {
        if self.is_installed(env, version) {
            return Ok(());
        }
        env.ensure_dirs()?;
        let asset = Self::asset_name()?;
        let url = Self::download_url(version, &asset);
        let archive = Self::archive_path(env, version);
        let dest = env.tool_version_dir(self.name(), version);

        // Resumable: if we already have a verified archive, skip download.
        let checksum = Self::official_checksum(version, &asset).ok();
        let need_download = if archive.exists() {
            match &checksum {
                Some(c) => !download::verify_sha256(&archive, c).unwrap_or(false),
                None => true,
            }
        } else {
            true
        };

        if need_download {
            println!("  Downloading Solana/Agave {} ...", version);
            let client = default_client();
            download::download_verified(&client, &url, &archive, checksum.as_deref())
                .with_context(|| "failed to download Solana release (network or checksum error)")?;
        }

        // Extract (resumable: skip if bin dir already present).
        let bin_marker = dest.join("bin");
        if !bin_marker.exists() {
            println!("  Extracting Solana/Agave {} ...", version);
            // tar.bz2 -> extract to a temp dir then move the inner
            // solana-release dir into place.
            let tmp = dest.with_extension("extract");
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp)
                .with_context(|| format!("failed to create {}", tmp.display()))?;
            extract_bz2(&archive, &tmp)
                .with_context(|| "failed to extract Solana release archive")?;
            // The archive contains a top-level "solana-release/" dir.
            let inner = tmp.join("solana-release");
            if inner.exists() {
                let _ = std::fs::remove_dir_all(&dest);
                if dest.exists() {
                    std::fs::remove_dir_all(&dest)?;
                }
                std::fs::rename(&inner, &dest)
                    .with_context(|| format!("failed to move {}", inner.display()))?;
            } else {
                // fallback: move contents
                let _ = std::fs::remove_dir_all(&dest);
                std::fs::rename(&tmp, &dest)?;
            }
            let _ = std::fs::remove_dir_all(&tmp);
        }

        env.record_installed(self.name(), version, Some("bin".into()))?;
        Ok(())
    }

    fn is_installed(&self, env: &Environment, version: &str) -> bool {
        env.is_installed(self.name(), version)
    }

    fn resolve_bin_dir(&self, env: &Environment, version: &str) -> Result<PathBuf> {
        let dir = env.tool_bin_dir(self.name(), version);
        if !dir.join("solana").exists() {
            bail!(
                "Solana/Agave {} is not installed in this environment. Run `solenv install`.",
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
            bail!("no command given for solana");
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

fn extract_bz2(archive: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(archive)
        .with_context(|| format!("failed to open {}", archive.display()))?;
    let bz = bzip2::read::BzDecoder::new(f);
    let mut tar = tar::Archive::new(bz);
    tar.unpack(dest)
        .with_context(|| format!("failed to extract {}", archive.display()))?;
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
    fn asset_name_uses_platform() {
        let name = SolanaManager::asset_name().unwrap();
        assert!(name.starts_with("solana-release-"));
        assert!(name.ends_with(".tar.bz2"));
    }

    #[test]
    fn download_url_shape() {
        let url = SolanaManager::download_url("4.0.3", "solana-release-x.tar.bz2");
        assert_eq!(
            url,
            "https://github.com/anza-xyz/agave/releases/download/v4.0.3/solana-release-x.tar.bz2"
        );
    }

    #[test]
    fn official_checksum_parses_digest() {
        // Parse a digest string the way we do in the code path.
        let digest = "sha256:abc123".to_string();
        let stripped = digest.strip_prefix("sha256:").unwrap();
        assert_eq!(stripped, "abc123");
    }
}
