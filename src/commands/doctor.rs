//! `solenv doctor`: diagnose common environment problems with actionable fixes.

use anyhow::Result;

use super::context::{env_for, resolve_root};
use crate::cli::Cli;
use crate::compatibility::COMPATIBILITY_MATRIX_URL;
use crate::compatibility::{validate, ToolchainRequest};
use crate::config::SolenvConfig;
use crate::environment::Environment;
use crate::managers::anchor::AnchorManager;
use crate::managers::node::NodeManager;
use crate::managers::rust::RustManager;
use crate::managers::solana::SolanaManager;
use crate::managers::Manager;

#[derive(Debug)]
struct Finding {
    ok: bool,
    title: String,
    detail: Option<String>,
    fix: Option<String>,
}

impl Finding {
    fn ok(title: impl Into<String>) -> Self {
        Finding {
            ok: true,
            title: title.into(),
            detail: None,
            fix: None,
        }
    }
    fn err(title: impl Into<String>, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Finding {
            ok: false,
            title: title.into(),
            detail: Some(detail.into()),
            fix: Some(fix.into()),
        }
    }
    fn warn(title: impl Into<String>, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Finding {
            ok: false,
            title: title.into(),
            detail: Some(detail.into()),
            fix: Some(fix.into()),
        }
    }
}

pub fn run(cli: &Cli) -> Result<()> {
    let root = resolve_root(cli)?;
    let env = env_for(&root);

    println!("solenv doctor");
    println!("{}", "─".repeat(34));
    println!("Project: {}", root.display());
    println!();

    let mut findings = Vec::new();
    let mut fatal = false;

    // 1. Is solenv initialized?
    let config_path = root.join("solenv.toml");
    if !config_path.exists() {
        findings.push(Finding::err(
            "solenv.toml missing",
            "this project is not initialized.",
            "run `solenv init` to create solenv.toml, then `solenv install`.",
        ));
        fatal = true;
    }
    let cfg = if config_path.exists() {
        match crate::config::load(&config_path) {
            Ok(c) => Some(c),
            Err(e) => {
                findings.push(Finding::err(
                    "solenv.toml unreadable",
                    format!("{e}"),
                    "fix the TOML syntax.",
                ));
                fatal = true;
                None
            }
        }
    } else {
        None
    };

    // 2. Core orchestrator tools present?
    check_on_path(&mut findings, "rustup");
    check_on_path(&mut findings, "avm");
    check_on_path(&mut findings, "node");

    // 3. .solenv writable/permissions.
    check_permissions(&mut findings, &env);

    // 4. Corrupted cached downloads (incompletely written temp files).
    check_cache(&mut findings, &env);

    if let Some(cfg) = &cfg {
        let tc = cfg.toolchain.clone().unwrap_or_default();
        let rust = RustManager::new();
        let solana = SolanaManager::new();
        let anchor = AnchorManager::new();
        let node = NodeManager::new();

        // 5. Pinned tools installed in environment.
        check_tool_installed(&mut findings, &rust, &env, tc.rust.as_deref(), "Rust");
        check_tool_installed(
            &mut findings,
            &solana,
            &env,
            tc.solana.as_deref(),
            "Solana/Agave",
        );
        check_tool_installed(&mut findings, &anchor, &env, tc.anchor.as_deref(), "Anchor");
        check_tool_installed(&mut findings, &node, &env, tc.node.as_deref(), "Node");

        // 6. Compatibility.
        let req: ToolchainRequest = (&tc).into();
        match validate(&req) {
            Ok(violations) if violations.is_empty() => {
                findings.push(Finding::ok("toolchain combinations compatible"));
            }
            Ok(violations) => {
                let detail = violations
                    .iter()
                    .map(|v| v.message.clone())
                    .collect::<Vec<_>>()
                    .join("; ");
                findings.push(Finding::err(
                    "toolchain combination incompatible",
                    detail,
                    format!("adjust versions in solenv.toml. See {COMPATIBILITY_MATRIX_URL}"),
                ));
            }
            Err(e) => {
                findings.push(Finding::warn(
                    "could not validate compatibility",
                    e.to_string(),
                    "internal error; report it.",
                ));
            }
        }

        // 7. SBF platform tools / rust target presence (best-effort).
        check_sbf(&mut findings, &env, tc.solana.as_deref());
    } else {
        findings.push(Finding::warn(
            "skipping toolchain checks",
            "no valid solenv.toml.",
            "run `solenv init`.",
        ));
    }

    // ---- Report ----
    let mut errors = 0;
    for f in &findings {
        if f.ok {
            println!("✓ {}", f.title);
        } else {
            println!("✗ {}", f.title);
            errors += 1;
            if let Some(d) = &f.detail {
                println!("    {d}");
            }
            if let Some(fx) = &f.fix {
                println!("    Fix: {fx}");
            }
        }
    }
    println!("{}", "─".repeat(34));
    if errors == 0 {
        println!("No problems found.");
        if fatal {
            // shouldn't happen given fatal sets errors
        }
    } else {
        println!("{errors} issue(s) found. Apply the fixes above, or run `solenv install`.");
    }

    Ok(())
}

fn check_tool_installed<M: Manager>(
    findings: &mut Vec<Finding>,
    m: &M,
    env: &Environment,
    pinned: Option<&str>,
    label: &str,
) {
    let Some(spec) = pinned else {
        return;
    };
    if spec.trim().is_empty() {
        return;
    }
    let resolved = m.resolve(spec).unwrap_or_else(|_| spec.to_string());
    if m.is_installed(env, &resolved) {
        findings.push(Finding::ok(format!("{label} {resolved} installed")));
    } else {
        findings.push(Finding::err(
            format!("{label} {resolved} not installed"),
            format!("the pinned {label} is missing from this environment."),
            format!(
                "run `solenv install`{}.",
                if m.name() == "node" {
                    " (needs network to download Node)"
                } else {
                    ""
                }
            ),
        ));
    }
}

fn check_on_path(findings: &mut Vec<Finding>, bin: &str) {
    match which::which(bin) {
        Ok(path) => findings.push(Finding::ok(format!("{bin} found at {}", path.display()))),
        Err(_) => {
            let fix = match bin {
                "rustup" => "Install rustup: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh".to_string(),
                "avm" => "Install avm: cargo install avm --locked  (or curl the official installer)".to_string(),
                "node" => "Install Node.js: run `solenv install` to provision it project-locally, or install a system Node.".to_string(),
                _ => format!("install {bin} and add it to PATH"),
            };
            findings.push(Finding::warn(
                format!("{bin} not found on PATH"),
                "solenv can provision it yourself for most tools.",
                fix,
            ));
        }
    }
}

fn check_permissions(findings: &mut Vec<Finding>, env: &Environment) {
    if std::env::consts::OS == "macos" || std::env::consts::OS == "linux" {
        let uid = unsafe { libc::geteuid() };
        if uid == 0 {
            findings.push(Finding::err(
                "running as root",
                "running node/package managers as root is unsafe.",
                "do not run solenv as root.",
            ));
        }
    }
    // Can we create the .solenv dir?
    let probe = env.solenv_dir().join(".solenv-probe");
    let result = std::fs::create_dir_all(&probe);
    let ok = result.is_ok();
    if ok {
        std::fs::remove_dir_all(env.solenv_dir().join(".solenv-probe")).ok();
    }
    if !ok {
        findings.push(Finding::err(
            "cannot write to .solenv",
            format!("{} is not writable", env.solenv_dir().display()),
            "check directory ownership/permissions.",
        ));
    } else {
        findings.push(Finding::ok(".solenv writable"));
    }
}

fn check_cache(findings: &mut Vec<Finding>, env: &Environment) {
    let dl = env.downloads_dir();
    if !dl.exists() {
        return;
    }
    let mut corrupted = 0;
    if let Ok(read) = std::fs::read_dir(&dl) {
        for entry in read.flatten() {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if s.contains(".tmp") {
                corrupted += 1;
            }
        }
    }
    if corrupted > 0 {
        findings.push(Finding::warn(
            "possible interrupted downloads",
            format!("{corrupted} temporary download file(s) found."),
            "run `solenv clean --cache` to remove stale downloads, then reinstall.",
        ));
    } else {
        findings.push(Finding::ok("no corrupted cached downloads"));
    }
}

fn check_sbf(findings: &mut Vec<Finding>, env: &Environment, solana: Option<&str>) {
    if let Some(ver) = solana {
        let solana_mgr = SolanaManager::new();
        if solana_mgr.is_installed(env, ver) {
            let bin = match solana_mgr.resolve_bin_dir(env, ver) {
                Ok(b) => b,
                Err(_) => return,
            };
            let build_sbf = bin.join("cargo-build-sbf");
            if build_sbf.exists() {
                findings.push(Finding::ok(
                    "cargo-build-sbf available (SBF platform tools)",
                ));
            } else {
                findings.push(Finding::warn(
                    "SBF platform tools may be missing",
                    "cargo-build-sbf was not found in the Solana install.",
                    "reinstall the Solana CLI for this version (the release bundles platform-tools).",
                ));
            }
        }
    }
}

// Re-export for potential tests.
#[allow(dead_code)]
fn _cfg_used(c: &SolenvConfig) -> bool {
    c.toolchain.is_some()
}
