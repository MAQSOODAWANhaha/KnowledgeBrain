//! Legacy SplitText: protected units + recursive separators + merge (brain `splitter.go`).

use regex::Regex;
use std::sync::LazyLock;

use crate::TextChunk;
use crate::header_tracker::{HeaderTracker, header_already_present, header_column_mismatch};

pub const HARD_CAP: usize = 7500;
pub const DEFAULT_CHUNK_SIZE: usize = 512;
pub const DEFAULT_CHUNK_OVERLAP: usize = 80;

static PROTECTED_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?s)\$\$.*?\$\$").expect("latex"),
        Regex::new(r"!\[[^\]]*\]\([^)]+\)").expect("image"),
        Regex::new(r"\[[^\]]*\]\([^)]+\)").expect("link"),
        Regex::new(r"(?m)[ ]*(?:\|[^|\n]*)+\|[\r\n]+\s*(?:\|\s*:?-{3,}:?\s*)+\|[\r\n]+")
            .expect("table-header"),
        Regex::new(r"(?m)[ ]*(?:\|[^|\n]*)+\|[\r\n]+").expect("table-row"),
        Regex::new(r"(?s)```(?:\w+)?[\r\n].*?```").expect("fence"),
    ]
});

#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
struct SplitUnit {
    text: String,
    start: usize,
    end: usize,
}

pub fn rune_len(s: &str) -> usize {
    s.chars().count()
}

pub fn split_text(
    text: &str,
    chunk_size: usize,
    chunk_overlap: usize,
    separators: &[&str],
) -> Vec<TextChunk> {
    if text.is_empty() {
        return Vec::new();
    }
    let chunk_size = if chunk_size == 0 {
        DEFAULT_CHUNK_SIZE
    } else {
        chunk_size
    };
    let protected = protected_spans(text);
    let units = build_units_with_protection(text, &protected, separators, chunk_size);
    merge_units(&units, chunk_size, chunk_overlap)
}

pub fn protected_spans(text: &str) -> Vec<Span> {
    let mut all: Vec<(usize, usize)> = Vec::new();
    for pat in PROTECTED_PATTERNS.iter() {
        for m in pat.find_iter(text) {
            if m.end() > m.start() {
                all.push((m.start(), m.end()));
            }
        }
    }
    if all.is_empty() {
        return Vec::new();
    }
    all.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| (b.1 - b.0).cmp(&(a.1 - a.0))));
    let mut result = Vec::new();
    let mut last_end = 0usize;
    for (start, end) in all {
        if start >= last_end {
            result.push(Span { start, end });
            last_end = end;
        }
    }
    result
}

pub fn protected_spans_rune(text: &str, byte_spans: &[Span]) -> Vec<Span> {
    if byte_spans.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(byte_spans.len());
    let mut rune_idx = 0usize;
    let mut byte_idx = 0usize;
    for s in byte_spans {
        while byte_idx < s.start && byte_idx < text.len() {
            let ch = text[byte_idx..].chars().next().unwrap();
            byte_idx += ch.len_utf8();
            rune_idx += 1;
        }
        let start_rune = rune_idx;
        while byte_idx < s.end && byte_idx < text.len() {
            let ch = text[byte_idx..].chars().next().unwrap();
            byte_idx += ch.len_utf8();
            rune_idx += 1;
        }
        out.push(Span {
            start: start_rune,
            end: rune_idx,
        });
    }
    out
}

pub fn split_by_separators(text: &str, separators: &[&str], chunk_size: usize) -> Vec<String> {
    if text.is_empty() || separators.is_empty() {
        return vec![text.to_string()];
    }
    if chunk_size > 0 && rune_len(text) <= chunk_size {
        return vec![text.to_string()];
    }
    for (i, sep) in separators.iter().enumerate() {
        if sep.is_empty() {
            continue;
        }
        let re = Regex::new(&format!("({})", regex::escape(sep))).expect("sep");
        if re.find(text).is_none() {
            continue;
        }
        let mut pieces = Vec::new();
        let mut last = 0usize;
        for m in re.find_iter(text) {
            if m.start() > last {
                pieces.push(text[last..m.start()].to_string());
            }
            if !m.as_str().is_empty() {
                pieces.push(m.as_str().to_string());
            }
            last = m.end();
        }
        if last < text.len() {
            pieces.push(text[last..].to_string());
        }
        if pieces.len() <= 1 {
            continue;
        }
        let remaining = &separators[i + 1..];
        let mut out = Vec::new();
        for p in pieces {
            if chunk_size > 0 && rune_len(&p) > chunk_size && !remaining.is_empty() {
                out.extend(split_by_separators(&p, remaining, chunk_size));
            } else {
                out.push(p);
            }
        }
        return out;
    }
    vec![text.to_string()]
}

fn build_units_with_protection(
    text: &str,
    protected: &[Span],
    separators: &[&str],
    chunk_size: usize,
) -> Vec<SplitUnit> {
    let mut units = Vec::new();
    let mut byte_pos = 0usize;
    let mut rune_pos = 0usize;

    for p in protected {
        if p.start > byte_pos {
            let pre = &text[byte_pos..p.start];
            let parts = split_by_separators(pre, separators, chunk_size);
            let mut rune_offset = rune_pos;
            for part in parts {
                let n = rune_len(&part);
                units.push(SplitUnit {
                    text: part,
                    start: rune_offset,
                    end: rune_offset + n,
                });
                rune_offset += n;
            }
            rune_pos += rune_len(pre);
        }

        let prot_text = &text[p.start..p.end];
        let prot_rune_len = rune_len(prot_text);
        if prot_rune_len > HARD_CAP {
            units.extend(force_split_runes(prot_text, rune_pos));
        } else {
            units.push(SplitUnit {
                text: prot_text.to_string(),
                start: rune_pos,
                end: rune_pos + prot_rune_len,
            });
        }
        rune_pos += prot_rune_len;
        byte_pos = p.end;
    }

    if byte_pos < text.len() {
        let remaining = &text[byte_pos..];
        let parts = split_by_separators(remaining, separators, chunk_size);
        let mut rune_offset = rune_pos;
        for part in parts {
            let n = rune_len(&part);
            units.push(SplitUnit {
                text: part,
                start: rune_offset,
                end: rune_offset + n,
            });
            rune_offset += n;
        }
    }
    units
}

fn force_split_runes(text: &str, base: usize) -> Vec<SplitUnit> {
    let runes: Vec<char> = text.chars().collect();
    let mut units = Vec::new();
    let mut offset = 0usize;
    while offset < runes.len() {
        let mut chunk_end = (offset + HARD_CAP).min(runes.len());
        if chunk_end < runes.len() {
            let snap = chunk_end.saturating_sub(200).max(offset);
            if let Some(i) = (snap..chunk_end)
                .rev()
                .find(|&i| runes[i] == '\n' || runes[i] == ' ')
            {
                chunk_end = i + 1;
            }
        }
        let chunk_text: String = runes[offset..chunk_end].iter().collect();
        units.push(SplitUnit {
            text: chunk_text,
            start: base + offset,
            end: base + chunk_end,
        });
        offset = chunk_end;
    }
    units
}

fn merge_units(units: &[SplitUnit], chunk_size: usize, chunk_overlap: usize) -> Vec<TextChunk> {
    if units.is_empty() {
        return Vec::new();
    }
    let mut ht = HeaderTracker::new();
    let mut chunks = Vec::new();
    let mut current: Vec<SplitUnit> = Vec::new();
    let mut cur_len = 0usize;

    for u in units {
        let u_len = rune_len(&u.text);
        if u_len > HARD_CAP {
            if !current.is_empty() {
                chunks.push(build_chunk(&current, chunks.len()));
                current.clear();
                cur_len = 0;
            }
            ht.update(&u.text);
            for piece in force_split_runes(&u.text, u.start) {
                chunks.push(TextChunk {
                    content: piece.text,
                    context_header: String::new(),
                    seq: chunks.len(),
                    start: piece.start,
                    end: piece.end,
                });
            }
            continue;
        }

        ht.update(&u.text);
        if ht.header_ended_this_unit && !current.is_empty() {
            chunks.push(build_chunk(&current, chunks.len()));
            current.clear();
            cur_len = 0;
        }
        let mut headers = ht.get_headers();
        let mut headers_len = rune_len(&headers);
        if headers_len > chunk_size {
            headers.clear();
            headers_len = 0;
        }

        if cur_len + u_len + headers_len > chunk_size && !current.is_empty() {
            chunks.push(build_chunk(&current, chunks.len()));
            let (ov, ov_len) = compute_overlap(&current, chunk_overlap, chunk_size, u_len);
            current = ov;
            cur_len = ov_len;

            if !headers.is_empty() && headers_len + u_len <= chunk_size {
                while !current.is_empty() && cur_len + u_len + headers_len > chunk_size {
                    cur_len -= rune_len(&current[0].text);
                    current.remove(0);
                }
                let overlap_text = units_text(&current);
                if !header_already_present(&headers, &overlap_text, &u.text)
                    && !header_column_mismatch(&headers, &u.text)
                {
                    let start_pos = if current.is_empty() {
                        u.start
                    } else {
                        current[0].start
                    };
                    current.insert(
                        0,
                        SplitUnit {
                            text: headers.clone(),
                            start: start_pos,
                            end: start_pos,
                        },
                    );
                    cur_len += headers_len;
                }
            }
        }

        if cur_len + u_len > HARD_CAP && !current.is_empty() {
            chunks.push(build_chunk(&current, chunks.len()));
            current.clear();
            cur_len = 0;
        }

        current.push(u.clone());
        cur_len += u_len;
    }

    if !current.is_empty() {
        chunks.push(build_chunk(&current, chunks.len()));
    }
    chunks
}

fn units_text(units: &[SplitUnit]) -> String {
    units.iter().map(|u| u.text.as_str()).collect()
}

fn build_chunk(units: &[SplitUnit], seq: usize) -> TextChunk {
    TextChunk {
        content: units_text(units),
        context_header: String::new(),
        seq,
        start: units[0].start,
        end: units[units.len() - 1].end,
    }
}

fn compute_overlap(
    current: &[SplitUnit],
    chunk_overlap: usize,
    chunk_size: usize,
    next_len: usize,
) -> (Vec<SplitUnit>, usize) {
    if chunk_overlap == 0 {
        return (Vec::new(), 0);
    }
    let mut overlap_len = 0usize;
    let mut start_idx = current.len();
    for i in (0..current.len()).rev() {
        let u_len = rune_len(&current[i].text);
        if overlap_len + u_len > chunk_overlap {
            break;
        }
        if overlap_len + u_len + next_len > chunk_size {
            break;
        }
        overlap_len += u_len;
        start_idx = i;
    }
    while start_idx < current.len() {
        let u = &current[start_idx];
        let is_header_marker = u.start == u.end;
        let trimmed = u.text.trim();
        if is_header_marker || trimmed.is_empty() || is_separator_only(&u.text) {
            overlap_len = overlap_len.saturating_sub(rune_len(&u.text));
            start_idx += 1;
        } else {
            break;
        }
    }
    if start_idx >= current.len() {
        return (Vec::new(), 0);
    }
    (current[start_idx..].to_vec(), overlap_len)
}

fn is_separator_only(s: &str) -> bool {
    s.chars()
        .all(|r| r == '\n' || r == '\r' || r == ' ' || r == '\t' || r == '。')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_empty() {
        assert!(split_text("", 100, 10, &["\n\n"]).is_empty());
    }

    #[test]
    fn chinese_start_end_are_rune_offsets() {
        let text = "你好世界。这是一段中文。";
        let chunks = split_text(text, 6, 0, &["。"]);
        assert!(!chunks.is_empty());
        let runes: Vec<char> = text.chars().collect();
        for c in &chunks {
            assert_eq!(c.end - c.start, c.content.chars().count());
            let sliced: String = runes[c.start..c.end].iter().collect();
            assert_eq!(sliced, c.content);
        }
    }

    #[test]
    fn protected_code_and_latex_stay_intact() {
        let text = "前文\n\n```\ncode line\n```\n\n中间 $$E=mc^2$$ 后文";
        let chunks = split_text(text, 80, 0, &["\n\n", "\n"]);
        let joined: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(joined.contains("```\ncode line\n```"));
        assert!(joined.contains("$$E=mc^2$$"));
    }

    #[test]
    fn hard_cap_force_splits() {
        let md: String = std::iter::repeat_n('字', 8000).collect();
        let chunks = split_text(&md, 512, 80, &["\n\n", "\n", "。"]);
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert!(c.content.chars().count() <= HARD_CAP);
            assert_eq!(c.end - c.start, c.content.chars().count());
        }
    }

    #[test]
    fn recursive_separators_keep_budget() {
        let text = (0..40)
            .map(|i| format!("paragraph {i} with some words"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = split_text(&text, 80, 10, &["\n\n", "\n"]);
        assert!(chunks.len() > 1);
        for c in &chunks {
            assert!(
                c.content.chars().count() <= 80 + 20,
                "{}",
                c.content.chars().count()
            );
        }
    }

    #[test]
    fn table_header_prepended_to_later_rows() {
        let text = "前面的文字\n\n\
| 姓名 | 年龄 | 城市 |\n\
| --- | --- | --- |\n\
| 张三 | 25 | 北京 |\n\
| 李四 | 30 | 上海 |\n\
| 王五 | 28 | 广州 |\n\
| 赵六 | 35 | 深圳 |\n\
| 孙七 | 22 | 杭州 |\n\
| 周八 | 40 | 成都 |\n\
\n后面的文字";
        let table_header = "| 姓名 | 年龄 | 城市 |\n| --- | --- | --- |\n";
        let chunks = split_text(text, 60, 5, &["\n\n", "\n"]);
        assert!(chunks.len() >= 3, "got {}", chunks.len());
        let mut prepend = 0;
        for c in &chunks {
            let has_later = c.content.contains("| 李四")
                || c.content.contains("| 王五")
                || c.content.contains("| 赵六")
                || c.content.contains("| 孙七")
                || c.content.contains("| 周八");
            if has_later && !c.content.contains("| 张三") && c.content.starts_with(table_header) {
                prepend += 1;
            }
        }
        assert!(
            prepend > 0,
            "chunks: {:?}",
            chunks.iter().map(|c| &c.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_header_on_plain_prose() {
        let text = "这是一段普通的中文文本，不包含任何表格。\n\n".repeat(10);
        let chunks = split_text(&text, 30, 5, &["\n\n", "\n"]);
        let runes: Vec<char> = text.chars().collect();
        for c in &chunks {
            assert_eq!(c.end - c.start, c.content.chars().count());
            let sliced: String = runes[c.start..c.end].iter().collect();
            assert_eq!(sliced, c.content);
        }
    }
}
