//! `solenv.toml` configuration parsing and Anchor.toml toolchain detection.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The project toolchain declared in `solenv.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct Toolchain {
    pub rust: Option<String>,
    pub solana: Option<String>,
    pub anchor: Option<String>,
    pub node: Option<String>,
    #[serde(rename = "package_manager")]
    pub package_manager: Option<String>,
}

/// Top-level `solenv.toml` document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SolenvConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<Toolchain>,
}

/// The toolchain block found in an existing `Anchor.toml`.
///
/// Source: https://www.anchor-lang.com/docs/references/anchor-toml
/// ```toml
/// [toolchain]
/// anchor_version = "..."
/// solana_version = "..."
/// package_manager = "..."
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AnchorToolchain {
    #[serde(rename = "anchor_version")]
    pub anchor_version: Option<String>,
    #[serde(rename = "solana_version")]
    pub solana_version: Option<String>,
    #[serde(rename = "package_manager")]
    pub package_manager: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AnchorToml {
    pub toolchain: Option<AnchorToolchain>,
}

impl SolenvConfig {
    /// Parse `solenv.toml` from a string.
    pub fn parse(s: &str) -> Result<Self> {
        let cfg: SolenvConfig =
            toml::from_str(s).context("failed to parse solenv.toml (is it valid TOML?)")?;
        Ok(cfg)
    }

    /// Serialize to TOML string.
    pub fn to_toml(&self) -> Result<String> {
        let s = toml::to_string_pretty(self).context("failed to serialize config")?;
        Ok(s)
    }
}

/// Load a config from a file path.
pub fn load(path: &Path) -> Result<SolenvConfig> {
    let s = std::fs::read_to_string(path).with_context(|| format!("cannot read {:?}", path))?;
    SolenvConfig::parse(&s).with_context(|| format!("in {:?}", path))
}

/// Parse an `Anchor.toml` (best-effort) and return its `[toolchain]` block.
pub fn parse_anchor_toolchain(s: &str) -> Result<Option<AnchorToolchain>> {
    let toml: AnchorToml = toml::from_str(s).context("failed to parse Anchor.toml")?;
    Ok(toml.toolchain)
}

/// Parse `Anchor.toml` at `path` (best-effort; returns None if unreadable or
/// no toolchain block).
pub fn anchor_toolchain_from_file(path: &Path) -> Option<AnchorToolchain> {
    let s = std::fs::read_to_string(path).ok()?;
    parse_anchor_toolchain(&s).ok()?
}

/// The default `solenv.toml` content, e.g. for `solenv init`.
pub fn default_config(versions: &Toolchain) -> SolenvConfig {
    SolenvConfig {
        toolchain: Some(versions.clone()),
    }
}

/// Create a `Toolchain` from a directory by detecting installed tool versions
/// and any Anchor.toml toolchain block. All fields are optional so callers can
/// decide which to include.
pub fn detect_from_dir(dir: &Path) -> Toolchain {
    let mut tc = Toolchain::default();

    // Anchor.toml toolchain block wins for anchor/solana when present.
    let anchor_path = dir.join("Anchor.toml");
    if let Some(atc) = anchor_toolchain_from_file(&anchor_path) {
        tc.anchor = atc.anchor_version.filter(|s| !s.is_empty());
        tc.solana = atc.solana_version.filter(|s| !s.is_empty());
        tc.package_manager = atc.package_manager.filter(|s| !s.is_empty());
    }

    // Fall back to detecting installed versions on the system.
    if tc.rust.is_none() {
        tc.rust = detect_installed_rust().or(tc.rust);
    }
    if tc.anchor.is_none() {
        tc.anchor = detect_installed_anchor().or(tc.anchor);
    }
    if tc.solana.is_none() {
        tc.solana = detect_installed_solana().or(tc.solana);
    }
    if tc.node.is_none() {
        tc.node = detect_installed_node().or(tc.node);
    }

    tc
}

/// Detect the active Rust toolchain version via `rustc --version`.
pub fn detect_installed_rust() -> Option<String> {
    let out = run_capture("rustc", &["--version"]).ok()?;
    parse_tool_version(&out, "rustc")
}

/// Detect the active Anchor CLI version via `anchor --version`.
pub fn detect_installed_anchor() -> Option<String> {
    let out = run_capture("anchor", &["--version"]).ok()?;
    parse_tool_version(&out, "anchor-cli").or_else(|| parse_any_semver(&out))
}

/// Detect the active Solana CLI version via `solana --version`.
pub fn detect_installed_solana() -> Option<String> {
    let out = run_capture("solana", &["--version"]).ok()?;
    parse_any_semver(&out)
}

/// Detect the active Node version via `node --version`.
pub fn detect_installed_node() -> Option<String> {
    let out = run_capture("node", &["--version"]).ok()?;
    parse_any_semver(&out)
}

fn run_capture(bin: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(bin)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {bin}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Pull a version number out of a `rustc 1.92.0 (abc...)`-style string by
/// matching the tool name, e.g. `parse_tool_version("rustc 1.92.0", "rustc")`.
fn parse_tool_version(out: &str, tool: &str) -> Option<String> {
    let lower = out.to_ascii_lowercase();
    let idx = lower.find(tool)?;
    let rest = &out[idx + tool.len()..];
    parse_any_semver(rest)
}

/// Find the first dotted numeric version substring in `s`.
fn parse_any_semver(s: &str) -> Option<String> {
    let mut best: Option<String> = None;
    for tok in s.split_whitespace() {
        let t = tok.trim_start_matches(|c: char| c.is_ascii_punctuation() && c != '.');
        // Handle a leading "v" prefix (e.g. "v24.13.0" from `node --version`).
        let t = t
            .strip_prefix(['v', 'V'])
            .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
            .unwrap_or(t);
        // Match x.y.z (optionally with 4th part like 4.0.3.0 or pre-release)
        let head: String = t
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let parts: Vec<&str> = head.split('.').collect();
        if parts.len() >= 3 && parts.iter().all(|p| !p.is_empty()) {
            // Prefer the first match that has at least 3 numeric components.
            if let Some(existing) = &best {
                if existing.split('.').filter(|p| !p.is_empty()).count() >= 3 {
                    continue;
                }
            }
            best = Some(parts[..3].join("."));
        }
    }
    best
}

/// Find the project root: an ancestor containing `solenv.toml`, else the cwd.
pub fn project_root(start: &Path) -> PathBuf {
    let mut cur = Some(if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| start.to_path_buf())
    });
    while let Some(dir) = cur {
        if dir.join("solenv.toml").exists() {
            return dir;
        }
        cur = dir.parent().map(|p| p.to_path_buf());
    }
    std::env::current_dir().unwrap_or_else(|_| start.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_solenv_toml() {
        let s = r#"
[toolchain]
rust = "1.92.0"
solana = "4.0.0"
anchor = "1.1.2"
node = "24"
package_manager = "pnpm"
"#;
        let cfg = SolenvConfig::parse(s).unwrap();
        let tc = cfg.toolchain.unwrap();
        assert_eq!(tc.rust.as_deref(), Some("1.92.0"));
        assert_eq!(tc.solana.as_deref(), Some("4.0.0"));
        assert_eq!(tc.anchor.as_deref(), Some("1.1.2"));
        assert_eq!(tc.node.as_deref(), Some("24"));
        assert_eq!(tc.package_manager.as_deref(), Some("pnpm"));
    }

    #[test]
    fn parse_empty() {
        let cfg = SolenvConfig::parse("").unwrap();
        assert!(cfg.toolchain.is_none());
    }

    #[test]
    fn parse_solenv_roundtrip() {
        let cfg = SolenvConfig {
            toolchain: Some(Toolchain {
                rust: Some("1.92.0".into()),
                solana: None,
                anchor: Some("1.1.2".into()),
                node: None,
                package_manager: Some("pnpm".into()),
            }),
        };
        let s = cfg.to_toml().unwrap();
        let back = SolenvConfig::parse(&s).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn parse_anchor_toml_toolchain() {
        let s = r#"
[provider]
cluster = "localnet"

[toolchain]
anchor_version = "0.32.1"
solana_version = "2.1.7"
package_manager = "yarn"
"#;
        let tc = parse_anchor_toolchain(s).unwrap().unwrap();
        assert_eq!(tc.anchor_version.as_deref(), Some("0.32.1"));
        assert_eq!(tc.solana_version.as_deref(), Some("2.1.7"));
        assert_eq!(tc.package_manager.as_deref(), Some("yarn"));
    }

    #[test]
    fn anchor_toml_without_toolchain() {
        let s = "[provider]\ncluster = \"devnet\"\n";
        let tc = parse_anchor_toolchain(s).unwrap();
        assert!(tc.is_none());
    }

    #[test]
    fn parse_tool_version_detection() {
        assert_eq!(
            parse_tool_version("rustc 1.92.0 (abc 2026-01-01)", "rustc"),
            Some("1.92.0".into())
        );
        assert_eq!(
            parse_tool_version("anchor-cli 1.1.2", "anchor-cli"),
            Some("1.1.2".into())
        );
        assert_eq!(
            parse_any_semver("solana-cli 4.0.3 (src:abc)"),
            Some("4.0.3".into())
        );
        assert_eq!(parse_any_semver("v24.13.0"), Some("24.13.0".into()));
        assert!(parse_any_semver("no versions here").is_none());
    }
}
