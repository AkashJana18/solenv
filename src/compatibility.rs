//! Compatibility layer.
//!
//! Consumes the embedded `data/compatibility.toml` dataset (itself derived from
//! the Solana Foundation compatibility matrix, see that file's header for the
//! exact provenance of every rule). Exposure is intentionally data-driven so a
//! versioned online manifest can replace the embedded dataset later.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::version::{compare, Spec};

/// One `[[anchor_solana]]` row.
#[derive(Debug, Clone, Deserialize)]
struct AnchorSolanaRule {
    anchor: String,
    min_solana: Option<String>,
    max_solana: Option<String>,
    #[serde(default)]
    note: String,
}

/// One `[[anchor_rust]]` row.
#[derive(Debug, Clone, Deserialize)]
struct AnchorRustRule {
    anchor: String,
    min_rust: Option<String>,
    max_rust: Option<String>,
    #[serde(default)]
    note: String,
}

/// One `[[anchor_node]]` row.
#[derive(Debug, Clone, Deserialize)]
struct AnchorNodeRule {
    anchor: String,
    min_node: Option<String>,
    #[serde(default)]
    note: String,
}

/// One `[[known_combination]]` row.
#[derive(Debug, Clone, Deserialize)]
struct KnownCombination {
    anchor: String,
    solana: String,
    rust: String,
    platform_tools: String,
    node: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct Dataset {
    anchor_solana: Vec<AnchorSolanaRule>,
    anchor_rust: Vec<AnchorRustRule>,
    anchor_node: Vec<AnchorNodeRule>,
    known_combination: Vec<KnownCombination>,
}

/// Current default (latest) platform-tools version used for build-sbf.
pub const DEFAULT_PLATFORM_TOOLS: &str = "v1.52";

/// URL of the authoritative compatibility matrix.
pub const COMPATIBILITY_MATRIX_URL: &str =
    "https://github.com/solana-foundation/solana-dev-skill/blob/main/skills/solana-dev/references/compatibility-matrix.md";

/// Parse a semver-style exact version into a `Spec`.
fn spec(s: &str) -> Result<Spec> {
    s.parse()
        .with_context(|| format!("invalid version in compatibility dataset: {s:?}"))
}

static DATASET: Lazy<Result<Dataset, String>> =
    Lazy::new(|| Dataset::parse_embedded().map_err(|e| e.to_string()));

impl Dataset {
    fn parse_embedded() -> Result<Dataset> {
        // Embedded at build time via include_str!; see main.rs/lib.rs for the
        // path resolution.
        let raw = crate::DATA_COMPATIBILITY;
        toml::from_str(raw).context("failed to parse embedded compatibility.toml")
    }

    fn get() -> &'static Dataset {
        match DATASET.as_ref() {
            Ok(d) => d,
            Err(e) => panic!("embedded compatibility dataset failed to load: {e}"),
        }
    }
}

/// What we know about a requested toolchain.
#[derive(Debug, Clone, Default)]
pub struct ToolchainRequest {
    pub rust: Option<String>,
    pub solana: Option<String>,
    pub anchor: Option<String>,
    pub node: Option<String>,
}

impl From<&crate::config::Toolchain> for ToolchainRequest {
    fn from(tc: &crate::config::Toolchain) -> Self {
        ToolchainRequest {
            rust: tc.rust.clone(),
            solana: tc.solana.clone(),
            anchor: tc.anchor.clone(),
            node: tc.node.clone(),
        }
    }
}

/// A single compatibility problem found during validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Short key used by tests, e.g. "anchor_solana".
    pub rule: &'static str,
    /// Which tool the problem is about, e.g. "Anchor".
    pub tool: &'static str,
    /// The requested version.
    pub requested: String,
    /// A constraint (e.g. min "3.1.0") that triggered the problem.
    pub constraint: String,
    /// Human-readable explanation.
    pub message: String,
}

impl Violation {
    pub fn display(&self) -> String {
        format!("✗ {} {}\n   {}", self.tool, self.requested, self.message)
    }
}

/// Validate a requested toolchain against the dataset. Returns a list of
/// violations (empty == compatible).
pub fn validate(req: &ToolchainRequest) -> Result<Vec<Violation>> {
    let dataset = Dataset::get();
    let mut violations = Vec::new();

    let anchor = match &req.anchor {
        Some(a) => Some(spec(a)?),
        None => None,
    };
    let solana = match &req.solana {
        Some(s) => Some(spec(s)?),
        None => None,
    };
    let rust = match &req.rust {
        Some(r) => Some(spec(r)?),
        None => None,
    };
    let node = match &req.node {
        Some(n) => Some(spec(n)?),
        None => None,
    };

    // Anchor <-> Solana
    if let (Some(anchor_s), Some(solana_s)) = (&anchor, &solana) {
        if let Some(rule) = match_rule(&dataset.anchor_solana, anchor_s) {
            if let Some(minv) = &rule.min_solana {
                let min = spec(minv)?;
                if !min.is_wildcard() {
                    // min is inclusive: solana >= min_solana
                    if compare(solana_s, &min) == std::cmp::Ordering::Less {
                        violations.push(Violation {
                            rule: "anchor_solana",
                            tool: "Anchor",
                            requested: anchor_s.to_string(),
                            constraint: format!("Solana >= {minv}"),
                            message: format!(
                                "Anchor {} requires Solana/Agave CLI >= {minv} (got {}; {}).",
                                anchor_s, solana_s, rule.note
                            ),
                        });
                    }
                }
            }
            if let Some(maxv) = &rule.max_solana {
                let max = spec(maxv)?;
                if !max.is_wildcard() && compare(solana_s, &max) == std::cmp::Ordering::Greater {
                    violations.push(Violation {
                            rule: "anchor_solana",
                            tool: "Anchor",
                            requested: anchor_s.to_string(),
                            constraint: format!("Solana <= {maxv}"),
                            message: format!(
                                "Anchor {} is not compatible with Solana/Agave CLI {} (max {maxv}; {}).",
                                anchor_s, solana_s, rule.note
                            ),
                        });
                }
            }
        }
    }

    // Anchor <-> Rust
    if let (Some(anchor_s), Some(rust_s)) = (&anchor, &rust) {
        if let Some(rule) = match_rule(&dataset.anchor_rust, anchor_s) {
            if let Some(minv) = &rule.min_rust {
                let min = spec(minv)?;
                if !min.is_wildcard()
                    && !rust_s.is_wildcard()
                    && compare(rust_s, &min) == std::cmp::Ordering::Less
                {
                    violations.push(Violation {
                        rule: "anchor_rust",
                        tool: "Anchor",
                        requested: anchor_s.to_string(),
                        constraint: format!("Rust >= {minv}"),
                        message: format!(
                            "Anchor {} requires Rust >= {minv} (got {}; {}).",
                            anchor_s, rust_s, rule.note
                        ),
                    });
                }
            }
            if let Some(maxv) = &rule.max_rust {
                let max = spec(maxv)?;
                if !max.is_wildcard()
                    && !rust_s.is_wildcard()
                    && compare(rust_s, &max) == std::cmp::Ordering::Greater
                {
                    violations.push(Violation {
                        rule: "anchor_rust",
                        tool: "Anchor",
                        requested: anchor_s.to_string(),
                        constraint: format!("Rust <= {maxv}"),
                        message: format!(
                            "Anchor {} may not support Rust {} (documented max {maxv}; {}).",
                            anchor_s, rust_s, rule.note
                        ),
                    });
                }
            }
        }
    }

    // Anchor <-> Node
    if let (Some(anchor_s), Some(node_s)) = (&anchor, &node) {
        if let Some(rule) = match_rule(&dataset.anchor_node, anchor_s) {
            if let Some(minv) = &rule.min_node {
                let min = spec(minv)?;
                if !min.is_wildcard()
                    && !node_s.is_wildcard()
                    && compare(node_s, &min) == std::cmp::Ordering::Less
                {
                    violations.push(Violation {
                        rule: "anchor_node",
                        tool: "Anchor",
                        requested: anchor_s.to_string(),
                        constraint: format!("Node >= {minv}"),
                        message: format!(
                            "Anchor {} requires Node.js >= {minv} (got {}; {}).",
                            anchor_s, node_s, rule.note
                        ),
                    });
                }
            }
        }
    }

    Ok(violations)
}

/// Generic: pick the most specific (first) rule whose anchor requirement
/// matches the requested anchor version.
trait HasAnchor {
    fn anchor(&self) -> &str;
}
impl HasAnchor for AnchorSolanaRule {
    fn anchor(&self) -> &str {
        &self.anchor
    }
}
impl HasAnchor for AnchorRustRule {
    fn anchor(&self) -> &str {
        &self.anchor
    }
}
impl HasAnchor for AnchorNodeRule {
    fn anchor(&self) -> &str {
        &self.anchor
    }
}

fn match_rule<'a, T: HasAnchor>(rules: &'a [T], anchor: &Spec) -> Option<&'a T> {
    rules
        .iter()
        .find(|r| r.anchor().parse::<Spec>().is_ok_and(|p| p.matches(anchor)))
}

/// Look up a documented known combination for a given anchor major line, if
/// any. Returns a human-readable recommendation.
pub fn recommended_for(anchor: &str) -> Option<KnownCombinationOut> {
    let dataset = Dataset::get();
    let anchor_spec: Spec = anchor.parse().ok()?;
    for k in &dataset.known_combination {
        let pat: Spec = k.anchor.parse().ok()?;
        if pat.matches(&anchor_spec) {
            return Some(KnownCombinationOut {
                solana: k.solana.clone(),
                rust: k.rust.clone(),
                platform_tools: k.platform_tools.clone(),
                node: k.node.clone(),
                note: k.note.clone(),
            });
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct KnownCombinationOut {
    pub solana: String,
    pub rust: String,
    pub platform_tools: String,
    pub node: String,
    pub note: String,
}

/// Resolve a requested toolchain to specific recommended versions where gaps
/// exist. Used to suggest fixes when incompatible.
pub fn build_recommendation(req: &ToolchainRequest) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();

    // If anchor known, prefer its known combination; otherwise guess per-tool.
    if let Some(anchor) = &req.anchor {
        if let Some(known) = recommended_for(anchor) {
            map.insert("anchor".into(), anchor.clone());
            map.insert("solana".into(), known.solana);
            map.insert("rust".into(), known.rust);
            map.insert("node".into(), known.node);
            map.insert("platform_tools".into(), known.platform_tools);
            return map;
        }
    }

    // Conservative per-tool defaults.
    if let Some(solana) = &req.solana {
        map.insert("solana".into(), solana.clone());
    }
    if let Some(rust) = &req.rust {
        map.insert("rust".into(), rust.clone());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_loads() {
        let d = Dataset::parse_embedded().unwrap();
        assert!(!d.anchor_solana.is_empty());
        assert!(!d.anchor_rust.is_empty());
        assert!(!d.anchor_node.is_empty());
        assert!(!d.known_combination.is_empty());
        // Every rule must parse as a Spec.
        for r in &d.anchor_solana {
            let _: Spec = r.anchor.parse().unwrap();
        }
    }

    #[test]
    fn compatible_modern_stack() {
        let req = ToolchainRequest {
            rust: Some("1.92.0".into()),
            solana: Some("3.1.10".into()),
            anchor: Some("1.1.2".into()),
            node: Some("22.13.0".into()),
        };
        let v = validate(&req).unwrap();
        assert!(v.is_empty(), "expected no violations, got {v:?}");
    }

    #[test]
    fn incompatible_anchor_solana() {
        let req = ToolchainRequest {
            rust: Some("1.92.0".into()),
            solana: Some("1.18.0".into()),
            anchor: Some("1.1.2".into()),
            node: Some("22.0.0".into()),
        };
        let v = validate(&req).unwrap();
        assert!(!v.is_empty());
        assert!(v.iter().any(|x| x.rule == "anchor_solana"));
    }

    #[test]
    fn incompatible_anchor_rust() {
        let req = ToolchainRequest {
            rust: Some("1.60.0".into()),
            solana: Some("3.1.0".into()),
            anchor: Some("1.1.2".into()),
            node: Some("22.0.0".into()),
        };
        let v = validate(&req).unwrap();
        assert!(v.iter().any(|x| x.rule == "anchor_rust"));
    }

    #[test]
    fn incompatible_anchor_node() {
        let req = ToolchainRequest {
            rust: Some("1.92.0".into()),
            solana: Some("3.1.0".into()),
            anchor: Some("1.1.2".into()),
            node: Some("14.0.0".into()),
        };
        let v = validate(&req).unwrap();
        assert!(v.iter().any(|x| x.rule == "anchor_node"));
    }

    #[test]
    fn legacy_stack_still_valid() {
        let req = ToolchainRequest {
            rust: Some("1.79.0".into()),
            solana: Some("1.18.0".into()),
            anchor: Some("0.30.1".into()),
            node: Some("16.0.0".into()),
        };
        let v = validate(&req).unwrap();
        assert!(v.is_empty(), "expected compatible, got {v:?}");
    }

    #[test]
    fn anchor_032_with_solana_2_1_ok() {
        let req = ToolchainRequest {
            rust: Some("1.84.0".into()),
            solana: Some("2.1.7".into()),
            anchor: Some("0.32.1".into()),
            node: Some("20.0.0".into()),
        };
        let v = validate(&req).unwrap();
        assert!(v.is_empty(), "expected compatible, got {v:?}");
    }

    #[test]
    fn recommended_for_anchor_11() {
        let known = recommended_for("1.1.2").unwrap();
        assert_eq!(known.platform_tools, "v1.52");
        assert!(known.solana.starts_with("3.1"));
    }
}
