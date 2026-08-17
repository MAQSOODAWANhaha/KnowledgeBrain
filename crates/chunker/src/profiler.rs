//! Document structure profile + strategy selection (brain `profiler.go`).

use crate::patterns::{
    ALL_CAPS_HEADING, CHINESE_CHAPTER, ENGLISH_CHAPTER, GERMAN_CHAPTER, MARKDOWN_HEADING,
    NUMBERED_SECTION, PAGE_FOOTER, VISUAL_SEPARATOR,
};
use crate::tokens::{
    LANG_CHINESE, LANG_ENGLISH, LANG_GERMAN, LANG_MIXED, byte_prefix, detect_language,
};

#[derive(Debug, Clone, Default)]
pub struct DocProfile {
    pub total_chars: usize,
    pub total_lines: usize,
    pub avg_line_len: f64,
    pub std_line_len: f64,
    pub md_heading_counts: [usize; 7],
    pub md_heading_total: usize,
    pub numbered_section_count: usize,
    pub all_caps_short_line_count: usize,
    pub blank_paragraph_breaks: usize,
    pub form_feed_count: usize,
    pub visual_sep_count: usize,
    pub german_chapter_count: usize,
    pub english_chapter_count: usize,
    pub chinese_chapter_count: usize,
    pub repeated_footer_count: usize,
    pub has_tables: bool,
    pub has_code: bool,
    pub code_ratio: f64,
    pub detected_langs: Vec<String>,
}

impl DocProfile {
    pub fn heading_density(&self) -> f64 {
        if self.total_lines == 0 {
            0.0
        } else {
            self.md_heading_total as f64 / self.total_lines as f64
        }
    }

    /// Lowest level with ≥3 hits, else deepest level present. 0 if none.
    pub fn dominant_heading_level(&self) -> usize {
        if self.md_heading_total == 0 {
            return 0;
        }
        for level in 1..=6 {
            if self.md_heading_counts[level] >= 3 {
                return level;
            }
        }
        for level in (1..=6).rev() {
            if self.md_heading_counts[level] > 0 {
                return level;
            }
        }
        0
    }

    pub fn heuristic_marker_total(&self) -> usize {
        self.numbered_section_count
            + self.german_chapter_count
            + self.english_chapter_count
            + self.chinese_chapter_count
            + self.all_caps_short_line_count
            + self.visual_sep_count
            + self.form_feed_count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyTier {
    Heading,
    Heuristic,
    Legacy,
}

impl StrategyTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Heading => "heading",
            Self::Heuristic => "heuristic",
            Self::Legacy => "legacy",
        }
    }
}

pub fn profile_document(text: &str) -> DocProfile {
    let mut p = DocProfile::default();
    if text.is_empty() {
        return p;
    }
    p.total_chars = text.chars().count();
    p.form_feed_count = text.matches('\u{000C}').count();

    let lines: Vec<&str> = text.split('\n').collect();
    p.total_lines = lines.len();

    let mut lengths: Vec<f64> = Vec::new();
    let mut in_fence = false;
    let mut code_chars = 0usize;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            p.has_code = true;
            continue;
        }
        if in_fence {
            code_chars += line.chars().count();
            continue;
        }
        lengths.push(line.chars().count() as f64);
        if match_heading(line, &mut p.md_heading_counts) {
            p.md_heading_total += 1;
            continue;
        }
        if NUMBERED_SECTION.is_match(line) {
            p.numbered_section_count += 1;
        }
        if GERMAN_CHAPTER.is_match(line) {
            p.german_chapter_count += 1;
        }
        if ENGLISH_CHAPTER.is_match(line) {
            p.english_chapter_count += 1;
        }
        if CHINESE_CHAPTER.is_match(line) {
            p.chinese_chapter_count += 1;
        }
        if ALL_CAPS_HEADING.is_match(line) {
            p.all_caps_short_line_count += 1;
        }
        if VISUAL_SEPARATOR.is_match(line) {
            p.visual_sep_count += 1;
        }
        if PAGE_FOOTER.is_match(line) {
            p.repeated_footer_count += 1;
        }
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            p.has_tables = true;
        }
    }

    if !lengths.is_empty() {
        let sum: f64 = lengths.iter().sum();
        p.avg_line_len = sum / lengths.len() as f64;
        let var: f64 = lengths
            .iter()
            .map(|l| {
                let d = l - p.avg_line_len;
                d * d
            })
            .sum::<f64>()
            / lengths.len() as f64;
        p.std_line_len = var.sqrt();
    }
    if p.total_chars > 0 {
        p.code_ratio = code_chars as f64 / p.total_chars as f64;
    }
    p.blank_paragraph_breaks = text.matches("\n\n\n").count();

    let sample = byte_prefix(text, 4096);
    let lang = detect_language(sample);
    if lang == LANG_MIXED {
        p.detected_langs = vec![LANG_ENGLISH.into(), LANG_GERMAN.into(), LANG_CHINESE.into()];
    } else {
        p.detected_langs = vec![lang.into()];
    }
    p
}

fn match_heading(line: &str, counts: &mut [usize; 7]) -> bool {
    let Some(caps) = MARKDOWN_HEADING.captures(line) else {
        return false;
    };
    let level = caps.get(1).map(|m| m.as_str().len()).unwrap_or(0);
    if !(1..=6).contains(&level) {
        return false;
    }
    counts[level] += 1;
    true
}

/// heading if ≥3 ATX headings, density > 0.005, and a dominant level;
/// heuristic if ≥5 markers / form-feed / any chapter marker; legacy always last.
pub fn select_strategy(p: &DocProfile) -> Vec<StrategyTier> {
    let mut chain = Vec::new();
    if p.md_heading_total >= 3 && p.heading_density() > 0.005 && p.dominant_heading_level() > 0 {
        chain.push(StrategyTier::Heading);
    }
    if p.heuristic_marker_total() >= 5
        || p.form_feed_count > 0
        || p.german_chapter_count + p.english_chapter_count + p.chinese_chapter_count > 0
    {
        chain.push(StrategyTier::Heuristic);
    }
    chain.push(StrategyTier::Legacy);
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_profile() {
        let p = profile_document("");
        assert_eq!(p.total_chars, 0);
        assert_eq!(select_strategy(&p), vec![StrategyTier::Legacy]);
    }

    #[test]
    fn heading_doc_selects_heading_tier() {
        let doc = "# A\nbody\n## B\nbody\n## C\nbody\n## D\nbody\n";
        let p = profile_document(doc);
        assert!(p.md_heading_total >= 3);
        let chain = select_strategy(&p);
        assert_eq!(chain[0], StrategyTier::Heading);
        assert_eq!(*chain.last().unwrap(), StrategyTier::Legacy);
    }

    #[test]
    fn heuristic_doc_selects_heuristic_tier() {
        let doc = "Kapitel 1: A\nbody\n\nKapitel 2: B\nbody\n";
        let p = profile_document(doc);
        assert!(p.german_chapter_count > 0);
        let chain = select_strategy(&p);
        assert_eq!(chain[0], StrategyTier::Heuristic);
    }

    #[test]
    fn plain_doc_is_legacy_only() {
        let p = profile_document("just a paragraph of prose without markers.");
        assert_eq!(select_strategy(&p), vec![StrategyTier::Legacy]);
    }

    #[test]
    fn dominant_level_prefers_backbone() {
        let doc = "# T\n## A\n## B\n## C\n### x\n";
        let p = profile_document(doc);
        assert_eq!(p.dominant_heading_level(), 2);
    }
}
