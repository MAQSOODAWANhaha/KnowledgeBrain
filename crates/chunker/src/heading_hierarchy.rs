//! Markdown heading stack (brain `heading_hierarchy.go`).

use crate::patterns::MARKDOWN_HEADING;

#[derive(Debug, Clone, Default)]
pub struct HeadingHierarchy {
    stack: [String; 6],
    depth: usize,
}

impl HeadingHierarchy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, line: &str) -> (usize, String) {
        let Some(caps) = MARKDOWN_HEADING.captures(line) else {
            return (0, String::new());
        };
        let level = caps.get(1).map(|m| m.as_str().len()).unwrap_or(0);
        if !(1..=6).contains(&level) {
            return (0, String::new());
        }
        let heading = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        self.stack[level - 1] = heading.clone();
        for i in level..6 {
            self.stack[i].clear();
        }
        if level > self.depth {
            self.depth = level;
        } else {
            self.depth = 0;
            for i in 0..6 {
                if !self.stack[i].is_empty() {
                    self.depth = i + 1;
                }
            }
        }
        (level, heading)
    }

    pub fn breadcrumb_with_hashes(&self) -> String {
        if self.depth == 0 {
            return String::new();
        }
        let mut parts = Vec::new();
        for i in 0..self.depth {
            if self.stack[i].is_empty() {
                continue;
            }
            parts.push(format!("{} {}", "#".repeat(i + 1), self.stack[i]));
        }
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_nesting_and_pop() {
        let mut h = HeadingHierarchy::new();
        h.observe("# Top");
        h.observe("## Section");
        h.observe("### Sub");
        assert_eq!(h.breadcrumb_with_hashes(), "# Top\n## Section\n### Sub");
        h.observe("## Sibling");
        assert_eq!(h.breadcrumb_with_hashes(), "# Top\n## Sibling");
        h.observe("# Other");
        assert_eq!(h.breadcrumb_with_hashes(), "# Other");
        assert_eq!(h.observe("plain"), (0, String::new()));
    }
}
