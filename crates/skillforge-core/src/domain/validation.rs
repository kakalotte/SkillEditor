//! Validation issue model and report aggregation.
//!
//! Every issue carries a stable rule id, a severity, the origin of the rule
//! (generic spec / platform limitation / quality suggestion) plus an optional
//! file and line so the UI can point at the exact place to fix.

use serde::{Deserialize, Serialize};

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Severity::Error => "错误",
            Severity::Warning => "警告",
            Severity::Info => "建议",
        }
    }
}

/// Where a rule comes from. Kept distinct from severity so quality advice is
/// never conflated with a hard specification error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueSource {
    /// Agent Skills open specification.
    Spec,
    /// Quality suggestion (not a spec requirement).
    Quality,
    /// Limitation of a specific target platform.
    Platform(String),
}

impl IssueSource {
    pub fn key(&self) -> String {
        match self {
            IssueSource::Spec => "spec".to_string(),
            IssueSource::Quality => "quality".to_string(),
            IssueSource::Platform(id) => format!("platform:{id}"),
        }
    }

    pub fn label_zh(&self) -> String {
        match self {
            IssueSource::Spec => "通用规范".to_string(),
            IssueSource::Quality => "质量建议".to_string(),
            IssueSource::Platform(id) => format!("平台:{id}"),
        }
    }

    pub fn is_platform(&self) -> bool {
        matches!(self, IssueSource::Platform(_))
    }
}

/// A single validation finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub rule_id: String,
    pub severity: Severity,
    pub source: IssueSource,
    /// Path relative to the skill root, e.g. `SKILL.md`.
    pub file: String,
    /// 1-based line number when known.
    pub line: Option<usize>,
    pub message: String,
    pub suggestion: String,
    pub doc_url: Option<String>,
}

impl ValidationIssue {
    pub fn new(
        rule_id: impl Into<String>,
        severity: Severity,
        source: IssueSource,
        file: impl Into<String>,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        ValidationIssue {
            rule_id: rule_id.into(),
            severity,
            source,
            file: file.into(),
            line: None,
            message: message.into(),
            suggestion: suggestion.into(),
            doc_url: None,
        }
    }

    pub fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    pub fn with_doc(mut self, url: impl Into<String>) -> Self {
        self.doc_url = Some(url.into());
        self
    }
}

/// An ordered collection of issues plus convenience counters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }

    pub fn extend(&mut self, issues: impl IntoIterator<Item = ValidationIssue>) {
        self.issues.extend(issues);
    }

    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    pub fn infos(&self) -> usize {
        self.count(Severity::Info)
    }

    pub fn count(&self, severity: Severity) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == severity)
            .count()
    }

    pub fn has_errors(&self) -> bool {
        self.errors() > 0
    }

    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// Issues sorted by severity (errors first), then by rule id.
    pub fn sorted(&self) -> Vec<ValidationIssue> {
        let mut out = self.issues.clone();
        out.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.rule_id.cmp(&b.rule_id))
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_by_severity() {
        let mut report = ValidationReport::new();
        report.push(ValidationIssue::new(
            "SPEC001",
            Severity::Error,
            IssueSource::Spec,
            "SKILL.md",
            "m",
            "s",
        ));
        report.push(ValidationIssue::new(
            "QUAL001",
            Severity::Info,
            IssueSource::Quality,
            "SKILL.md",
            "m",
            "s",
        ));
        report.push(ValidationIssue::new(
            "QUAL002",
            Severity::Info,
            IssueSource::Quality,
            "SKILL.md",
            "m",
            "s",
        ));
        assert_eq!(report.errors(), 1);
        assert_eq!(report.warnings(), 0);
        assert_eq!(report.infos(), 2);
        assert!(report.has_errors());
    }

    #[test]
    fn sorts_errors_first() {
        let mut report = ValidationReport::new();
        report.push(ValidationIssue::new(
            "B",
            Severity::Info,
            IssueSource::Quality,
            "SKILL.md",
            "m",
            "s",
        ));
        report.push(ValidationIssue::new(
            "A",
            Severity::Error,
            IssueSource::Spec,
            "SKILL.md",
            "m",
            "s",
        ));
        let sorted = report.sorted();
        assert_eq!(sorted[0].severity, Severity::Error);
    }
}
