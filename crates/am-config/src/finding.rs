//! A validation finding: file, 1-based line, severity, message and fix (AD-16).

use std::fmt;

/// Errors make `agentic validate` exit 1; warnings never do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

/// One problem in one project file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Project-relative path with forward slashes, e.g. `workflows/x.yaml`.
    pub file: String,
    /// 1-based line.
    pub line: u32,
    pub severity: Severity,
    pub message: String,
    /// What to do about it, in plain words.
    pub fix: String,
}

impl fmt::Display for Finding {
    /// `file:line: message` (errors) or `file:line: warning: message`, then an
    /// indented `fix:` line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.severity {
            Severity::Error => "",
            Severity::Warning => "warning: ",
        };
        write!(
            f,
            "{}:{}: {label}{}\n  fix: {}",
            self.file, self.line, self.message, self.fix
        )
    }
}

/// The findings of one file, collected while it is read.
pub(crate) struct Report {
    pub file: String,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn new(file: &str) -> Report {
        Report {
            file: file.to_string(),
            findings: Vec::new(),
        }
    }

    pub fn error(&mut self, line: u32, message: impl Into<String>, fix: impl Into<String>) {
        self.push(Severity::Error, line, message.into(), fix.into());
    }

    pub fn warning(&mut self, line: u32, message: impl Into<String>, fix: impl Into<String>) {
        self.push(Severity::Warning, line, message.into(), fix.into());
    }

    fn push(&mut self, severity: Severity, line: u32, message: String, fix: String) {
        self.findings.push(Finding {
            file: self.file.clone(),
            line: line.max(1),
            severity,
            message,
            fix,
        });
    }
}
