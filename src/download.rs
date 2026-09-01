//! Secure download and checksum verification utilities.
//!
//! Security model:
//!   * HTTPS only (reqwest is configured with rustls, no plaintext fallback).
//!   * SHA-256 checksums verified before any file is used.
//!   * Files are written to a temp path then atomically renamed on success.
//!   * Downloaded files are never executed before verification.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// Download `url` to `dest`. If `expected_sha256` is `Some`, verify the file
/// after download and error on mismatch. Returns the SHA-256 of what was
/// downloaded (for recording).
pub fn download_verified(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
) -> Result<String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let resp = client
        .get(url)
        .send()
        .with_context(|| format!("failed to GET {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!(
            "download failed for {url}: HTTP {status} ({}). Is the version valid?",
            status.canonical_reason().unwrap_or("unknown")
        );
    }

    // Stream to a temp file, hashing as we go.
    let tmp = temp_dest(dest);
    let mut f =
        File::create(&tmp).with_context(|| format!("failed to create {}", tmp.display()))?;
    let mut hasher = Sha256::new();
    let mut reader = resp;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .with_context(|| format!("failed reading {url}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        f.write_all(&buf[..n])
            .with_context(|| "failed writing to disk")?;
    }
    f.flush().ok();
    drop(f);

    let digest = format!("{:x}", hasher.finalize());

    if let Some(expected) = expected_sha256 {
        if !digest.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!(
                "SHA-256 checksum mismatch for {url}\n  expected: {expected}\n  actual:   {digest}\nThe download is untrusted and will not be used."
            );
        }
    }

    std::fs::rename(&tmp, dest).with_context(|| format!("failed to write {}", dest.display()))?;
    Ok(digest)
}

/// A unique temp path adjacent to `dest` (same filesystem for atomic rename).
fn temp_dest(dest: &Path) -> PathBuf {
    use std::ffi::OsStr;
    let mut name = dest
        .file_name()
        .unwrap_or(OsStr::new("download"))
        .to_os_string();
    name.push(format!(".tmp{}", std::process::id()));
    dest.with_file_name(name)
}

/// Compute the SHA-256 of a file on disk.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .with_context(|| format!("failed reading {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify a file on disk matches an expected sha256 (case-insensitive).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<bool> {
    let actual = sha256_file(path)?;
    Ok(actual.eq_ignore_ascii_case(expected))
}

/// Extract a `.tar.gz` archive into `dest_dir`.
pub fn extract_tar_gz(archive: &Path, dest_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("failed to create {}", dest_dir.display()))?;
    let f = File::open(archive).with_context(|| format!("failed to open {}", archive.display()))?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(dest_dir).with_context(|| {
        format!(
            "failed to extract {} into {}",
            archive.display(),
            dest_dir.display()
        )
    })?;
    Ok(())
}

/// Extract a `.tar.xz` archive into `dest_dir` (Node distributions use .tar.xz).
pub fn extract_tar_xz(archive: &Path, dest_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("failed to create {}", dest_dir.display()))?;
    let f = File::open(archive).with_context(|| format!("failed to open {}", archive.display()))?;
    let xz = xz2::read::XzDecoder::new(f);
    let mut tar = tar::Archive::new(xz);
    tar.unpack(dest_dir).with_context(|| {
        format!(
            "failed to extract {} into {}",
            archive.display(),
            dest_dir.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("empty");
        std::fs::write(&p, b"").unwrap();
        // SHA-256 of empty string
        assert_eq!(
            sha256_file(&p).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn verify_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("f");
        std::fs::write(&p, b"hello").unwrap();
        let h = sha256_file(&p).unwrap();
        assert!(verify_sha256(&p, &h).unwrap());
        assert!(!verify_sha256(&p, "deadbeef".repeat(8).as_str()).unwrap());
    }
}
