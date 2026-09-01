//! Platform detection (isolated so Windows can be added later).

use anyhow::Result;

/// The host triple, e.g. `x86_64-apple-darwin`. Used to select release assets.
pub fn host_triple() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu".to_string()),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu".to_string()),
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
}
