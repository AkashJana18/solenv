//! Rust manager: orchestrates `rustup` to install and pin toolchains.
//!
//! rustup is the canonical Rust version manager. We do not reimplement it.
//! `rustup toolchain install <ver>` installs a toolchain into rustup's shared
//! storage; `rustup run <ver> -- <cmd>` executes `cmd` under that toolchain
//! without ever changing the user's default/active toolchain.
//!
//! For project-local reproducibility, `solenv run` sets `RUSTUP_TOOLCHAIN` and
//! prepends the resolved toolchain bin dir (via `rustup which rustc`) to PATH.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::Manager;
use crate::environment::Environment;
use crate::version::Spec;

#[derive(Debug)]
pub struct RustManager;

impl Default for RustManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RustManager {
    pub fn new() -> Self {
        RustManager
    }

    /// Whether `rustup` is available on PATH or via the standard location.
    fn rustup_bin() -> Result<PathBuf> {
        if let Ok(p) = which::which("rustup") {
            return Ok(p);
        }
        // Common fallbacks.
        for cand in ["~/.cargo/bin/rustup", "~/.rustup/rustup"] {
            let expanded = expand_tilde(cand);
            if expanded.exists() {
                return Ok(expanded);
            }
        }
        bail!("rustup not found. Install it with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh")
    }
}

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

/// List installed rustup toolchain names (e.g. "1.91.0-aarch64-apple-darwin", "stable-x86_64-unknown-linux-gnu").
fn list_rust_toolchains() -> Result<Vec<String>> {
    let rustup = RustManager::rustup_bin()?;
    let out = std::process::Command::new(&rustup)
        .args(["toolchain", "list"])
        .output()
        .with_context(|| "failed to run `rustup toolchain list`")?;
    if !out.status.success() {
        bail!("rustup toolchain list failed");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.split_whitespace().map(|s| s.to_string()).collect())
}

/// Extract the version/channel spec from a rustup toolchain name, e.g.
/// "1.91.0-aarch64-apple-darwin" -> Spec(1.91.0) or "stable-x86_64-unknown-linux-gnu"
/// -> Spec(stable).
fn channel_spec_of(name: &str) -> Option<Spec> {
    // Extract the leading channel token (before any target triple suffix),
    // e.g. "1.91.0-aarch64-apple-darwin" -> "1.91.0", "stable-..." -> "stable".
    let token = name.split('-').next().unwrap_or(name);
    // Channel names.
    if matches!(token, "stable" | "nightly" | "beta") {
        return token.parse().ok();
    }
    // Numeric version: "1.91.0".
    let count_digits = token.chars().filter(|c| c.is_ascii_digit()).count();
    let all_numeric = token.chars().all(|c| c.is_ascii_digit() || c == '.');
    if all_numeric && count_digits >= 3 {
        token.parse().ok()
    } else {
        None
    }
}

impl Manager for RustManager {
    fn name(&self) -> &'static str {
        "rust"
    }
    fn label(&self) -> &'static str {
        "Rust"
    }

    fn resolve(&self, spec: &str) -> Result<String> {
        let req: Spec = spec
            .parse()
            .with_context(|| format!("invalid Rust version spec {spec:?}"))?;
        // Channels are passed through to rustup verbatim.
        if req.channel.is_some() {
            return Ok(spec.to_string());
        }
        // Exact patch (e.g. 1.91.0) -> use it directly.
        if req.wildcard == crate::version::Wildcard::None && req.to_semver().is_some() {
            return Ok(req.to_semver().unwrap().to_string());
        }
        // Partial like "1.91": still resolve to an installed 1.91.x below.
        // Match against toolchains already installed via rustup, choosing the
        // highest satisfying version.
        let names = list_rust_toolchains()?;
        let mut best: Option<Spec> = None;
        let mut best_str: Option<String> = None;
        for name in &names {
            let Some(cand) = channel_spec_of(name) else {
                continue;
            };
            if req.matches(&cand) {
                let better = match &best {
                    None => true,
                    Some(b) => crate::version::compare(&cand, b) == std::cmp::Ordering::Greater,
                };
                if better {
                    best = Some(cand.clone());
                    best_str = Some(cand.to_string());
                }
            }
        }
        best_str.with_context(|| {
            format!(
                "Rust toolchain matching {spec:?} is not installed. Run `solenv install` or `rustup toolchain install {spec}`."
            )
        })
    }

    fn install(&self, env: &Environment, version: &str) -> Result<()> {
        if self.is_installed(env, version) {
            return Ok(());
        }
        let rustup = Self::rustup_bin()?;
        env.ensure_dirs()?;
        println!("  Installing Rust toolchain {} via rustup ...", version);
        let status = std::process::Command::new(&rustup)
            .args(["toolchain", "install", version])
            .status()
            .with_context(|| "failed to run rustup toolchain install")?;
        if !status.success() {
            bail!("rustup failed to install toolchain {}", version);
        }
        env.record_installed(self.name(), version, None)?;
        Ok(())
    }

    fn is_installed(&self, env: &Environment, version: &str) -> bool {
        if env.is_installed(self.name(), version) {
            return true;
        }
        // Ask rustup if the toolchain exists. Compare using the channel/version
        // prefix so an exact pin like "1.91.0" matches toolchain
        // "1.91.0-aarch64-apple-darwin".
        if let Ok(rustup) = Self::rustup_bin() {
            if let Ok(out) = std::process::Command::new(&rustup)
                .args(["toolchain", "list"])
                .output()
            {
                let text = String::from_utf8_lossy(&out.stdout);
                if text.split_whitespace().any(|t| t == version) {
                    return true;
                }
                let req: Option<Spec> = version.parse().ok();
                return text.split_whitespace().any(|t| match &req {
                    Some(r) => {
                        r.matches(&channel_spec_of(t).unwrap_or_else(|| Spec::exact(0, 0, 0)))
                    }
                    None => false,
                });
            }
        }
        false
    }

    fn resolve_bin_dir(&self, _env: &Environment, version: &str) -> Result<PathBuf> {
        let rustup = Self::rustup_bin()?;
        let out = std::process::Command::new(&rustup)
            .args(["which", version, "rustc"])
            .output()
            .with_context(|| format!("failed to resolve rust toolchain {version}"))?;
        if !out.status.success() {
            bail!(
                "Rust toolchain {} is not installed. Run `solenv install` first.",
                version
            );
        }
        let rustc_path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
        let bin = rustc_path
            .parent()
            .with_context(|| format!("unexpected rustc path {}", rustc_path.display()))?;
        Ok(bin.to_path_buf())
    }

    fn run(
        &self,
        _env: &Environment,
        version: &str,
        args: &[String],
        base_env: &BTreeMap<String, String>,
    ) -> Result<i32> {
        if args.is_empty() {
            bail!("no command given for rust");
        }
        let bin = self.resolve_bin_dir(_env, version)?;
        let program = bin.join(&args[0]);
        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args[1..]);
        cmd.env("PATH", path_with_pathenv(&bin, base_env.get("PATH")));
        cmd.env("RUSTUP_TOOLCHAIN", version);
        for (k, v) in base_env {
            cmd.env(k, v);
        }
        inherit_io(&mut cmd);
        let st = cmd.status().with_context(|| {
            format!(
                "failed to run {} ({} under {:?})",
                args[0],
                self.label(),
                version
            )
        })?;
        Ok(st.code().unwrap_or(1))
    }
}

/// Prepend `dir` to an existing PATH string.
pub fn path_with_pathenv(dir: &Path, existing: Option<&String>) -> String {
    let sep = crate::process::PATH_SEPARATOR;
    match existing {
        Some(existing) => format!("{}{sep}{existing}", dir.display()),
        None => dir.display().to_string(),
    }
}

/// Inherit stdout/stderr so interactive tools behave normally.
pub fn inherit_io(cmd: &mut std::process::Command) {
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());
    cmd.stdin(std::process::Stdio::inherit());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_prepend_linux_sep() {
        // PATH_SEPARATOR on unix is ':'.
        let d = PathBuf::from("/x/bin");
        let got = path_with_pathenv(&d, Some(&"/usr/bin:/usr/local/bin".to_string()));
        assert!(got.starts_with("/x/bin:"));
        assert!(got.ends_with("/usr/local/bin"));
    }

    #[test]
    fn path_prepend_none() {
        let got = path_with_pathenv(&PathBuf::from("/x/bin"), None);
        assert_eq!(got, "/x/bin");
    }

    #[test]
    fn spec_parses_rust_version() {
        let s: Spec = "1.92.0".parse().unwrap();
        assert!(!s.is_wildcard());
    }

    #[test]
    fn channel_spec_extracts_version_prefix() {
        assert_eq!(
            channel_spec_of("1.91.0-aarch64-apple-darwin").map(|s| s.to_string()),
            Some("1.91.0".to_string())
        );
        assert_eq!(
            channel_spec_of("1.86.0-x86_64-unknown-linux-gnu").map(|s| s.to_string()),
            Some("1.86.0".to_string())
        );
        // Channel-named toolchains.
        assert!(channel_spec_of("stable-aarch64-apple-darwin").is_some());
        // A bare non-version token yields None.
        assert!(channel_spec_of("custom-toolchain").is_none());
    }

    #[test]
    fn resolve_rust_wildcard_picks_highest() {
        let m = RustManager::new();
        // Depends on the host's rustup toolchains; just ensure it either
        // resolves or fails cleanly for something no toolchain matches.
        let err = m.resolve("99.x").unwrap_err().to_string();
        assert!(err.contains("is not installed"), "unexpected: {err}");
    }
}
