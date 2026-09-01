//! Version parsing and comparison utilities.
//!
//! Solana/Anchor tooling versions come in a variety of shapes:
//!   * exact:   "1.92.0", "0.30.1", "1.1.2"
//!   * major:   "24" (Node major-only), "3.1.x" (matrix patterns)
//!   * channels: "stable", "nightly", "latest", "latest-pre-release"
//!
//! We normalise these into a `VersionReq`-like internal representation that
//! the compatibility dataset can match against.

use std::fmt;
use std::str::FromStr;

use anyhow::{bail, Result};

/// A parsed, comparable version "fingerprint".
///
/// `Spec` is intentionally looser than `semver::Version`: it captures up to
/// three numeric components and an optional pre-release/tag. This lets us
/// represent both real versions (`1.92.0`), patterns (`1.1.x`) and channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// "" for an exact version, "major", "minor" or "patch" for a wildcard.
    pub wildcard: Wildcard,
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Channel-like token for `stable`, `nightly`, `latest`, etc.
    pub channel: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wildcard {
    None,
    /// Only the major component is fixed (e.g. `3.x`).
    Major,
    /// Major and minor are fixed, patch is wild (e.g. `3.1.x`).
    Minor,
    Full,
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.wildcard {
            Wildcard::None => write!(f, "{}.{}.{}", self.major, self.minor, self.patch),
            Wildcard::Minor => write!(f, "{}.{}", self.major, self.minor),
            Wildcard::Major => write!(f, "{}", self.major),
            Wildcard::Full => write!(f, "*"),
        }
    }
}

impl FromStr for Spec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim();
        if s.is_empty() {
            bail!("empty version string");
        }
        // Channel names.
        let lower = s.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "latest" | "latest-pre-release" | "nightly" | "stable" | "beta"
        ) {
            return Ok(Spec {
                wildcard: Wildcard::Full,
                major: 0,
                minor: 0,
                patch: 0,
                channel: Some(lower),
            });
        }
        // Wildcard like "*" or "x".
        if s == "*" || s == "x" || s == "X" {
            return Ok(Spec {
                wildcard: Wildcard::Full,
                major: 0,
                minor: 0,
                patch: 0,
                channel: None,
            });
        }

        // Strip pre-release suffix: "1.1.2-rc.3" or "0.30.0-beta.1"
        let (nums, channel) = match s.split_once('-') {
            Some((n, pre)) => (n, Some(pre.to_string())),
            None => (s, None),
        };

        let parts: Vec<&str> = nums.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            bail!("invalid version string: {s:?}");
        }

        let parse = |p: &str| -> Result<u64> {
            p.parse::<u64>()
                .map_err(|_| anyhow::anyhow!("invalid version component {p:?} in {s:?}"))
        };

        let mut minor = 0u64;
        let mut patch = 0u64;
        let mut wildcard = Wildcard::None;

        if parts[0] == "x" || parts[0] == "X" || parts[0] == "*" {
            return Ok(Spec {
                wildcard: Wildcard::Full,
                major: 0,
                minor: 0,
                patch: 0,
                channel,
            });
        }
        let major = parse(parts[0])?;

        if parts.len() >= 2 {
            if parts[1] == "x" || parts[1] == "*" {
                wildcard = Wildcard::Major;
            } else {
                minor = parse(parts[1])?;
            }
        }
        if parts.len() == 3 {
            if parts[2] == "x" || parts[2] == "*" {
                if wildcard == Wildcard::None {
                    wildcard = Wildcard::Minor;
                }
            } else {
                patch = parse(parts[2])?;
            }
        }

        Ok(Spec {
            wildcard,
            major,
            minor,
            patch,
            channel,
        })
    }
}

impl Spec {
    pub fn exact(major: u64, minor: u64, patch: u64) -> Self {
        Spec {
            wildcard: Wildcard::None,
            major,
            minor,
            patch,
            channel: None,
        }
    }

    pub fn is_channel(&self) -> bool {
        self.channel.is_some()
    }

    pub fn is_wildcard(&self) -> bool {
        self.wildcard != Wildcard::None
    }

    /// Returns a `semver::Version` if this is an exact patch version with no
    /// channel, else `None`.
    pub fn to_semver(&self) -> Option<semver::Version> {
        if self.channel.is_some() || self.wildcard != Wildcard::None {
            return None;
        }
        semver::Version::new(self.major, self.minor, self.patch).into()
    }

    /// Does `self` (a requirement pattern) match an actual `version` (exact)?
    pub fn matches(&self, version: &Spec) -> bool {
        // Channel requirements only match channeled versions of the same kind,
        // or are treated as permissive.
        match (&self.channel, &version.channel) {
            (Some(a), Some(b)) => return a == b,
            (Some(_), None) => return true, // "latest"/"stable" can match any exact
            (None, Some(_)) => return false,
            (None, None) => {}
        }
        match self.wildcard {
            Wildcard::Full => true,
            Wildcard::Major => version.major == self.major,
            Wildcard::Minor => version.major == self.major && version.minor == self.minor,
            Wildcard::None => {
                version.major == self.major
                    && version.minor == self.minor
                    && version.patch == self.patch
            }
        }
    }
}

/// Comparator semantics for building range checks (>=, <=).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ordering {
    Min, // inclusive lower bound
    Max, // inclusive upper bound
}

/// Compare two exact versions lexically by (major, minor, patch).
/// Returns core::cmp::Ordering.
pub fn compare(a: &Spec, b: &Spec) -> std::cmp::Ordering {
    (a.major, a.minor, a.patch).cmp(&(b.major, b.minor, b.patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exact() {
        let s: Spec = "1.92.0".parse().unwrap();
        assert!(matches!(s.wildcard, Wildcard::None));
        assert_eq!(s.major, 1);
        assert_eq!(s.minor, 92);
        assert_eq!(s.patch, 0);
    }

    #[test]
    fn parse_major_only() {
        let s: Spec = "24".parse().unwrap();
        assert!(matches!(s.wildcard, Wildcard::None));
        assert_eq!(s.major, 24);
        assert_eq!(s.minor, 0);
    }

    #[test]
    fn parse_minor_wildcard() {
        let s: Spec = "1.1.x".parse().unwrap();
        assert!(matches!(s.wildcard, Wildcard::Minor));
        assert_eq!(s.major, 1);
        assert_eq!(s.minor, 1);
    }

    #[test]
    fn parse_major_wildcard() {
        let s: Spec = "3.x".parse().unwrap();
        assert!(matches!(s.wildcard, Wildcard::Major));
        assert_eq!(s.major, 3);
    }

    #[test]
    fn parse_patch_wildcard() {
        let s: Spec = "3.0.x".parse().unwrap();
        assert!(matches!(s.wildcard, Wildcard::Minor));
        assert_eq!(s.major, 3);
        assert_eq!(s.minor, 0);
    }

    #[test]
    fn parse_full_wildcard() {
        let s: Spec = "*".parse().unwrap();
        assert!(matches!(s.wildcard, Wildcard::Full));
    }

    #[test]
    fn parse_channel() {
        let s: Spec = "stable".parse().unwrap();
        assert_eq!(s.channel.as_deref(), Some("stable"));
    }

    #[test]
    fn parse_prerelease() {
        let s: Spec = "1.0.0-rc.3".parse().unwrap();
        assert!(matches!(s.wildcard, Wildcard::None));
        assert_eq!(s.major, 1);
        assert_eq!(s.channel.as_deref(), Some("rc.3"));
    }

    #[test]
    fn matches_exact() {
        let req: Spec = "1.1.2".parse().unwrap();
        assert!(req.matches(&"1.1.2".parse().unwrap()));
        assert!(!req.matches(&"1.1.3".parse().unwrap()));
        assert!(!req.matches(&"1.1.1".parse().unwrap()));
    }

    #[test]
    fn matches_minor_wildcard() {
        let req: Spec = "0.32.x".parse().unwrap();
        assert!(req.matches(&"0.32.1".parse().unwrap()));
        assert!(req.matches(&"0.32.9".parse().unwrap()));
        assert!(!req.matches(&"0.31.0".parse().unwrap()));
        assert!(!req.matches(&"1.0.0".parse().unwrap()));
    }

    #[test]
    fn matches_patch_wildcard() {
        let req: Spec = "3.x".parse().unwrap();
        assert!(req.matches(&"3.1.10".parse().unwrap()));
        assert!(!req.matches(&"2.1.0".parse().unwrap()));
    }

    #[test]
    fn matches_full_wildcard() {
        let req: Spec = "*".parse().unwrap();
        assert!(req.matches(&"0.30.1".parse().unwrap()));
        assert!(req.matches(&"4.0.0".parse().unwrap()));
    }

    #[test]
    fn compare_ordering() {
        assert_eq!(
            compare(&"1.89.0".parse().unwrap(), &"1.90.0".parse().unwrap()),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare(&"1.90.0".parse().unwrap(), &"1.89.5".parse().unwrap()),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare(&"1.90.0".parse().unwrap(), &"1.90.0".parse().unwrap()),
            std::cmp::Ordering::Equal
        );
    }
}
