//! Skill `name` validation and kebab-case suggestion.
//!
//! Rules come from the Agent Skills specification (agentskills.io/specification):
//! 1-64 characters, lowercase alphanumerics and hyphens only, no leading or
//! trailing hyphen, no consecutive hyphens. The name must match the parent
//! directory name.

use thiserror::Error;

/// Maximum length of a skill name (Agent Skills spec).
pub const MAX_NAME_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NameError {
    #[error("名称不能为空")]
    Empty,
    #[error("名称长度不能超过 {MAX_NAME_LEN} 个字符（当前 {0}）")]
    TooLong(usize),
    #[error("名称只能包含小写字母、数字和连字符")]
    InvalidChars,
    #[error("名称不能以连字符开头或结尾")]
    HyphenBoundary,
    #[error("名称不能包含连续的连字符 `--`")]
    ConsecutiveHyphens,
}

/// Validate a skill name against the Agent Skills rules.
pub fn validate_skill_name(name: &str) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    let count = name.chars().count();
    if count > MAX_NAME_LEN {
        return Err(NameError::TooLong(count));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(NameError::HyphenBoundary);
    }
    if name.contains("--") {
        return Err(NameError::ConsecutiveHyphens);
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(NameError::InvalidChars);
    }
    Ok(())
}

/// Quick boolean form of the name check.
pub fn is_valid_skill_name(name: &str) -> bool {
    validate_skill_name(name).is_ok()
}

/// Suggest a kebab-case name from free-form text.
///
/// Non-ASCII characters (for example Chinese) are dropped because the spec
/// only allows `[a-z0-9-]`. When nothing remains the caller should ask the user
/// to type a name rather than inventing one.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_hyphen = false;
    for ch in input.chars() {
        let mapped = ch.to_ascii_lowercase();
        if mapped.is_ascii_lowercase() || mapped.is_ascii_digit() {
            out.push(mapped);
            last_hyphen = false;
        } else if !last_hyphen && !out.is_empty() {
            out.push('-');
            last_hyphen = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.chars().count() > MAX_NAME_LEN {
        trimmed
            .chars()
            .take(MAX_NAME_LEN)
            .collect::<String>()
            .trim_end_matches('-')
            .to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names() {
        for name in ["deploy", "code-review", "a1-b2-c3", "x"] {
            assert!(is_valid_skill_name(name), "{name} should be valid");
        }
    }

    #[test]
    fn rejects_invalid_names() {
        assert_eq!(validate_skill_name(""), Err(NameError::Empty));
        assert!(matches!(
            validate_skill_name("PDF-Processing"),
            Err(NameError::InvalidChars)
        ));
        assert!(matches!(
            validate_skill_name("-pdf"),
            Err(NameError::HyphenBoundary)
        ));
        assert!(matches!(
            validate_skill_name("pdf-"),
            Err(NameError::HyphenBoundary)
        ));
        assert!(matches!(
            validate_skill_name("pdf--processing"),
            Err(NameError::ConsecutiveHyphens)
        ));
        assert!(matches!(
            validate_skill_name("has space"),
            Err(NameError::InvalidChars)
        ));
    }

    #[test]
    fn rejects_overlong_names() {
        let long = "a".repeat(MAX_NAME_LEN + 1);
        assert!(matches!(
            validate_skill_name(&long),
            Err(NameError::TooLong(_))
        ));
    }

    #[test]
    fn slugifies_free_text() {
        assert_eq!(slugify("My Great Skill!"), "my-great-skill");
        assert_eq!(slugify("  Deploy   To Prod  "), "deploy-to-prod");
        assert_eq!(slugify("a--b"), "a-b");
        assert_eq!(slugify("代码审查"), "");
    }
}
