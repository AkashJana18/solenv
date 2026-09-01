//! Shared process helpers for building project-scoped commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

/// The PATH list separator. On POSIX systems solenv targets this is `:`.
pub const PATH_SEPARATOR: char = ':';

/// Prepend `dir` to an existing PATH string (or use `dir` alone if none).
pub fn path_prepend(dir: &Path, existing: Option<&str>) -> String {
    let sep = PATH_SEPARATOR;
    match existing {
        Some(existing) if !existing.is_empty() => {
            format!("{}{sep}{existing}", dir.display())
        }
        _ => dir.display().to_string(),
    }
}

/// Prepend several dirs (in order, first wins) to an existing PATH.
pub fn path_prepend_many(dirs: &[PathBuf], existing: Option<&str>) -> String {
    let sep = PATH_SEPARATOR;
    let mut parts: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    if let Some(existing) = existing {
        if !existing.is_empty() {
            parts.push(existing.to_string());
        }
    }
    parts.join(&sep.to_string())
}

/// Run `args[0]` (resolved against `search_dirs`) with a scoped environment.
///
/// `extra_env` are set on the child; the current process env is inherited by
/// default and augmented. Returns the child's exit code.
pub fn run_scoped(
    program: &Path,
    args: &[String],
    search_dirs: &[PathBuf],
    extra_env: &BTreeMap<String, String>,
) -> Result<i32> {
    if args.is_empty() {
        bail!("no command given");
    }
    let resolved = if program.is_absolute() {
        program.to_path_buf()
    } else {
        resolve_in_dirs(program, search_dirs).unwrap_or_else(|| program.to_path_buf())
    };
    if !resolved.exists() {
        bail!(
            "command not found in this environment: {} (searched {:?})",
            args[0],
            search_dirs
        );
    }

    let mut cmd = std::process::Command::new(&resolved);
    cmd.args(&args[1..]);

    // Build PATH: our search dirs first, then existing.
    let existing_path = std::env::var("PATH").ok();
    cmd.env(
        "PATH",
        path_prepend_many(search_dirs, existing_path.as_deref()),
    );

    // Apply existing environment + overrides.
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());
    cmd.stdin(std::process::Stdio::inherit());

    let status = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run {}: {e}", args[0]))?;
    Ok(status.code().unwrap_or(1))
}

/// Search `dirs` (in order) for a file named `name`, returning the first hit.
pub fn resolve_in_dirs(name: &Path, dirs: &[PathBuf]) -> Option<PathBuf> {
    for d in dirs {
        let cand = d.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// Run a command and capture stdout as a string.
pub fn capture(bin: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run {}: {e}", bin.display()))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepend_single() {
        let p = path_prepend(&PathBuf::from("/a/bin"), Some("/usr/bin"));
        assert_eq!(p, "/a/bin:/usr/bin");
    }

    #[test]
    fn prepend_many_ordering() {
        let dirs = vec![PathBuf::from("/one"), PathBuf::from("/two")];
        let p = path_prepend_many(&dirs, Some("/usr/bin"));
        assert_eq!(p, "/one:/two:/usr/bin");
    }

    #[test]
    fn prepend_many_no_existing() {
        let dirs = vec![PathBuf::from("/one")];
        assert_eq!(path_prepend_many(&dirs, None), "/one");
    }

    #[test]
    fn resolve_in_dirs_finds() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("foo"), b"x").unwrap();
        let found = resolve_in_dirs(Path::new("foo"), &[tmp.path().to_path_buf()]);
        assert!(found.is_some());
        let missing = resolve_in_dirs(Path::new("nope"), &[tmp.path().to_path_buf()]);
        assert!(missing.is_none());
    }
}
