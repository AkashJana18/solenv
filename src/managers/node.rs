//! Node.js manager.
//!
//! Downloads the official Node.js distribution from `nodejs.org`, verifies it
//! against the official `SHASUMS256.txt`, and extracts it project-locally.
//! Version specs like `24` (major-only) are resolved to the latest matching
//! patch release using the official dist index.
//!
//! Package managers (pnpm/yarn/bun) are handled separately; `npm` ships with
//! Node itself.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::download;
use crate::environment::Environment;
use crate::managers::{default_client, Manager};
use crate::platform;
use crate::process::{path_prepend, run_scoped};
use crate::version::Spec;

const NODE_DIST: &str = "https://nodejs.org/dist";

#[derive(Debug)]
pub struct NodeManager;

impl Default for NodeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeManager {
    pub fn new() -> Self {
        NodeManager
    }

    /// Resolve a version spec (may be major-only or channel) to a concrete
    /// published patch version, e.g. "24" -> "24.13.0".
    pub fn resolve_version(spec: &str) -> Result<String> {
        let parsed: Spec = spec
            .parse()
            .with_context(|| format!("invalid node version spec {spec:?}"))?;
        if parsed.channel.as_deref() == Some("latest")
            || parsed.channel.as_deref() == Some("stable")
        {
            return fetch_latest_major(None);
        }
        // major-only (e.g. "24") or wildcard -> resolve to latest patch of that major
        if parsed.wildcard != crate::version::Wildcard::None || parsed.to_semver().is_none() {
            return fetch_latest_major(Some(parsed.major));
        }
        // exact patch version
        Ok(format!(
            "{}.{}.{}",
            parsed.major, parsed.minor, parsed.patch
        ))
    }

    fn asset_dir_name(version: &str) -> Result<String> {
        let (os, arch) = platform::node_asset()?;
        Ok(format!("node-v{version}-{os}-{arch}"))
    }

    fn tarball_url(version: &str) -> Result<String> {
        let base = Self::asset_dir_name(version)?;
        Ok(format!("{NODE_DIST}/v{version}/{base}.tar.xz"))
    }

    fn shasums_url(version: &str) -> String {
        format!("{NODE_DIST}/v{version}/SHASUMS256.txt")
    }

    /// Fetch the official SHA-256 for the tarball of `version` from
    /// SHASUMS256.txt. Aggressively guarded: on any fetch/parse error we bail
    /// so we never install an unverified artifact.
    fn official_checksum(version: &str) -> Result<String> {
        let base = Self::asset_dir_name(version)?;
        let target = format!("{base}.tar.xz");
        let url = Self::shasums_url(version);
        let client = default_client();
        let body = client
            .get(&url)
            .send()
            .with_context(|| format!("failed to fetch official checksums {url}"))?
            .error_for_status()
            .with_context(|| format!("checksums not found for node {version}"))?
            .text()
            .context("failed to read checksums body")?;
        for line in body.lines() {
            let mut parts = line.split_whitespace();
            let hash = parts.next().unwrap_or("");
            let file = parts.next().unwrap_or("");
            if file == target {
                if hash.len() != 64 {
                    bail!("malformed official checksum for {target}");
                }
                return Ok(hash.to_string());
            }
        }
        bail!("official checksum for {target} not present in SHASUMS256.txt")
    }

    fn archive_path(env: &Environment, version: &str) -> PathBuf {
        env.downloads_dir()
            .join("node")
            .join(format!("{version}.tar.xz"))
    }
}

/// Fetch the latest published patch for a Node major line (or latest overall
/// if None) using the official dist index.
fn fetch_latest_major(major: Option<u64>) -> Result<String> {
    let index_url = format!("{NODE_DIST}/index.json");
    let client = default_client();
    let versions: Vec<DistVersion> = client
        .get(&index_url)
        .send()
        .with_context(|| format!("failed to fetch {index_url}"))?
        .error_for_status()
        .with_context(|| "failed to fetch node dist index")?
        .json()
        .context("failed to parse node dist index")?;
    // Dist index is ordered newest-first.
    for v in &versions {
        let parsed: Spec = match v.version.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        match major {
            Some(m) if parsed.major == m => {
                return Ok(format!(
                    "{}.{}.{}",
                    parsed.major, parsed.minor, parsed.patch
                ))
            }
            Some(_) => continue,
            None => {
                return Ok(format!(
                    "{}.{}.{}",
                    parsed.major, parsed.minor, parsed.patch
                ))
            }
        }
    }
    bail!("no node.js release found for major {major:?}")
}

#[derive(Debug, Deserialize)]
struct DistVersion {
    version: String,
}

impl Manager for NodeManager {
    fn name(&self) -> &'static str {
        "node"
    }
    fn label(&self) -> &'static str {
        "Node"
    }

    fn resolve(&self, spec: &str) -> Result<String> {
        Self::resolve_version(spec)
    }

    fn install(&self, env: &Environment, version_spec: &str) -> Result<()> {
        let version = Self::resolve_version(version_spec)?;
        if self.is_installed(env, &version) {
            // Still record under the spec for idempotency.
            env.record_installed(self.name(), &version, Some("bin".into()))?;
            return Ok(());
        }
        env.ensure_dirs()?;
        let url = Self::tarball_url(&version)?;
        let checksum = Self::official_checksum(&version)?;
        let archive = Self::archive_path(env, &version);
        let dest = env.tool_version_dir(self.name(), &version);

        let need_download = if archive.exists() {
            !download::verify_sha256(&archive, &checksum).unwrap_or(false)
        } else {
            true
        };
        if need_download {
            println!("  Downloading Node {} ...", version);
            let client = default_client();
            download::download_verified(&client, &url, &archive, Some(&checksum))
                .with_context(|| "failed to download Node (network or checksum error)")?;
        }

        if !dest.join("bin").exists() {
            println!("  Extracting Node {} ...", version);
            let tmp = dest.with_extension("extract");
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp)
                .with_context(|| format!("failed to create {}", tmp.display()))?;
            download::extract_tar_xz(&archive, &tmp)?;
            let inner = tmp.join(Self::asset_dir_name(&version)?);
            let _ = std::fs::remove_dir_all(&dest);
            if inner.exists() {
                std::fs::rename(&inner, &dest)
                    .with_context(|| format!("failed to move {}", inner.display()))?;
            } else {
                std::fs::rename(&tmp, &dest)?;
            }
            let _ = std::fs::remove_dir_all(&tmp);
        }

        env.record_installed(self.name(), &version, Some("bin".into()))?;
        Ok(())
    }

    fn is_installed(&self, env: &Environment, version_spec: &str) -> bool {
        let version =
            Self::resolve_version(version_spec).unwrap_or_else(|_| version_spec.to_string());
        env.is_installed(self.name(), &version)
    }

    fn resolve_bin_dir(&self, env: &Environment, version_spec: &str) -> Result<PathBuf> {
        let version = Self::resolve_version(version_spec)?;
        let dir = env.tool_bin_dir(self.name(), &version);
        if !dir.join("node").exists() {
            bail!(
                "Node {} is not installed in this environment. Run `solenv install`.",
                version
            );
        }
        Ok(dir)
    }

    fn run(
        &self,
        env: &Environment,
        version_spec: &str,
        args: &[String],
        base_env: &std::collections::BTreeMap<String, String>,
    ) -> Result<i32> {
        if args.is_empty() {
            bail!("no command given for node");
        }
        let bin = self.resolve_bin_dir(env, version_spec)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tarball_url_shape() {
        let url = NodeManager::tarball_url("24.13.0").unwrap();
        assert!(url.contains("/v24.13.0/"));
        assert!(url.ends_with(".tar.xz"));
        // Contains darwin/linux and x64/arm64
        assert!(url.contains("-darwin-") || url.contains("-linux-"));
    }

    #[test]
    fn shasums_url_shape() {
        assert_eq!(
            NodeManager::shasums_url("24.13.0"),
            "https://nodejs.org/dist/v24.13.0/SHASUMS256.txt"
        );
    }

    #[test]
    fn resolve_exact_version() {
        let v = NodeManager::resolve_version("24.13.0").unwrap();
        assert_eq!(v, "24.13.0");
    }

    #[test]
    fn official_checksum_parsing_logic() {
        // simulate parsing of SHASUMS lines
        let body =
            "abc123  node-v24.13.0-darwin-arm64.tar.xz\ndef456  node-v24.13.0-darwin-x64.tar.xz\n";
        let target = "node-v24.13.0-darwin-arm64.tar.xz";
        let found = body.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            let h = parts.next()?;
            let f = parts.next()?;
            (f == target).then(|| h.to_string())
        });
        assert_eq!(found.as_deref(), Some("abc123"));
    }
}
