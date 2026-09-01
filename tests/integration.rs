//! End-to-end integration tests for the `solenv` CLI.
//!
//! These do not require a real Solana installation. They exercise the CLI with
//! a project-local environment built from fake toolchain binaries, so the
//! command resolution logic is verified without network installs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

fn bin() -> Command {
    Command::cargo_bin("solenv").expect("solenv binary")
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// Create a fake, executable tool binary in the environment's version bin dir.
fn fake_tool(env_dir: &Path, tool: &str, version: &str, prog: &str, output: &str) {
    let bin_dir = env_dir
        .join(".solenv/versions")
        .join(tool)
        .join(version)
        .join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = format!("#!/usr/bin/env bash\necho \"{output}\" \"$@\"\n");
    write(&bin_dir.join(prog), &script);
    let mut perms = fs::metadata(bin_dir.join(prog)).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(bin_dir.join(prog), perms).unwrap();
}

const COMPAT_TOML: &str = r#"
[toolchain]
rust = "1.92.0"
solana = "3.1.10"
anchor = "1.1.2"
node = "24.13.0"
package_manager = "npm"
"#;

#[test]
fn init_creates_solenv_toml() {
    let tmp = TempDir::new().unwrap();
    bin()
        .args(["init", "--yes"])
        .current_dir(tmp.path())
        .assert()
        .success();
    let cfg_path = tmp.path().join("solenv.toml");
    assert!(cfg_path.exists());
    let content = fs::read_to_string(&cfg_path).unwrap();
    assert!(content.contains("[toolchain]"));
}

#[test]
fn init_detects_anchor_toml_toolchain() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp.path().join("Anchor.toml"),
        "[provider]\ncluster = \"localnet\"\n\n[toolchain]\nanchor_version = \"0.32.1\"\nsolana_version = \"2.1.7\"\n",
    );
    bin()
        .args(["init", "--yes"])
        .current_dir(tmp.path())
        .assert()
        .success();
    let content = fs::read_to_string(tmp.path().join("solenv.toml")).unwrap();
    assert!(content.contains("anchor = \"0.32.1\""));
    assert!(content.contains("solana = \"2.1.7\""));
}

#[test]
fn check_reports_healthy_for_compatible_stack() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("solenv.toml"), COMPAT_TOML);
    bin()
        .args(["check"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Environment is healthy."));
}

#[test]
fn check_reports_incompatible() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp.path().join("solenv.toml"),
        r#"
[toolchain]
rust = "1.60.0"
solana = "1.18.0"
anchor = "1.1.2"
node = "14.0.0"
"#,
    );
    bin()
        .args(["check"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Incompatible toolchain"));
}

#[test]
fn check_requires_config() {
    let tmp = TempDir::new().unwrap();
    bin()
        .args(["check"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("no solenv.toml"));
}

#[test]
fn run_uses_project_tool_and_arguments() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp.path().join("solenv.toml"),
        "[toolchain]\nsolana = \"3.1.10\"\n",
    );
    // Seed a fake solana binary for the pinned version.
    fake_tool(
        tmp.path(),
        "solana",
        "3.1.10",
        "solana",
        "FAKE-SOLANA-3.1.10",
    );

    bin()
        .args(["run", "solana", "--version"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("FAKE-SOLANA-3.1.10"));
}

#[test]
fn run_pins_anchor_version() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp.path().join("solenv.toml"),
        "[toolchain]\nanchor = \"1.1.2\"\n",
    );
    fake_tool(tmp.path(), "anchor", "1.1.2", "anchor", "FAKE-ANCHOR-1.1.2");

    bin()
        .args(["run", "anchor", "build"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("FAKE-ANCHOR-1.1.2"));
}

#[test]
fn run_errors_when_tool_not_installed() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp.path().join("solenv.toml"),
        "[toolchain]\nsolana = \"3.1.10\"\n",
    );
    // No fake binaries seeded.
    bin()
        .args(["run", "solana", "--version"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not installed"));
}

#[test]
fn list_shows_configured_versions() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("solenv.toml"), COMPAT_TOML);
    bin()
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Configured toolchain"));
}

#[test]
fn clean_removes_installed_versions() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("solenv.toml"), COMPAT_TOML);
    fake_tool(tmp.path(), "solana", "3.1.10", "solana", "FAKE");
    let version_dir = tmp.path().join(".solenv/versions/solana/3.1.10");
    assert!(version_dir.exists());
    bin()
        .args(["clean", "--yes"])
        .current_dir(tmp.path())
        .assert()
        .success();
    assert!(!version_dir.exists());
    // Config preserved.
    assert!(tmp.path().join("solenv.toml").exists());
}

#[test]
fn uninstall_removes_solenv_dir() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("solenv.toml"), COMPAT_TOML);
    fs::create_dir_all(tmp.path().join(".solenv")).unwrap();
    bin()
        .args(["uninstall", "--yes"])
        .current_dir(tmp.path())
        .assert()
        .success();
    assert!(!tmp.path().join(".solenv").exists());
    assert!(tmp.path().join("solenv.toml").exists());
}

#[test]
fn doctor_runs_and_diagnoses() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("solenv.toml"), COMPAT_TOML);
    bin()
        .args(["doctor"])
        .current_dir(tmp.path())
        .assert()
        .success();
}
