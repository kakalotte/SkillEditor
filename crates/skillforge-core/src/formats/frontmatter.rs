//! YAML frontmatter that preserves unknown fields and key order.
//!
//! We keep two representations:
//! * a parsed `Mapping` used for reads and edits, and
//! * the original YAML text.
//!
//! When nothing was edited we return the original text verbatim so comments
//! and formatting survive. When a field is changed we re-serialize the map,
//! which preserves key order because `serde_yaml_ng::Mapping` is ordered.

use serde_yaml_ng::{Mapping, Value};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FrontmatterError {
    #[error("frontmatter 必须是一个 YAML 映射（键值对），当前是其他类型")]
    NotMapping,
    #[error("YAML 解析失败（第 {line} 行）: {message}")]
    Yaml { line: usize, message: String },
    #[error("frontmatter 序列化失败: {0}")]
    Serialize(String),
}

#[derive(Debug, Clone)]
pub struct FrontmatterDocument {
    map: Mapping,
    /// Original YAML text between the `---` markers.
    raw: String,
    present: bool,
    dirty: bool,
}

impl Default for FrontmatterDocument {
    fn default() -> Self {
        Self::absent()
    }
}

impl FrontmatterDocument {
    /// No frontmatter block at all.
    pub fn absent() -> Self {
        FrontmatterDocument {
            map: Mapping::new(),
            raw: String::new(),
            present: false,
            dirty: false,
        }
    }

    /// An empty but present frontmatter block.
    pub fn empty() -> Self {
        FrontmatterDocument {
            map: Mapping::new(),
            raw: String::new(),
            present: true,
            dirty: true,
        }
    }

    /// Parse YAML text. `base_line` is the 1-based file line of the first YAML
    /// line, so errors can point at the right file line.
    pub fn parse_yaml(raw: &str, base_line: usize) -> Result<Self, FrontmatterError> {
        if raw.trim().is_empty() {
            return Ok(FrontmatterDocument {
                map: Mapping::new(),
                raw: raw.to_string(),
                present: true,
                dirty: false,
            });
        }

        let value: Value = serde_yaml_ng::from_str(raw).map_err(|err| {
            let line = err
                .location()
                .map(|loc| base_line + loc.line().saturating_sub(1))
                .unwrap_or(base_line);
            FrontmatterError::Yaml {
                line,
                message: err.to_string().replace('\n', " "),
            }
        })?;

        let map = match value {
            Value::Mapping(map) => map,
            Value::Null => Mapping::new(),
            _ => return Err(FrontmatterError::NotMapping),
        };

        Ok(FrontmatterDocument {
            map,
            raw: raw.to_string(),
            present: true,
            dirty: false,
        })
    }

    pub fn is_present(&self) -> bool {
        self.present
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn map(&self) -> &Mapping {
        &self.map
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(Value::String(key.to_string()))
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(Value::String(key.to_string()))
    }

    /// Read a scalar as a string (bools/numbers are rendered, collections are
    /// not).
    pub fn get_str(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Number(n) => Some(n.to_string()),
            Value::Null => None,
            _ => None,
        }
    }

    /// Read a scalar as a string, including stringified collections.
    pub fn get_str_lossy(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Number(n) => Some(n.to_string()),
            Value::Null => None,
            Value::Sequence(seq) => Some(
                seq.iter()
                    .filter_map(scalar_to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            Value::Mapping(_) => None,
            Value::Tagged(t) => scalar_to_string(&t.value),
        }
    }

    /// Insert or replace a string field. Marks the document dirty.
    pub fn set_str(&mut self, key: &str, value: &str) {
        self.map.insert(
            Value::String(key.to_string()),
            Value::String(value.to_string()),
        );
        self.present = true;
        self.dirty = true;
    }

    /// Insert or replace an arbitrary YAML value.
    pub fn set_value(&mut self, key: &str, value: Value) {
        self.map.insert(Value::String(key.to_string()), value);
        self.present = true;
        self.dirty = true;
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let removed = self.map.remove(Value::String(key.to_string())).is_some();
        if removed {
            self.dirty = true;
        }
        removed
    }

    /// Top-level keys, in document order.
    pub fn keys(&self) -> Vec<String> {
        self.map
            .keys()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect()
    }

    /// Serialize the frontmatter body (without `---` markers).
    pub fn to_yaml_string(&self) -> Result<String, FrontmatterError> {
        if !self.present {
            return Ok(String::new());
        }
        if !self.dirty {
            return Ok(self.raw.clone());
        }
        if self.map.is_empty() {
            return Ok(String::new());
        }
        let mut text = serde_yaml_ng::to_string(&self.map)
            .map_err(|e| FrontmatterError::Serialize(e.to_string()))?;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        Ok(text)
    }
}

fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unknown_fields_and_order() {
        let raw = "name: demo\nx-custom: 42\nnested:\n  a: b\n";
        let doc = FrontmatterDocument::parse_yaml(raw, 2).unwrap();
        assert_eq!(doc.keys(), vec!["name", "x-custom", "nested"]);
        // Untouched -> byte-identical.
        assert_eq!(doc.to_yaml_string().unwrap(), raw);
        assert!(doc.contains_key("x-custom"));
    }

    #[test]
    fn edits_keep_order_and_unknown_fields() {
        let raw = "name: demo\nx-custom: 42\ndescription: old\n";
        let mut doc = FrontmatterDocument::parse_yaml(raw, 2).unwrap();
        doc.set_str("description", "new");
        let out = doc.to_yaml_string().unwrap();
        assert!(out.contains("x-custom: 42"));
        let reparsed = FrontmatterDocument::parse_yaml(&out, 2).unwrap();
        assert_eq!(reparsed.keys(), vec!["name", "x-custom", "description"]);
        assert_eq!(reparsed.get_str("description").as_deref(), Some("new"));
        assert_eq!(reparsed.get_str("x-custom").as_deref(), Some("42"));
    }

    #[test]
    fn reports_yaml_line_numbers() {
        let err = FrontmatterDocument::parse_yaml("name: [unclosed\n", 2).unwrap_err();
        match err {
            FrontmatterError::Yaml { line, .. } => assert!(line >= 2),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn non_mapping_frontmatter_is_rejected() {
        let err = FrontmatterDocument::parse_yaml("- a\n- b\n", 2).unwrap_err();
        assert_eq!(err, FrontmatterError::NotMapping);
    }

    #[test]
    fn empty_frontmatter_is_empty_mapping() {
        let doc = FrontmatterDocument::parse_yaml("", 2).unwrap();
        assert!(doc.is_present());
        assert!(doc.keys().is_empty());
    }
}
