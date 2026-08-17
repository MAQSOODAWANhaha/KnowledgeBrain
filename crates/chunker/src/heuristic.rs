//! Tier 2: form-feed / chapter / numbered / all-caps / footer boundaries (brain `heuristic_splitter.go`).

use crate::patterns::{
    ALL_CAPS_HEADING, EXCESSIVE_BLANKS, NUMBERED_SECTION, PAGE_FOOTER, PRIO_ALL_CAPS_HEADING,
    PRIO_BLANK_BLOCK, PRIO_CHAPTER_MARKER, PRIO_FORM_FEED, PRIO_NUMBERED_HEAD, PRIO_PAGE_FOOTER,
    PRIO_VISUAL_SEP, VISUAL_SEPARATOR, chapter_patterns_for_langs,
};
use crate::splitter::{Span, protected_spans, protected_spans_rune, split_text};
use crate::{SplitterConfig, TextChunk};

#[derive(Debug, Clone, Copy)]
pub struct Boundary {
    pub rune_start: usize,
    pub priority: i32,
}

pub fn split_by_heuristics(
    text: &str,
    cfg: &SplitterConfig,
    _profile: Option<&crate::profiler::DocProfile>,
) -> Vec<TextChunk> {
    if text.is_empty() {
        return Vec::new();
    }
    let runes: Vec<char> = text.chars().collect();
    let total = runes.len();
    if total <= cfg.chunk_size {
        return split_text(text, cfg.chunk_size, cfg.chunk_overlap, &cfg.seps());
    }

    let mut bounds = find_heuristic_boundaries(text, &cfg.languages);
    let prot = protected_spans_rune(text, &protected_spans(text));
    if !prot.is_empty() {
        bounds = drop_bounds_inside_spans(&bounds, &prot);
    }
    if bounds.is_empty() {
        return split_text(text, cfg.chunk_size, cfg.chunk_overlap, &cfg.seps());
    }
    bounds.push(Boundary {
        rune_start: total,
        priority: 0,
    });
    if bounds[0].rune_start != 0 {
        bounds.insert(
            0,
            Boundary {
                rune_start: 0,
                priority: 0,
            },
        );
    }

    let mut out = Vec::new();
    let mut seq = 0usize;
    let mut chunk_start = bounds[0].rune_start;
    let mut cur_end = chunk_start;
    let min_chunk_size = (cfg.chunk_size / 4).max(50);

    for b in bounds.iter().skip(1) {
        let next_end = b.rune_start;
        let block_len = next_end.saturating_sub(cur_end);
        if block_len > cfg.chunk_size {
            if cur_end > chunk_start {
                append_chunk(&mut out, &runes, chunk_start, cur_end, &mut seq);
            }
            append_oversize(&mut out, &runes, cur_end, next_end, cfg, &mut seq);
            cur_end = next_end;
            chunk_start = next_end;
            continue;
        }
        let accumulated = next_end.saturating_sub(chunk_start);
        if accumulated > cfg.chunk_size && cur_end.saturating_sub(chunk_start) >= min_chunk_size {
            append_chunk(&mut out, &runes, chunk_start, cur_end, &mut seq);
            chunk_start = apply_overlap_aligned(&runes, cur_end, cfg.chunk_overlap, &bounds);
        }
        cur_end = next_end;
    }
    if cur_end > chunk_start {
        append_chunk(&mut out, &runes, chunk_start, cur_end, &mut seq);
    }
    out
}

pub fn find_heuristic_boundaries(text: &str, langs: &[String]) -> Vec<Boundary> {
    let mut bounds = Vec::new();
    for (idx, r) in text.chars().enumerate() {
        if r == '\u{000C}' {
            bounds.push(Boundary {
                rune_start: idx,
                priority: PRIO_FORM_FEED,
            });
        }
    }

    let chapter_pats = chapter_patterns_for_langs(langs);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut pos = 0usize;
    let mut in_fence = false;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence {
            let rune_start = pos;
            let mut added = false;
            for pat in &chapter_pats {
                if pat.is_match(line) {
                    bounds.push(Boundary {
                        rune_start,
                        priority: PRIO_CHAPTER_MARKER,
                    });
                    added = true;
                    break;
                }
            }
            if !added && NUMBERED_SECTION.is_match(line) {
                bounds.push(Boundary {
                    rune_start,
                    priority: PRIO_NUMBERED_HEAD,
                });
                added = true;
            }
            if !added && ALL_CAPS_HEADING.is_match(line) {
                bounds.push(Boundary {
                    rune_start,
                    priority: PRIO_ALL_CAPS_HEADING,
                });
                added = true;
            }
            if !added && VISUAL_SEPARATOR.is_match(line) {
                bounds.push(Boundary {
                    rune_start,
                    priority: PRIO_VISUAL_SEP,
                });
                added = true;
            }
            if !added && PAGE_FOOTER.is_match(line) {
                bounds.push(Boundary {
                    rune_start,
                    priority: PRIO_PAGE_FOOTER,
                });
            }
        }
        pos += line.chars().count();
        if i + 1 < lines.len() {
            pos += 1;
        }
    }

    for m in EXCESSIVE_BLANKS.find_iter(text) {
        let rune_start = text[..m.end()].chars().count();
        bounds.push(Boundary {
            rune_start,
            priority: PRIO_BLANK_BLOCK,
        });
    }

    if bounds.is_empty() {
        return Vec::new();
    }
    bounds.sort_by(|a, b| {
        a.rune_start
            .cmp(&b.rune_start)
            .then_with(|| b.priority.cmp(&a.priority))
    });
    let mut deduped = Vec::new();
    let mut prev = usize::MAX;
    for b in bounds {
        if b.rune_start != prev {
            prev = b.rune_start;
            deduped.push(b);
        }
    }
    deduped
}

pub fn drop_bounds_inside_spans(bounds: &[Boundary], spans: &[Span]) -> Vec<Boundary> {
    if spans.is_empty() {
        return bounds.to_vec();
    }
    let mut out = Vec::new();
    'bound: for b in bounds {
        for s in spans {
            if s.start >= b.rune_start {
                break;
            }
            if b.rune_start < s.end {
                continue 'bound;
            }
        }
        out.push(*b);
    }
    out
}

fn append_chunk(
    out: &mut Vec<TextChunk>,
    runes: &[char],
    start: usize,
    end: usize,
    seq: &mut usize,
) {
    if end <= start {
        return;
    }
    let raw: String = runes[start..end].iter().collect();
    if raw.trim().is_empty() {
        return;
    }
    out.push(TextChunk {
        content: raw,
        context_header: String::new(),
        seq: *seq,
        start,
        end,
    });
    *seq += 1;
}

fn append_oversize(
    out: &mut Vec<TextChunk>,
    runes: &[char],
    start: usize,
    end: usize,
    cfg: &SplitterConfig,
    seq: &mut usize,
) {
    if end <= start {
        return;
    }
    let sub: String = runes[start..end].iter().collect();
    for s in split_text(&sub, cfg.chunk_size, cfg.chunk_overlap, &cfg.seps()) {
        out.push(TextChunk {
            content: s.content,
            context_header: String::new(),
            seq: *seq,
            start: start + s.start,
            end: start + s.end,
        });
        *seq += 1;
    }
}

fn apply_overlap_aligned(
    runes: &[char],
    cur_end: usize,
    overlap: usize,
    bounds: &[Boundary],
) -> usize {
    if overlap == 0 {
        return cur_end;
    }
    let target = cur_end.saturating_sub(overlap);
    let window_start = cur_end.saturating_sub(2 * overlap);
    let mut best = None;
    for b in bounds {
        if b.rune_start >= window_start && b.rune_start < cur_end {
            best = Some(best.map_or(b.rune_start, |cur: usize| cur.max(b.rune_start)));
        }
    }
    if let Some(b) = best {
        return b;
    }
    let mut i = target;
    while i > window_start && i < runes.len() {
        if runes[i] == '\n' {
            return i + 1;
        }
        i -= 1;
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::LANG_CHINESE;

    fn cfg(size: usize, overlap: usize) -> SplitterConfig {
        SplitterConfig {
            chunk_size: size,
            chunk_overlap: overlap,
            separators: vec![". ".into()],
            ..SplitterConfig::default()
        }
    }

    #[test]
    fn form_feed_boundary() {
        let doc = format!(
            "{}{}{}",
            "page one body text. ".repeat(30),
            '\u{000C}',
            "page two body. ".repeat(30)
        );
        let chunks = split_by_heuristics(&doc, &cfg(400, 20), None);
        assert!(chunks.len() >= 2, "got {}", chunks.len());
    }

    #[test]
    fn numbered_sections() {
        let body = "body sentence. ".repeat(8);
        let doc = format!("1. Introduction\n{body}\n\n2. Methods\n{body}\n\n3. Results\n{body}");
        let chunks = split_by_heuristics(&doc, &cfg(200, 20), None);
        assert!(chunks.len() >= 2, "got {}", chunks.len());
    }

    #[test]
    fn german_chapters() {
        let body = "Beispieltext. ".repeat(10);
        let doc = format!("Kapitel 1: Einführung\n{body}\n\nKapitel 2: Hauptteil\n{body}");
        let chunks = split_by_heuristics(&doc, &cfg(200, 20), None);
        assert!(chunks.len() >= 2, "got {}", chunks.len());
    }

    #[test]
    fn chinese_chapters() {
        let body = "内容内容内容。".repeat(60);
        let doc = format!("第一章 引言\n{body}\n\n第二章 方法\n{body}");
        let mut c = cfg(200, 20);
        c.separators = vec!["。".into()];
        c.languages = vec![LANG_CHINESE.into()];
        let chunks = split_by_heuristics(&doc, &c, None);
        assert!(chunks.len() >= 2, "got {}", chunks.len());
    }

    #[test]
    fn unstructured_short_is_one_chunk() {
        let doc = "plain prose without structure. ".repeat(5);
        let mut c = cfg(1000, 20);
        c.separators = vec!["\n\n".into(), "\n".into(), "。".into()];
        let chunks = split_by_heuristics(&doc, &c, None);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn oversize_block_recurses() {
        let huge = "This is a long sentence. ".repeat(200);
        let doc = format!("1. Intro\n{huge}");
        let chunks = split_by_heuristics(&doc, &cfg(500, 50), None);
        assert!(chunks.len() >= 5, "got {}", chunks.len());
        for c in &chunks {
            assert!(c.content.chars().count() <= 2 * 500);
        }
    }

    #[test]
    fn bounds_are_ordered() {
        let doc =
            "Kapitel 1: A\nbody\n\n---\n\n2. Section B\nbody\n\nPage 3 of 10\n\n第三章 C\nbody";
        let bounds = find_heuristic_boundaries(doc, &[]);
        assert!(bounds.len() >= 2);
        for w in bounds.windows(2) {
            assert!(w[1].rune_start >= w[0].rune_start);
        }
    }

    #[test]
    fn empty_text() {
        assert!(split_by_heuristics("", &SplitterConfig::default(), None).is_empty());
    }

    #[test]
    fn overlap_actually_overlaps() {
        let mut sb = String::new();
        for i in 1..=12 {
            sb.push_str("\n\n");
            sb.push(char::from(b'0' + (i % 10) as u8));
            sb.push_str(". ");
            sb.push_str(&"alpha beta gamma. ".repeat(4));
        }
        let chunks = split_by_heuristics(&sb, &cfg(200, 80), None);
        assert!(chunks.len() >= 2, "got {}", chunks.len());
        let mut saw = false;
        for w in chunks.windows(2) {
            let prev = w[0].content.trim();
            let cur = w[1].content.trim();
            let max = prev.len().min(cur.len());
            let mut match_n = 0;
            for n in 1..=max {
                if cur.starts_with(&prev[prev.len() - n..]) {
                    match_n = n;
                }
            }
            if match_n >= 20 {
                saw = true;
                break;
            }
        }
        assert!(
            saw,
            "no overlapping pair; sizes {:?}",
            chunks
                .iter()
                .map(|c| c.content.chars().count())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn drops_bounds_inside_protected() {
        let body = "filler. ".repeat(30);
        let doc = format!("{body}\n\n$$\nx = 1\n1. equation step one\ny = 2\n$$\n\n{body}");
        let bounds = find_heuristic_boundaries(&doc, &[]);
        let prot = protected_spans_rune(&doc, &protected_spans(&doc));
        assert!(!prot.is_empty());
        let filtered = drop_bounds_inside_spans(&bounds, &prot);
        for b in &filtered {
            for s in &prot {
                assert!(
                    !(b.rune_start > s.start && b.rune_start < s.end),
                    "bound {} inside [{}, {})",
                    b.rune_start,
                    s.start,
                    s.end
                );
            }
        }
        assert!(
            filtered.len() < bounds.len(),
            "before={} after={}",
            bounds.len(),
            filtered.len()
        );
    }
}
