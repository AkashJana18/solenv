//! Toolchain managers. Each manager knows how to install, cache and resolve a
//! single tool into a project-local environment while orchestrating existing
//! tooling where practical (rustup for Rust; official release downloads for
//! Solana/Anchor/Node, mirroring what avm/the Agave installer do).

pub mod anchor;
pub mod node;
pub mod rust;
pub mod solana;

use anyhow::{bail, Context, Result};
use std::fmt;
use std::path::PathBuf;

use crate::environment::Environment;
use crate::version::{Spec, Wildcard};

/// Common interface for a tool manager.
pub trait Manager: fmt::Debug {
    /// Short key, e.g. "rust", "solana", "anchor", "node".
    fn name(&self) -> &'static str;

    /// A friendly display name, e.g. "Anchor".
    fn label(&self) -> &'static str;

    /// Resolve a version spec (which may be exact, major-only, or a wildcard
    /// such as `3.1.x`) to a concrete, installable version string.
    ///
    /// The default only accepts exact patch versions; tools override this to
    /// resolve partial/wildcard pins (e.g. `3.0.x` -> `3.0.2`) by querying
    /// their version source.
    fn resolve(&self, spec: &str) -> Result<String> {
        let req: Spec = spec
            .parse()
            .with_context(|| format!("invalid {} version spec {spec:?}", self.label()))?;
        if req.channel.is_some() || req.wildcard != Wildcard::None {
            bail!(
                "cannot resolve {} spec {spec:?}; use an exact version",
                self.label()
            );
        }
        Ok(req
            .to_semver()
            .map(|v| v.to_string())
            .unwrap_or_else(|| spec.to_string()))
    }

    /// Install `version` into `env`. Must be idempotent: if already installed,
    /// do nothing. Must be resumable (an interrupted install can be retried).
    fn install(&self, env: &Environment, version: &str) -> Result<()>;

    /// Whether `version` is already usable in `env`.
    fn is_installed(&self, env: &Environment, version: &str) -> bool;

    /// The bin directory to place on PATH for `version`, if any.
    fn resolve_bin_dir(&self, env: &Environment, version: &str) -> Result<PathBuf>;

    /// Run `args` using this tool's pinned version. Default implementation
    /// prepends the resolved bin dir to PATH and runs the command there.
    fn run(
        &self,
        env: &Environment,
        version: &str,
        args: &[String],
        base_env: &std::collections::BTreeMap<String, String>,
    ) -> Result<i32>;
}

/// Convenience accessor shared by managers.
pub fn default_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        // HTTPS-only: no plaintext allows redirects to http.
        .https_only(true)
        .user_agent(concat!("solenv/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build HTTP client")
}

/// Resolve a version requirement against a GitHub repo's published releases,
/// returning the highest concrete version tag that satisfies `spec`.
///
/// Exact patch versions return immediately (no network); partial/wildcard
/// specs (e.g. `3.0.x`, `22`) are matched against the repo's release tags and
/// the highest satisfying tag is returned.
pub fn resolve_github_version(repo: &str, spec: &str) -> Result<String> {
    let req: Spec = spec
        .parse()
        .with_context(|| format!("invalid version spec {spec:?}"))?;
    // Exact patch -> no network needed.
    if req.channel.is_none() && req.wildcard == Wildcard::None && req.to_semver().is_some() {
        return Ok(req.to_semver().unwrap().to_string());
    }

    let mut tags: Vec<String> = Vec::new();
    let client = default_client();
    let mut page: u64 = 1;
    loop {
        let url = format!("https://api.github.com/repos/{repo}/releases?per_page=100&page={page}");
        let resp = client
            .get(&url)
            .send()
            .with_context(|| format!("failed to query GitHub releases for {repo}"))?;
        if !resp.status().is_success() {
            bail!(
                "failed to list releases for {repo} (HTTP {}). Check the repository and network.",
                resp.status()
            );
        }
        let releases: Vec<GithubRelease> = resp.json().context("failed to parse releases")?;
        if releases.is_empty() {
            break;
        }
        tags.extend(releases.iter().map(|r| r.tag_name.clone()));
        if releases.len() < 100 {
            break;
        }
        page += 1;
    }

    best_matching_tag(&req, tags.iter().map(|s| s.as_str())).with_context(|| {
        format!("no published release for {repo} satisfies {spec:?} (tried latest releases)")
    })
}

/// Given release tags and a version requirement, return the highest concrete
/// semantic version tag that satisfies `req`, ignoring channels/prereleases.
fn best_matching_tag<'a>(req: &Spec, tags: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<Spec> = None;
    let mut best_str: Option<String> = None;
    for raw in tags {
        let tag = raw.strip_prefix('v').unwrap_or(raw);
        let Ok(cand) = tag.parse::<Spec>() else {
            continue;
        };
        if cand.channel.is_some() {
            continue;
        }
        if req.matches(&cand) {
            let is_better = match &best {
                None => true,
                Some(b) => crate::version::compare(&cand, b) == std::cmp::Ordering::Greater,
            };
            if is_better {
                best = Some(cand.clone());
                best_str = Some(cand.to_string());
            }
        }
    }
    best_str
}

#[derive(Debug, serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(s: &str) -> Spec {
        s.parse().unwrap()
    }

    #[test]
    fn best_matching_picks_highest_patch_for_wildcard() {
        let tags = [
            "v3.0.0",
            "v3.0.1",
            "v3.0.2",
            "v3.1.0",
            "v2.1.9",
            "v3.0.1-rc.1",
        ];
        assert_eq!(
            best_matching_tag(&req("3.0.x"), tags.iter().copied()),
            Some("3.0.2".to_string())
        );
    }

    #[test]
    fn best_matching_major_wildcard() {
        let tags = ["v3.0.0", "v3.2.1", "v4.0.0", "v3.9.9"];
        assert_eq!(
            best_matching_tag(&req("3.x"), tags.iter().copied()),
            Some("3.9.9".to_string())
        );
    }

    #[test]
    fn best_matching_exact() {
        let tags = ["v0.32.1", "v0.32.2"];
        assert_eq!(
            best_matching_tag(&req("0.32.1"), tags.iter().copied()),
            Some("0.32.1".to_string())
        );
    }

    #[test]
    fn best_matching_no_match() {
        let tags = ["v4.0.0", "v3.2.1"];
        assert_eq!(
            best_matching_tag(&req("0.33.x"), tags.iter().copied()),
            None
        );
    }

    #[test]
    fn best_matching_ignores_prereleases_and_channels() {
        let tags = ["v0.32.0", "v0.32.1", "v0.32.1-rc.1", "stable"];
        assert_eq!(
            best_matching_tag(&req("0.32.x"), tags.iter().copied()),
            Some("0.32.1".to_string())
        );
    }
}
