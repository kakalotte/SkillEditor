//! Lightweight Markdown structure analysis used for preview and quality
//! suggestions. It never rewrites the source; it only counts features.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkdownFacts {
    pub lines: usize,
    pub characters: usize,
    pub headings: usize,
    pub list_items: usize,
    pub ordered_items: usize,
    pub code_fences: usize,
    pub links: usize,
    /// True when the body contains an ordered list or numbered steps.
    pub has_steps: bool,
    /// True when the body contains any code fence or inline example.
    pub has_examples: bool,
}

impl MarkdownFacts {
    /// Rough token estimate (characters / 4). Intended only as a heuristic hint.
    pub fn approx_tokens(&self) -> usize {
        self.characters / 4
    }
}

/// Compute structural facts for a Markdown body.
pub fn analyze(body: &str) -> MarkdownFacts {
    let mut facts = MarkdownFacts {
        characters: body.chars().count(),
        ..Default::default()
    };
    let mut in_fence = false;
    for raw in body.lines() {
        facts.lines += 1;
        let line = raw.trim_start();
        if line.starts_with("```") || line.starts_with("~~~") {
            facts.code_fences += 1;
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if line.starts_with('#') {
            facts.headings += 1;
            continue;
        }
        if is_ordered_item(line) {
            facts.ordered_items += 1;
            facts.list_items += 1;
            facts.has_steps = true;
            continue;
        }
        if is_unordered_item(line) {
            facts.list_items += 1;
            continue;
        }
    }
    facts.has_examples =
        facts.code_fences >= 2 || body.contains("例如") || body.contains("Example");
    facts.links = count_links(body);
    facts
}

fn is_ordered_item(line: &str) -> bool {
    let mut chars = line.chars();
    let mut digits = 0;
    for ch in chars.by_ref() {
        if ch.is_ascii_digit() {
            digits += 1;
        } else {
            break;
        }
    }
    if digits == 0 {
        return false;
    }
    line[digits..].starts_with(". ") || line[digits..].starts_with(") ")
}

fn is_unordered_item(line: &str) -> bool {
    line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ")
}

fn count_links(body: &str) -> usize {
    // Count `[text](target)` occurrences without a regex dependency.
    let bytes = body.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(close) = body[i..].find(']') {
                let after = i + close + 1;
                if body[after..].starts_with('(') && body[after..].contains(')') {
                    count += 1;
                }
                i = after;
                continue;
            }
        }
        i += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_headings_and_steps() {
        let body = "# Title\n\n## Steps\n\n1. one\n2. two\n\n- bullet\n";
        let facts = analyze(body);
        assert_eq!(facts.headings, 2);
        assert_eq!(facts.ordered_items, 2);
        assert_eq!(facts.list_items, 3);
        assert!(facts.has_steps);
    }

    #[test]
    fn ignores_markers_inside_code_fences() {
        let body = "```\n# not a heading\n1. not a step\n```\n";
        let facts = analyze(body);
        assert_eq!(facts.headings, 0);
        assert_eq!(facts.ordered_items, 0);
        assert_eq!(facts.code_fences, 2);
    }

    #[test]
    fn counts_links() {
        assert_eq!(count_links("see [a](b) and [c](d)"), 2);
        assert_eq!(count_links("not a [link"), 0);
    }
}
