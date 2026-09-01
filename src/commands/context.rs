//! Shared command helpers: project root resolution, config loading, env.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::Cli;
use crate::config::{load, project_root, SolenvConfig};
use crate::environment::Environment;
use crate::errors::{ErrorKind, SolenvError};

/// Resolve the project root. Uses `--dir` if given; otherwise the nearest dir
/// containing `solenv.toml`, else the current directory.
pub fn resolve_root(cli: &Cli) -> Result<PathBuf> {
    if let Some(d) = &cli.dir {
        let abs = std::fs::canonicalize(d)
            .with_context(|| format!("directory does not exist: {}", d.display()))?;
        return Ok(abs);
    }
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    Ok(project_root(&cwd))
}

/// The Environment for a project root.
pub fn env_for(root: &Path) -> Environment {
    Environment::new(root.to_path_buf())
}

/// Load `solenv.toml` from root, with an actionable error if missing.
pub fn require_config(root: &Path) -> Result<SolenvConfig> {
    let path = root.join("solenv.toml");
    if !path.exists() {
        return Err(anyhow::Error::from(
            SolenvError::new(
                ErrorKind::Config,
                format!("no solenv.toml in {}", root.display()),
            )
            .with_why("this project has not been initialized with solenv.")
            .with_fix(format!(
                "run `solenv init` in {} to create one, then `solenv install`.",
                root.display()
            )),
        ));
    }
    load(&path).map_err(|e| {
        anyhow::Error::from(
            SolenvError::new(
                ErrorKind::Config,
                format!("failed to read {}", path.display()),
            )
            .with_why(e.to_string())
            .with_fix("check that solenv.toml is valid TOML."),
        )
    })
}
