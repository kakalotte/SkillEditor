//! Parsing and serialization of `SKILL.md`.
//!
//! The format is: an optional YAML frontmatter block delimited by `---` on its
//! own line, followed by a Markdown body. The delimiter scan is done line by
//! line (never by naive string splitting) so that YAML content containing
//! `---` is not mistaken for the end of the block.

use thiserror::Error;

use super::frontmatter::{FrontmatterDocument, FrontmatterError};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SkillMdError {
    #[error("{0}")]
    Frontmatter(#[from] FrontmatterError),
}

#[derive(Debug, Clone)]
pub struct ParsedSkillMd {
    pub frontmatter: FrontmatterDocument,
    pub body: String,
    /// Whether a `---` frontmatter block was present.
    pub had_block: bool,
}

/// Parse the contents of a `SKILL.md`.
pub fn parse(content: &str) -> Result<ParsedSkillMd, SkillMdError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);

    match split_frontmatter(content) {
        Some((yaml_range, body_range)) => {
            let yaml = &content[yaml_range];
            // The opening `---` is line 1, so YAML starts on line 2.
            let frontmatter = FrontmatterDocument::parse_yaml(yaml, 2)?;
            Ok(ParsedSkillMd {
                frontmatter,
                body: content[body_range].to_string(),
                had_block: true,
            })
        }
        None => Ok(ParsedSkillMd {
            frontmatter: FrontmatterDocument::absent(),
            body: content.to_string(),
            had_block: false,
        }),
    }
}

/// Serialize frontmatter + body back to `SKILL.md` text.
///
/// The line-ending style of the source document is preserved for the fences
/// and the YAML block: a document that used CRLF keeps CRLF. This matters on
/// Windows, where editors commonly write CRLF, and it keeps an untouched
/// document byte-for-byte identical. The body is written as provided.
pub fn serialize(frontmatter: &FrontmatterDocument, body: &str) -> Result<String, SkillMdError> {
    if !frontmatter.is_present() {
        return Ok(body.to_string());
    }
    let newline = detected_newline(frontmatter, body);
    let yaml = normalize_newlines(&frontmatter.to_yaml_string()?, newline);
    let mut out = String::with_capacity(yaml.len() + body.len() + 8);
    out.push_str("---");
    out.push_str(newline);
    out.push_str(&yaml);
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        out.push_str(newline);
    }
    out.push_str("---");
    out.push_str(newline);
    out.push_str(body);
    Ok(out)
}

/// Use the style found in the source: frontmatter first, then the body.
fn detected_newline(frontmatter: &FrontmatterDocument, body: &str) -> &'static str {
    let sample = if frontmatter.raw().is_empty() {
        body
    } else {
        frontmatter.raw()
    };
    if sample.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Normalize every line break to `newline`, leaving lone `\r` bytes alone.
fn normalize_newlines(text: &str, newline: &str) -> String {
    if newline == "\n" {
        return text.replace("\r\n", "\n");
    }
    text.replace("\r\n", "\n").replace('\n', newline)
}

/// Byte ranges of the YAML text and the body, for callers that rewrite only
/// the frontmatter without re-serializing it.
///
/// Offsets are relative to `content` (including a leading BOM, when present).
/// Returns `None` when the file does not begin with a `---` line or the block
/// is never closed.
pub fn frontmatter_ranges(
    content: &str,
) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let stripped = content.strip_prefix('\u{feff}').unwrap_or(content);
    let offset = content.len() - stripped.len();
    let (yaml, body) = split_frontmatter(stripped)?;
    Some((
        yaml.start + offset..yaml.end + offset,
        body.start + offset..body.end + offset,
    ))
}

/// Locate the frontmatter and body byte ranges.
///
/// Returns `None` when the file does not begin with a `---` line or the block
/// is never closed.
fn split_frontmatter(content: &str) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let (first, next) = read_line(content, 0);
    if !is_fence(first) {
        return None;
    }

    let yaml_start = next;
    let mut cursor = next;
    loop {
        if cursor >= content.len() {
            // Unterminated frontmatter.
            return None;
        }
        let (line, next_cursor) = read_line(content, cursor);
        if is_fence(line) {
            let yaml_end = cursor;
            let body_start = next_cursor;
            return Some((yaml_start..yaml_end, body_start..content.len()));
        }
        if next_cursor <= cursor {
            return None;
        }
        cursor = next_cursor;
    }
}

/// True when a line is a frontmatter fence (`---` or `...`) at column 0.
///
/// Trailing whitespace is tolerated, but leading whitespace is not: an indented
/// `  ---` is raw YAML content (e.g. inside a block scalar) and must not close
/// the block.
fn is_fence(line: &str) -> bool {
    let s = line.trim_end_matches(['\n', '\r']);
    s == "---" || s == "..."
}

/// Read one line (including its trailing `\n` when present) starting at `from`.
fn read_line(content: &str, from: usize) -> (&str, usize) {
    match content[from..].find('\n') {
        Some(offset) => {
            let end = from + offset + 1;
            (&content[from..end], end)
        }
        None => (&content[from..], content.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let content = "---\nname: demo\ndescription: A demo\n---\n\n# Title\n\nBody\n";
        let parsed = parse(content).unwrap();
        assert!(parsed.had_block);
        assert_eq!(parsed.frontmatter.get_str("name").as_deref(), Some("demo"));
        assert_eq!(parsed.body, "\n# Title\n\nBody\n");
    }

    #[test]
    fn document_without_frontmatter_is_all_body() {
        let parsed = parse("# Just a body\n").unwrap();
        assert!(!parsed.had_block);
        assert_eq!(parsed.body, "# Just a body\n");
    }

    #[test]
    fn round_trips_byte_for_byte() {
        let content = "---\nname: demo\nx-extra: keep\n---\n\nBody line 1\nBody line 2\n";
        let parsed = parse(content).unwrap();
        let out = serialize(&parsed.frontmatter, &parsed.body).unwrap();
        assert_eq!(out, content);
    }

    #[test]
    fn round_trips_without_frontmatter() {
        let content = "# No frontmatter here\n";
        let parsed = parse(content).unwrap();
        let out = serialize(&parsed.frontmatter, &parsed.body).unwrap();
        assert_eq!(out, content);
    }

    #[test]
    fn yaml_containing_dashes_is_not_truncated() {
        // A description that itself contains a `---` line must not end the block.
        let content = "---\nname: demo\ndescription: |\n  a\n  ---\n  b\n---\nbody\n";
        let parsed = parse(content).unwrap();
        assert_eq!(parsed.body, "body\n");
        let out = serialize(&parsed.frontmatter, &parsed.body).unwrap();
        assert_eq!(out, content);
    }

    #[test]
    fn unterminated_frontmatter_is_treated_as_body() {
        let content = "---\nname: demo\n";
        let parsed = parse(content).unwrap();
        assert!(!parsed.had_block);
        assert_eq!(parsed.body, content);
    }

    #[test]
    fn bom_is_stripped() {
        let content = "\u{feff}---\nname: demo\n---\nbody";
        let parsed = parse(content).unwrap();
        assert!(parsed.had_block);
        assert_eq!(parsed.frontmatter.get_str("name").as_deref(), Some("demo"));
    }

    #[test]
    fn crlf_lines_are_handled() {
        let content = "---\r\nname: demo\r\n---\r\nbody";
        let parsed = parse(content).unwrap();
        assert!(parsed.had_block);
        assert_eq!(parsed.frontmatter.get_str("name").as_deref(), Some("demo"));
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn crlf_document_round_trips_byte_for_byte() {
        // Windows checkouts and editors commonly produce CRLF. The untouched
        // document must survive parse + serialize without mixed line endings.
        let content = "---\r\nname: demo\r\ndescription: 中文\r\n---\r\n\r\n# 标题\r\n\r\n正文\r\n";
        let parsed = parse(content).unwrap();
        let out = serialize(&parsed.frontmatter, &parsed.body).unwrap();
        assert_eq!(out, content);
    }

    #[test]
    fn crlf_style_survives_a_frontmatter_edit() {
        let content = "---\r\nname: demo\r\ndescription: old\r\n---\r\nbody\r\n";
        let mut parsed = parse(content).unwrap();
        parsed.frontmatter.set_str("description", "new");
        let out = serialize(&parsed.frontmatter, &parsed.body).unwrap();
        assert!(out.starts_with("---\r\n"), "{out:?}");
        assert!(
            !out.replace("\r\n", "").contains('\n'),
            "mixed line endings: {out:?}"
        );
    }
}
