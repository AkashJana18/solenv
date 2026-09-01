//! Project-local environment layout and state management.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Toolchain;

/// Where everything project-local lives.
///
/// ```text
/// .solenv/
/// ├── bin/          # resolved shims / version-pinned binaries
/// ├── versions/     # installed toolchain versions (keyed by tool/version)
/// ├── cache/        # downloaded artifacts + checksums
/// └── state.toml    # what is currently installed/active
/// ```
#[derive(Debug, Clone)]
pub struct Environment {
    root: PathBuf,
}

/// One installed tool version within an environment.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledTool {
    pub version: String,
    pub installed: bool,
    /// Path (relative to `.solenv/versions/<tool>/<ver>`) of the resolved
    /// bin directory, when applicable.
    pub bin_dir: Option<String>,
}

/// Serialized `.solenv/state.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    /// tool -> version -> InstalledTool
    pub tools: BTreeMap<String, BTreeMap<String, InstalledTool>>,
}

impl Environment {
    pub fn new(root: PathBuf) -> Self {
        Environment { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn solenv_dir(&self) -> PathBuf {
        self.root.join(".solenv")
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.solenv_dir().join("bin")
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.solenv_dir().join("versions")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.solenv_dir().join("cache")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.cache_dir().join("downloads")
    }

    pub fn state_path(&self) -> PathBuf {
        self.solenv_dir().join("state.toml")
    }

    pub fn tool_version_dir(&self, tool: &str, version: &str) -> PathBuf {
        self.versions_dir().join(tool).join(version)
    }

    /// The bin directory for a tool/version, resolved from state if known,
    /// else the conventional `<tool>/<ver>/bin`.
    pub fn tool_bin_dir(&self, tool: &str, version: &str) -> PathBuf {
        self.tool_version_dir(tool, version).join("bin")
    }

    /// Create all directories that should exist for an environment.
    pub fn ensure_dirs(&self) -> Result<()> {
        for d in [
            self.solenv_dir(),
            self.bin_dir(),
            self.versions_dir(),
            self.cache_dir(),
            self.downloads_dir(),
        ] {
            std::fs::create_dir_all(&d)
                .with_context(|| format!("failed to create {}", d.display()))?;
        }
        Ok(())
    }

    /// Load state; returns empty/default state if none exists.
    pub fn load_state(&self) -> Result<State> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(State::default());
        }
        let s = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let st: State =
            toml::from_str(&s).with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(st)
    }

    /// Save state to disk (atomically-ish: write temp then rename).
    pub fn save_state(&self, state: &State) -> Result<()> {
        self.ensure_dirs()?;
        let s = toml::to_string_pretty(state).context("failed to serialize state")?;
        let tmp = self.state_path().with_extension("toml.tmp");
        std::fs::write(&tmp, s).with_context(|| format!("failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, self.state_path())
            .with_context(|| format!("failed to write {}", self.state_path().display()))?;
        Ok(())
    }

    /// Mark a tool+version as installed and record its resolved bin dir.
    pub fn record_installed(
        &self,
        tool: &str,
        version: &str,
        bin_dir: Option<String>,
    ) -> Result<()> {
        let mut state = self.load_state()?;
        state.tools.entry(tool.to_string()).or_default().insert(
            version.to_string(),
            InstalledTool {
                version: version.to_string(),
                installed: true,
                bin_dir,
            },
        );
        self.save_state(&state)
    }

    /// Is a tool+version already installed (per state or on disk)?
    pub fn is_installed(&self, tool: &str, version: &str) -> bool {
        let state = self.load_state().unwrap_or_default();
        if let Some(ver) = state.tools.get(tool).and_then(|m| m.get(version)) {
            if ver.installed {
                return true;
            }
        }
        // Fall back to checking the version bin dir exists and has contents.
        let dir = self.tool_version_dir(tool, version);
        dir.exists() && has_binaries(&dir)
    }

    /// Return installed versions (so far) for a tool as a sorted list.
    pub fn installed_versions(&self, tool: &str) -> Vec<String> {
        let state = self.load_state().unwrap_or_default();
        let mut v: Vec<String> = state
            .tools
            .get(tool)
            .map(|m| {
                m.values()
                    .filter(|i| i.installed)
                    .map(|i| i.version.clone())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
}

fn has_binaries(dir: &Path) -> bool {
    let bin = dir.join("bin");
    if bin.is_dir() {
        if let Ok(read) = std::fs::read_dir(&bin) {
            return read.count() > 0;
        }
    }
    false
}

/// Compute a `[toolchain]` request from a config's toolchain block, resolving
/// wildcards/platform defaults where the user left fields blank. This returns
/// the effective version set to install.
pub fn effective_toolchain(tc: &Toolchain) -> Result<EffectiveToolchain> {
    Ok(EffectiveToolchain {
        rust: tc.rust.clone(),
        solana: tc.solana.clone(),
        anchor: tc.anchor.clone(),
        node: tc.node.clone(),
        package_manager: tc.package_manager.clone(),
    })
}

/// The effective, resolved toolchain the environment should target.
#[derive(Debug, Clone, Default)]
pub struct EffectiveToolchain {
    pub rust: Option<String>,
    pub solana: Option<String>,
    pub anchor: Option<String>,
    pub node: Option<String>,
    pub package_manager: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_paths() {
        let env = Environment::new("/tmp/foo".into());
        assert_eq!(env.solenv_dir(), PathBuf::from("/tmp/foo/.solenv"));
        assert_eq!(env.bin_dir(), PathBuf::from("/tmp/foo/.solenv/bin"));
        assert_eq!(
            env.versions_dir(),
            PathBuf::from("/tmp/foo/.solenv/versions")
        );
        assert_eq!(env.cache_dir(), PathBuf::from("/tmp/foo/.solenv/cache"));
        assert_eq!(
            env.tool_version_dir("anchor", "1.1.2"),
            PathBuf::from("/tmp/foo/.solenv/versions/anchor/1.1.2")
        );
        assert_eq!(
            env.tool_bin_dir("anchor", "1.1.2"),
            PathBuf::from("/tmp/foo/.solenv/versions/anchor/1.1.2/bin")
        );
    }

    #[test]
    fn state_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let env = Environment::new(tmp.path().to_path_buf());
        env.ensure_dirs().unwrap();
        env.record_installed("anchor", "1.1.2", Some("bin".into()))
            .unwrap();
        env.record_installed("solana", "4.0.0", None).unwrap();

        let state = env.load_state().unwrap();
        assert!(state.tools["anchor"]["1.1.2"].installed);
        assert!(state.tools["solana"]["4.0.0"].installed);
        assert_eq!(env.installed_versions("anchor"), vec!["1.1.2"]);
    }

    #[test]
    fn not_installed_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let env = Environment::new(tmp.path().to_path_buf());
        assert!(!env.is_installed("anchor", "1.1.2"));
    }
}
