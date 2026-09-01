//! Error types and user-facing diagnostics.

use std::fmt;

/// Kinds of failure, used to select actionable guidance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Config,
    Network,
    Checksum,
    NotInstalled,
    Compat,
    Permissions,
    Exists,
    Bug,
}

/// A user-friendly error carrying path to fix.
#[derive(Debug)]
pub struct SolenvError {
    pub kind: ErrorKind,
    /// What went wrong.
    pub what: String,
    /// Why it likely happened.
    pub why: Option<String>,
    /// How to fix it.
    pub fix: Option<String>,
}

impl SolenvError {
    pub fn new(kind: ErrorKind, what: impl Into<String>) -> Self {
        SolenvError {
            kind,
            what: what.into(),
            why: None,
            fix: None,
        }
    }

    pub fn with_why(mut self, why: impl Into<String>) -> Self {
        self.why = Some(why.into());
        self
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

impl fmt::Display for SolenvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.what)
    }
}

impl std::error::Error for SolenvError {}

/// Render an error to multi-line, actionable text (used by main).
pub fn render(err: &anyhow::Error) -> String {
    if let Some(solenv_err) = err.downcast_ref::<SolenvError>() {
        let mut out = String::new();
        out.push_str("Error: ");
        out.push_str(&solenv_err.what);
        if let Some(why) = &solenv_err.why {
            out.push_str("\nWhy: ");
            out.push_str(why);
        }
        if let Some(fix) = &solenv_err.fix {
            out.push_str("\nFix: ");
            out.push_str(fix);
        }
        return out;
    }
    // Fallback: anyhow chain, compact.
    let mut out = format!("Error: {}", err);
    for cause in err.chain().skip(1) {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
    }
    out
}
