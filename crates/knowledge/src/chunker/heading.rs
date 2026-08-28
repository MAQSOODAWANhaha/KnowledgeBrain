//! Tier 1: ATX heading sections; breadcrumb in ContextHeader (brain `heading_splitter.go`).

use crate::chunker::heading_hierarchy::HeadingHierarchy;
use crate::chunker::patterns::MARKDOWN_HEADING;
use crate::chunker::profiler::{DocProfile, profile_document};
use crate::chunker::splitter::split_text;
use crate::chunker::{SplitterConfig, TextChunk};

struct HeadingBoundary {
    rune_start: usize,
    line: String,
}

struct SectionBreadcrumb {
    rune_start: usize,
    breadcrumb: String,
}

pub fn split_by_headings(
    text: &str,
    cfg: &SplitterConfig,
    profile: Option<&DocProfile>,
) -> Vec<TextChunk> {
    if text.is_empty() {
        return Vec::new();
    }
    let owned;
    let profile = match profile {
        Some(p) => p,
        None => {
            owned = profile_document(text);
            &owned
        }
    };
    let primary_level = profile.dominant_heading_level();
    if primary_level == 0 {
        return split_text(text, cfg.chunk_size, cfg.chunk_overlap, &cfg.seps());
    }
    let bounds = find_heading_boundaries(text, primary_level);
    if bounds.len() <= 1 {
        return split_text(text, cfg.chunk_size, cfg.chunk_overlap, &cfg.seps());
    }

    let runes: Vec<char> = text.chars().collect();
    let mut hierarchy = HeadingHierarchy::new();
    let mut out = Vec::new();
    let mut seq = 0usize;

    for (i, b) in bounds.iter().enumerate() {
        let end_rune = bounds
            .get(i + 1)
            .map(|n| n.rune_start)
            .unwrap_or(runes.len());
        if !b.line.is_empty() {
            hierarchy.observe(&b.line);
        }
        let breadcrumb = hierarchy.breadcrumb_with_hashes();
        let section_start = hierarchy.clone();
        observe_sub_headings(
            &runes[b.rune_start..end_rune],
            primary_level,
            &mut hierarchy,
        );

        if b.rune_start >= end_rune {
            continue;
        }
        let section_runes = &runes[b.rune_start..end_rune];
        let section_content: String = section_runes.iter().collect();
        let sec_len = section_runes.len();
        let bc_len = breadcrumb.chars().count();
        if bc_len + 2 + sec_len <= cfg.chunk_size {
            out.push(TextChunk {
                content: section_content,
                context_header: breadcrumb,
                seq,
                start: b.rune_start,
                end: end_rune,
            });
            seq += 1;
            continue;
        }

        let sub_breadcrumbs = section_breadcrumbs(section_runes, primary_level, section_start);
        let sub_chunks = split_text(
            &section_content,
            cfg.chunk_size,
            cfg.chunk_overlap,
            &cfg.seps(),
        );
        for sub in sub_chunks {
            out.push(TextChunk {
                content: sub.content,
                context_header: breadcrumb_at_offset(&sub_breadcrumbs, sub.start, &breadcrumb),
                seq,
                start: b.rune_start + sub.start,
                end: b.rune_start + sub.end,
            });
            seq += 1;
        }
    }
    coalesce_tiny_chunks(out, cfg.chunk_size)
}

fn find_heading_boundaries(text: &str, primary_level: usize) -> Vec<HeadingBoundary> {
    let mut bounds = vec![HeadingBoundary {
        rune_start: 0,
        line: String::new(),
    }];
    if text.is_empty() {
        return bounds;
    }
    let mut pos = 0usize;
    let mut in_fence = false;
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            pos += line.chars().count();
            if i + 1 < lines.len() {
                pos += 1;
            }
            continue;
        }
        if !in_fence && let Some(caps) = MARKDOWN_HEADING.captures(line) {
            let level = caps.get(1).map(|m| m.as_str().len()).unwrap_or(0);
            if (1..=primary_level).contains(&level) && pos > 0 {
                bounds.push(HeadingBoundary {
                    rune_start: pos,
                    line: (*line).to_string(),
                });
            }
            if (1..=primary_level).contains(&level) && pos == 0 {
                bounds[0].line = (*line).to_string();
            }
        }
        pos += line.chars().count();
        if i + 1 < lines.len() {
            pos += 1;
        }
    }
    bounds
}

fn observe_sub_headings(runes: &[char], primary_level: usize, h: &mut HeadingHierarchy) {
    if runes.is_empty() {
        return;
    }
    let text: String = runes.iter().collect();
    let mut in_fence = false;
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let Some(caps) = MARKDOWN_HEADING.captures(line) else {
            continue;
        };
        let level = caps.get(1).map(|m| m.as_str().len()).unwrap_or(0);
        if level > primary_level {
            h.observe(line);
        }
    }
}

fn section_breadcrumbs(
    section_runes: &[char],
    primary_level: usize,
    mut h: HeadingHierarchy,
) -> Vec<SectionBreadcrumb> {
    let mut result = vec![SectionBreadcrumb {
        rune_start: 0,
        breadcrumb: h.breadcrumb_with_hashes(),
    }];
    let mut pos = 0usize;
    let mut in_fence = false;
    let text: String = section_runes.iter().collect();
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            pos += line.chars().count();
            if i + 1 < lines.len() {
                pos += 1;
            }
            continue;
        }
        if !in_fence && let Some(caps) = MARKDOWN_HEADING.captures(line) {
            let level = caps.get(1).map(|m| m.as_str().len()).unwrap_or(0);
            if level > primary_level {
                h.observe(line);
                result.push(SectionBreadcrumb {
                    rune_start: pos,
                    breadcrumb: h.breadcrumb_with_hashes(),
                });
            }
        }
        pos += line.chars().count();
        if i + 1 < lines.len() {
            pos += 1;
        }
    }
    result
}

fn breadcrumb_at_offset(bcs: &[SectionBreadcrumb], offset: usize, fallback: &str) -> String {
    let mut bc = fallback.to_string();
    for e in bcs {
        if e.rune_start > offset {
            break;
        }
        bc = e.breadcrumb.clone();
    }
    bc
}

fn coalesce_tiny_chunks(input: Vec<TextChunk>, chunk_size: usize) -> Vec<TextChunk> {
    if input.len() <= 1 || chunk_size == 0 {
        return input;
    }
    let target = (chunk_size / 2).max(200);
    let mut out = Vec::new();
    let mut cur = input[0].clone();
    let mut cur_len = cur.content.chars().count();
    for next in input.into_iter().skip(1) {
        let next_len = next.content.chars().count();
        let shared = common_heading_prefix(&cur.context_header, &next.context_header);
        if !shared.is_empty()
            && cur.end == next.start
            && cur_len < target
            && cur_len + next_len <= chunk_size
        {
            cur.content.push_str(&next.content);
            cur.context_header = shared;
            cur.end = next.end;
            cur_len += next_len;
            continue;
        }
        out.push(cur);
        cur = next;
        cur_len = next_len;
    }
    out.push(cur);
    for (i, c) in out.iter_mut().enumerate() {
        c.seq = i;
    }
    out
}

pub fn common_heading_prefix(a: &str, b: &str) -> String {
    if a == b {
        return a.to_string();
    }
    let la: Vec<&str> = a.split('\n').collect();
    let lb: Vec<&str> = b.split('\n').collect();
    let n = la.len().min(lb.len());
    let mut common = 0usize;
    for i in 0..n {
        if la[i] != lb[i] {
            break;
        }
        common = i + 1;
    }
    if common == 0 {
        String::new()
    } else {
        la[..common].join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(size: usize, overlap: usize) -> SplitterConfig {
        SplitterConfig {
            chunk_size: size,
            chunk_overlap: overlap,
            separators: vec!["\n\n".into(), "\n".into(), "。".into()],
            ..SplitterConfig::default()
        }
    }

    #[test]
    fn basic_sections_put_heading_in_content_and_breadcrumb() {
        let body = "Lorem ipsum dolor sit amet consectetur adipiscing elit. ".repeat(4);
        let doc = format!(
            "# Top\n{body}\n\n## Section A\n{body}\n\n## Section B\n{body}\n\n## Section C\n{body}"
        );
        let chunks = split_by_headings(&doc, &cfg(300, 0), None);
        assert!(chunks.len() >= 3, "got {}", chunks.len());
        for c in &chunks {
            assert!(c.context_header.contains("# Top"), "{}", c.context_header);
            assert!(c.embedding_content().contains("# Top"));
        }
        assert!(
            chunks
                .iter()
                .any(|c| c.content.contains("## Section B") && c.content.contains("Lorem ipsum"))
        );
    }

    #[test]
    fn unstructured_falls_through_to_legacy() {
        let doc = "Just a plain paragraph without any headings at all in this text.";
        let chunks = split_by_headings(doc, &cfg(200, 0), None);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn breadcrumb_reflects_latest_path() {
        let body = "Lorem ipsum dolor sit amet consectetur adipiscing elit. ".repeat(4);
        let doc = format!("# Chapter 1\n{body}\n\n## Section A\n{body}\n\n## Section B\n{body}");
        let chunks = split_by_headings(&doc, &cfg(300, 0), None);
        let b = chunks
            .iter()
            .find(|c| c.content.contains("## Section B"))
            .expect("B");
        assert!(b.context_header.contains("## Section B"));
        assert!(!b.context_header.contains("## Section A"));
        assert!(b.context_header.contains("# Chapter 1"));
    }

    #[test]
    fn ignores_headings_inside_fence() {
        let doc = "# Real\n\n```\n# Fake heading inside code\n```\n\nbody";
        let chunks = split_by_headings(doc, &cfg(500, 0), None);
        assert!(
            chunks
                .iter()
                .any(|c| c.context_header.contains("# Real") || c.content.contains("# Real"))
        );
        for c in &chunks {
            assert!(!c.context_header.contains("# Fake"));
        }
    }

    #[test]
    fn position_invariant() {
        let doc = "# Top\nintro paragraph here.\n\n## Section A\ncontent of A here, several sentences.\n\n## Section B\ncontent of B here.\n\n## Section C\ncontent of C here.";
        let chunks = split_by_headings(doc, &cfg(200, 20), None);
        let runes: Vec<char> = doc.chars().collect();
        for c in &chunks {
            assert_eq!(c.end - c.start, c.content.chars().count(), "{}", c.content);
            let sliced: String = runes[c.start..c.end].iter().collect();
            assert_eq!(sliced, c.content);
        }
    }

    #[test]
    fn coalesces_tiny_adjacent_sections() {
        let doc = "# Install Log\n\n## Docker镜像\n使用 daocloud 部署 v0.3.1。\n\n## 前端老版本\n浏览器缓存了旧前端资源。\n\n## 登录报错\nERROR: column missing.\n\n## 解析失败\nembedding 表缺列。";
        let chunks = split_by_headings(doc, &cfg(500, 0), None);
        assert!(!chunks.is_empty());
        assert!(chunks.len() < 5, "got {}", chunks.len());
        for c in &chunks {
            assert!(c.context_header.contains("# Install Log"));
        }
        for h in [
            "## Docker镜像",
            "## 前端老版本",
            "## 登录报错",
            "## 解析失败",
        ] {
            assert!(chunks.iter().any(|c| c.content.contains(h)), "missing {h}");
        }
    }

    #[test]
    fn does_not_coalesce_distinct_top_level() {
        let doc = "# Intro\nshort intro.\n\n# Usage\nshort usage.\n\n# FAQ\nshort faq.";
        let chunks = split_by_headings(doc, &cfg(500, 0), None);
        assert_eq!(chunks.len(), 3);
        for (i, h) in ["# Intro", "# Usage", "# FAQ"].iter().enumerate() {
            assert!(chunks[i].content.contains(h), "{}", chunks[i].content);
        }
    }

    #[test]
    fn no_breadcrumb_duplication_in_content() {
        let doc = "# Chapter 1\nintro.\n\n## Section A\nbody A.\n\n## Section B\nbody B.";
        let chunks = split_by_headings(doc, &cfg(500, 0), None);
        for c in &chunks {
            assert!(c.content.matches("## Section A").count() <= 1);
            assert!(c.content.matches("## Section B").count() <= 1);
        }
    }

    #[test]
    fn deep_subheading_survives_large_section() {
        let filler = "clause body sentence that pads the section out. ".repeat(20);
        let doc = format!(
            "# Standard XYZ\n## Preface\n{filler}\n\n## 5 Classification\n{filler}\n\n### 5.9 Grade Nine\n{filler}\n\n#### 5.9.2 Clause Series\n{filler}\n\nthe item users search for is MARKER_ITEM_23 graded here.\n\n## Appendix A\n{filler}\n\n## Appendix B\n{filler}\n\n## Appendix C\n{filler}"
        );
        let mut c = cfg(300, 0);
        c.separators = vec![". ".into()];
        let chunks = split_by_headings(&doc, &c, None);
        let marker = chunks
            .iter()
            .find(|c| c.content.contains("MARKER_ITEM_23"))
            .expect("marker");
        assert!(
            marker.context_header.contains("#### 5.9.2 Clause Series"),
            "{}",
            marker.context_header
        );
        assert!(marker.context_header.contains("## 5 Classification"));
    }

    #[test]
    fn common_prefix() {
        assert_eq!(common_heading_prefix("# Top\n## A", "# Top\n## B"), "# Top");
        assert_eq!(common_heading_prefix("# X", "# Y"), "");
        assert_eq!(
            common_heading_prefix("# Top\n## A\n### x", "# Top\n## A\n### y"),
            "# Top\n## A"
        );
    }
}
