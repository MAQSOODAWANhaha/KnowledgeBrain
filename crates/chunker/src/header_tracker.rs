//! Table-header context across split units (brain `header_tracker.go`).

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const MARKDOWN_TABLE_HOOK_PRIORITY: i32 = 15;

static TABLE_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?si)^\s*(?:\|[^|\n]*)+[\r\n]+\s*(?:\|\s*:?-{3,}:?\s*)+\|?[\r\n]+$")
        .expect("tbl-start")
});

static TABLE_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?si)^\s*$|^\s*[^|\s].*$").expect("tbl-end"));

static TABLE_ROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*(?:\|[^|\n]*)+\|\s*$").expect("tbl-row"));

pub struct HeaderTracker {
    active_headers: HashMap<i32, String>,
    ended_headers: HashSet<i32>,
    pending_extend: HashSet<i32>,
    pending_table_break: bool,
    pub header_ended_this_unit: bool,
}

impl HeaderTracker {
    pub fn new() -> Self {
        Self {
            active_headers: HashMap::new(),
            ended_headers: HashSet::new(),
            pending_extend: HashSet::new(),
            pending_table_break: false,
            header_ended_this_unit: false,
        }
    }

    pub fn update(&mut self, split: &str) {
        self.header_ended_this_unit = false;

        if self.pending_table_break {
            self.pending_table_break = false;
            if self
                .active_headers
                .contains_key(&MARKDOWN_TABLE_HOOK_PRIORITY)
            {
                if first_table_row_column_count(split) > 0 {
                    self.clear_table_header();
                    self.header_ended_this_unit = true;
                } else {
                    self.clear_table_header();
                }
            }
        }

        if self
            .active_headers
            .contains_key(&MARKDOWN_TABLE_HOOK_PRIORITY)
            && TABLE_END.is_match(split)
        {
            self.ended_headers.insert(MARKDOWN_TABLE_HOOK_PRIORITY);
            self.active_headers.remove(&MARKDOWN_TABLE_HOOK_PRIORITY);
            self.pending_extend.remove(&MARKDOWN_TABLE_HOOK_PRIORITY);
        }

        if self
            .active_headers
            .contains_key(&MARKDOWN_TABLE_HOOK_PRIORITY)
            && !self.pending_extend.contains(&MARKDOWN_TABLE_HOOK_PRIORITY)
        {
            if split_ends_with_paragraph_break(split) {
                self.pending_table_break = true;
            } else {
                self.end_table_header_on_column_mismatch(split);
            }
        }

        let pending: Vec<i32> = self.pending_extend.iter().copied().collect();
        for p in pending {
            if self.active_headers.contains_key(&p) && TABLE_ROW.is_match(split) {
                let sep = extract_separator_line(
                    self.active_headers
                        .get(&p)
                        .map(|s| s.as_str())
                        .unwrap_or(""),
                );
                self.active_headers.insert(p, format!("{split}{sep}"));
            }
            self.pending_extend.remove(&p);
        }

        if !self
            .active_headers
            .contains_key(&MARKDOWN_TABLE_HOOK_PRIORITY)
            && !self.ended_headers.contains(&MARKDOWN_TABLE_HOOK_PRIORITY)
            && let Some(m) = TABLE_START.find(split)
        {
            let loc = m.as_str().to_string();
            if is_empty_table_header_row(&loc) {
                self.pending_extend.insert(MARKDOWN_TABLE_HOOK_PRIORITY);
            }
            self.active_headers
                .insert(MARKDOWN_TABLE_HOOK_PRIORITY, loc);
        }

        if self.active_headers.is_empty() {
            self.ended_headers.clear();
        }
    }

    pub fn get_headers(&self) -> String {
        if self.active_headers.is_empty() {
            return String::new();
        }
        let mut entries: Vec<(i32, &String)> =
            self.active_headers.iter().map(|(p, t)| (*p, t)).collect();
        entries.sort_by_key(|b| std::cmp::Reverse(b.0));
        entries
            .into_iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn clear_table_header(&mut self) {
        self.ended_headers.insert(MARKDOWN_TABLE_HOOK_PRIORITY);
        self.active_headers.remove(&MARKDOWN_TABLE_HOOK_PRIORITY);
        self.pending_extend.remove(&MARKDOWN_TABLE_HOOK_PRIORITY);
    }

    fn end_table_header_on_column_mismatch(&mut self, split: &str) {
        let Some(header) = self.active_headers.get(&MARKDOWN_TABLE_HOOK_PRIORITY) else {
            return;
        };
        let row_cols = first_table_row_column_count(split);
        let header_cols = header_table_column_count(header);
        if row_cols > 0 && header_cols > 0 && row_cols != header_cols {
            self.clear_table_header();
            self.header_ended_this_unit = true;
        }
    }
}

fn is_empty_table_header_row(header: &str) -> bool {
    let Some(idx) = header.find('\n') else {
        return false;
    };
    let row = header[..idx].trim();
    row.chars().all(|r| r == '|' || r == ' ' || r == '\t')
}

fn extract_separator_line(header: &str) -> String {
    for line in header.split('\n') {
        if line.contains("---") {
            return format!("{line}\n");
        }
    }
    String::new()
}

fn split_ends_with_paragraph_break(split: &str) -> bool {
    let trimmed = split.trim_end_matches([' ', '\t', '\r']);
    trimmed.ends_with("\n\n") || trimmed.ends_with("\r\n\r\n")
}

fn table_row_column_count(line: &str) -> usize {
    let line = line.trim();
    if !line.starts_with('|') {
        return 0;
    }
    let mut parts: Vec<&str> = line.split('|').collect();
    if parts.first().is_some_and(|p| p.trim().is_empty()) {
        parts.remove(0);
    }
    if parts.last().is_some_and(|p| p.trim().is_empty()) {
        parts.pop();
    }
    parts.len()
}

fn first_table_row_column_count(text: &str) -> usize {
    for line in text.split('\n') {
        let t = line.trim();
        if !t.is_empty() && TABLE_ROW.is_match(line) {
            return table_row_column_count(line);
        }
    }
    0
}

fn header_table_column_count(header: &str) -> usize {
    for line in header.split('\n') {
        let t = line.trim();
        if t.is_empty() || t.contains("---") {
            continue;
        }
        let n = table_row_column_count(line);
        if n > 0 {
            return n;
        }
    }
    0
}

pub fn header_already_present(headers: &str, overlap_text: &str, unit_text: &str) -> bool {
    if overlap_text.contains(headers) || unit_text.contains(headers) {
        return true;
    }
    let col_row = header_column_row(headers);
    if col_row.is_empty() {
        return false;
    }
    overlap_text.contains(&col_row) || unit_text.contains(&col_row)
}

pub fn header_column_row(header: &str) -> String {
    for line in header.split('\n') {
        let line = line.trim();
        if line.is_empty() || line.contains("---") {
            continue;
        }
        if line.chars().any(|r| r != '|' && r != ' ' && r != '\t') {
            return line.to_string();
        }
    }
    String::new()
}

pub fn header_column_mismatch(headers: &str, next_unit: &str) -> bool {
    let header_cols = header_table_column_count(headers);
    let row_cols = first_table_row_column_count(next_unit);
    header_cols > 0 && row_cols > 0 && header_cols != row_cols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_lifecycle() {
        let mut ht = HeaderTracker::new();
        ht.update("Some regular text");
        assert!(ht.get_headers().is_empty());
        ht.update("| A | B |\n| --- | --- |\n");
        assert!(!ht.get_headers().is_empty());
        ht.update("| 1 | 2 |\n");
        assert!(!ht.get_headers().is_empty());
        ht.update("\n");
        assert!(ht.get_headers().is_empty());
        ht.update("| X | Y |\n| --- | --- |\n");
        assert!(!ht.get_headers().is_empty());
    }

    #[test]
    fn empty_header_row_rewrite() {
        let mut ht = HeaderTracker::new();
        ht.update("||\n| --- | --- |\n");
        ht.update("| real col A | real col B |\n");
        let h = ht.get_headers();
        assert!(h.contains("real col A"), "{h}");
        assert!(h.contains("---"), "{h}");
    }

    #[test]
    fn column_mismatch_ends_table() {
        let mut ht = HeaderTracker::new();
        ht.update("| A | B |\n| --- | --- |\n");
        ht.update("| 1 | 2 | 3 |\n");
        assert!(ht.header_ended_this_unit || ht.get_headers().is_empty());
    }
}
