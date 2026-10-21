//! The `SkillDocument` model.
//!
//! Design note: `name` and `description` are *projections* of the frontmatter
//! map, not separate fields. This avoids holding two copies of the same value
//! that could drift out of sync. The raw frontmatter text is preserved so an
//! untouched document round-trips byte for byte.

use std::path::{Path, PathBuf};

use crate::domain::resource::SkillResource;
use crate::formats::frontmatter::FrontmatterDocument;
use crate::formats::skill_md::{self, SkillMdError};

/// Stable identifier for a document within a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SkillId(pub String);

impl SkillId {
    /// Identity of an on-disk skill is its canonical root path.
    pub fn from_root(root: &Path) -> Self {
        SkillId(crate::domain::path_safety::to_slash(root))
    }

    pub fn draft(seq: u64) -> Self {
        SkillId(format!("draft:{seq}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SkillId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parse/serialization health of the on-disk document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceState {
    /// Frontmatter parsed successfully.
    pub parse_ok: bool,
    /// A `---` frontmatter block was present.
    pub frontmatter_present: bool,
    /// The document has unsaved edits.
    pub dirty: bool,
    /// Original text kept when parsing failed (raw-text fallback mode).
    pub raw_fallback: Option<String>,
}

impl SourceState {
    /// True when the document is only available as raw text.
    pub fn is_raw(&self) -> bool {
        !self.parse_ok
    }
}

#[derive(Debug, Clone)]
pub struct SkillDocument {
    pub id: SkillId,
    pub root: PathBuf,
    pub frontmatter: FrontmatterDocument,
    pub instructions: String,
    pub resources: Vec<SkillResource>,
    pub state: SourceState,
}

impl SkillDocument {
    /// Load a document from the text of a `SKILL.md`.
    ///
    /// A malformed frontmatter does not fail: the document is returned in
    /// raw-text fallback mode so nothing is lost.
    pub fn from_skill_md(root: PathBuf, content: &str) -> Self {
        let id = SkillId::from_root(&root);
        match skill_md::parse(content) {
            Ok(parsed) => SkillDocument {
                id,
                root,
                frontmatter: parsed.frontmatter,
                instructions: parsed.body,
                resources: Vec::new(),
                state: SourceState {
                    parse_ok: true,
                    frontmatter_present: parsed.had_block,
                    dirty: false,
                    raw_fallback: None,
                },
            },
            Err(err) => SkillDocument {
                id,
                root,
                frontmatter: FrontmatterDocument::absent(),
                instructions: content.to_string(),
                resources: Vec::new(),
                state: SourceState {
                    parse_ok: false,
                    frontmatter_present: false,
                    dirty: false,
                    raw_fallback: Some(content.to_string()),
                },
            }
            .with_parse_error(err),
        }
    }

    fn with_parse_error(mut self, _err: SkillMdError) -> Self {
        // Keep the error out of the model; the validation service re-derives it
        // with a line number from `raw_fallback`.
        self.state.parse_ok = false;
        self
    }

    /// Create an empty draft with a helpful starter body.
    pub fn new_draft(id: SkillId, root: PathBuf, name: &str, description: &str) -> Self {
        let mut frontmatter = FrontmatterDocument::empty();
        frontmatter.set_str("name", name);
        frontmatter.set_str("description", description);
        SkillDocument {
            id,
            root,
            frontmatter,
            instructions: DEFAULT_BODY.to_string(),
            resources: Vec::new(),
            state: SourceState {
                parse_ok: true,
                frontmatter_present: true,
                dirty: true,
                raw_fallback: None,
            },
        }
    }

    pub fn name(&self) -> String {
        self.frontmatter.get_str("name").unwrap_or_default()
    }

    pub fn set_name(&mut self, value: &str) {
        self.frontmatter.set_str("name", value);
    }

    pub fn description(&self) -> String {
        self.frontmatter.get_str("description").unwrap_or_default()
    }

    pub fn set_description(&mut self, value: &str) {
        self.frontmatter.set_str("description", value);
    }

    /// Directory name of the skill root, used for the name/dir match check.
    pub fn dir_name(&self) -> Option<String> {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }

    pub fn set_instructions(&mut self, body: &str) {
        self.instructions = body.to_string();
        self.state.dirty = true;
    }

    pub fn mark_dirty(&mut self) {
        self.state.dirty = true;
    }

    /// Render the full `SKILL.md` content.
    pub fn to_skill_md(&self) -> Result<String, SkillMdError> {
        skill_md::serialize(&self.frontmatter, &self.instructions)
    }
}

/// Default body used for a brand new skill. Kept deliberately small and
/// clearly marked as a starting point, not a required structure.
pub const DEFAULT_BODY: &str = "\
## 何时使用

描述在什么场景下应该使用这个 Skill。

## 步骤

1. 第一步
2. 第二步
3. 第三步

## 注意事项

- 需要明确的边界或反例。
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_description_are_frontmatter_projections() {
        let mut doc = SkillDocument::new_draft(
            SkillId("draft:1".into()),
            PathBuf::from("/tmp/demo"),
            "demo",
            "A demo skill",
        );
        assert_eq!(doc.name(), "demo");
        assert_eq!(doc.description(), "A demo skill");
        doc.set_name("renamed");
        assert_eq!(doc.name(), "renamed");
        // Still a single frontmatter entry, not duplicated.
        assert_eq!(doc.frontmatter.keys().len(), 2);
    }

    #[test]
    fn malformed_frontmatter_keeps_raw_text() {
        let content = "---\nname: [unclosed\n---\nbody";
        let doc = SkillDocument::from_skill_md(PathBuf::from("/tmp/x"), content);
        assert!(!doc.state.parse_ok);
        assert_eq!(doc.state.raw_fallback.as_deref(), Some(content));
    }

    #[test]
    fn dir_name_uses_last_component() {
        let doc = SkillDocument::from_skill_md(PathBuf::from("/a/b/code-review"), "body");
        assert_eq!(doc.dir_name().as_deref(), Some("code-review"));
    }
}
