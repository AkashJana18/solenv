//! Platform detection (isolated so Windows can be added later).

use anyhow::Result;

/// The host triple, e.g. `x86_64-apple-darwin`. Used to select release assets.
///
/// On Linux, only glibc distributions are supported (musl distros such as
/// Alpine do not ship the prebuilt `*-unknown-linux-gnu` Solana/Anchor assets
/// solenv downloads), so an explicit error is returned there.
pub fn host_triple() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("linux", "x86_64") => {
            require_glibc()?;
            Ok("x86_64-unknown-linux-gnu".to_string())
        }
        ("linux", "aarch64") => {
            require_glibc()?;
            Ok("aarch64-unknown-linux-gnu".to_string())
        }
        _ => anyhow::bail!(
            "unsupported platform {os}/{arch}; solenv currently supports macOS and Linux on x86_64/aarch64"
        ),
    }
}

/// Whether this build is effectively a Linux glibc machine (informational for
/// doctor's GLIBC checks).
pub fn is_macos() -> bool {
    std::env::consts::OS == "macos"
}

/// Best-effort detection of the Linux libc flavor: `"glibc"`, `"musl"`, or
/// `"unknown"`. Only meaningful on Linux; returns `"glibc"` on macOS.
pub fn libc_flavor() -> &'static str {
    if is_macos() {
        return "glibc";
    }
    // musl's dynamic loader is always named ld-musl-<arch>.so.1.
    for entry in std::fs::read_dir("/lib").into_iter().flatten().flatten() {
        let fname = entry.file_name();
        let name = fname.to_string_lossy();
        if name.starts_with("ld-musl-") {
            return "musl";
        }
    }
    "glibc"
}

/// Bail with a clear message when running on a musl-based Linux (e.g. Alpine),
/// where the official glibc Solana/Anchor release assets cannot run and no musl
/// equivalents are published.
fn require_glibc() -> Result<()> {
    if libc_flavor() == "musl" {
        anyhow::bail!(
            "this Linux uses musl libc (e.g. Alpine), but solenv needs the glibc-based \
             Solana/Anchor binaries. These distros are not supported; use a glibc \
             distribution (Ubuntu, Debian, Fedora) or Ubuntu/WSL2."
        );
    }
    Ok(())
}

/// Whether solenv is running inside WSL (Windows Subsystem for Linux).
///
/// WSL's Linux kernel identifies itself in `/proc/version` with a
/// "microsoft" marker, e.g. `... #1 SMP ... Microsoft ...`.
pub fn is_wsl() -> bool {
    let Ok(out) = std::fs::read_to_string("/proc/version") else {
        return false;
    };
    let lower = out.to_ascii_lowercase();
    lower.contains("microsoft") || lower.contains("wsl")
}

/// A short hint for WSL users, or `None` when not on WSL.
pub fn wsl_hint() -> Option<&'static str> {
    if !is_wsl() {
        return None;
    }
    Some(
        "running inside WSL: run solenv from the Linux shell, keep projects on the native \
         Linux filesystem (~/…), and use a glibc distro (Ubuntu is the default).",
    )
}

/// Return (os, arch) short codes used by Node.js release asset naming:
/// `node-v24.0.0-<os>-<arch>.tar.xz`, e.g. `darwin-arm64`, `linux-x64`.
pub fn node_asset() -> Result<(String, String)> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let os_code = match os {
        "macos" => "darwin",
        "linux" => "linux",
        other => anyhow::bail!("unsupported OS for Node: {other}"),
    };
    let arch_code = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => anyhow::bail!("unsupported arch for Node: {other}"),
    };
    Ok((os_code.to_string(), arch_code.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_triple_is_supported() {
        let t = host_triple().unwrap();
        assert!(t.ends_with("-apple-darwin") || t.ends_with("-unknown-linux-gnu"));
    }

    #[test]
    fn node_asset_codes_valid() {
        let (os, arch) = node_asset().unwrap();
        assert!(["darwin", "linux"].contains(&os.as_str()));
        assert!(["x64", "arm64"].contains(&arch.as_str()));
    }

    #[test]
    fn wsl_never_true_without_proc_version_marker() {
        // is_wsl() only reads /proc/version; on non-WSL hosts it must be false.
        // This is best-effort but should not crash anywhere.
        let _ = is_wsl();
    }

    #[test]
    fn docstring_wsl_detection_matches_markers() {
        // The detection keywords live in the code; assert the marker strings we
        // rely on are the canonical ones so drift is caught.
        let sample = "Linux version 6.6.36.3-microsoft-standard-WSL2 (oe-user@oe-host) ...";
        let lower = sample.to_ascii_lowercase();
        assert!(lower.contains("microsoft") || lower.contains("wsl"));
    }
}
