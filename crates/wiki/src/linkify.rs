//! Brain `wiki_linkify.go`: wrap the first safe mention of each page.

#[derive(Clone, Debug)]
pub struct LinkRef {
    pub slug: String,
    pub match_text: String,
}

#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}

pub fn linkify_content(content: &str, refs: &[LinkRef], self_slug: &str) -> (String, bool) {
    if content.is_empty() || refs.is_empty() {
        return (content.to_string(), false);
    }
    let mut sorted: Vec<&LinkRef> = refs
        .iter()
        .filter(|r| !r.slug.is_empty() && !r.match_text.is_empty() && r.slug != self_slug)
        .collect();
    sorted.sort_by(|a, b| {
        b.match_text
            .chars()
            .count()
            .cmp(&a.match_text.chars().count())
    });
    let mut out = content.to_string();
    let mut forbidden = compute_forbidden_spans(&out);
    let mut used: Vec<String> = existing_slugs(&out);
    let mut changed = false;
    for r in sorted {
        if used.iter().any(|s| s == &r.slug) {
            continue;
        }
        let Some(pos) = find_first_safe_match(&out, &r.match_text, &forbidden) else {
            continue;
        };
        let replacement = format!("[[{}|{}]]", r.slug, r.match_text);
        let end = pos + r.match_text.len();
        out.replace_range(pos..end, &replacement);
        let delta = replacement.len() as isize - r.match_text.len() as isize;
        forbidden = shift_spans_after(forbidden, pos, delta);
        forbidden.push(Span {
            start: pos,
            end: pos + replacement.len(),
        });
        forbidden.sort_by_key(|s| s.start);
        used.push(r.slug.clone());
        changed = true;
    }
    (out, changed)
}

fn next_char(s: &str, i: usize) -> usize {
    s.get(i..)
        .and_then(|rest| rest.chars().next())
        .map(|c| i + c.len_utf8())
        .unwrap_or(s.len())
}

fn existing_slugs(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if !content.is_char_boundary(i) {
            i = next_char(content, i);
            continue;
        }
        if bytes[i] == b'['
            && bytes[i + 1] == b'['
            && content.is_char_boundary(i + 2)
            && let Some(rel) = content[i + 2..].find("]]")
        {
            let inner = &content[i + 2..i + 2 + rel];
            let slug = inner.split('|').next().unwrap_or(inner).trim();
            if !slug.is_empty() {
                out.push(slug.to_string());
            }
            i += 2 + rel + 2;
            continue;
        }
        i = next_char(content, i);
    }
    out
}

fn compute_forbidden_spans(content: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !content.is_char_boundary(i) {
            i = next_char(content, i);
            continue;
        }
        if content[i..].starts_with("```") {
            let rest = &content[i + 3..];
            if let Some(rel) = rest.find("```") {
                let end = i + 3 + rel + 3;
                spans.push(Span { start: i, end });
                i = end;
                continue;
            }
            spans.push(Span {
                start: i,
                end: content.len(),
            });
            break;
        }
        if bytes[i] == b'`'
            && let Some(rel) = content[i + 1..].find('`')
        {
            let end = i + 1 + rel + 1;
            spans.push(Span { start: i, end });
            i = end;
            continue;
        }
        if content[i..].starts_with("[[")
            && let Some(rel) = content[i + 2..].find("]]")
        {
            let end = i + 2 + rel + 2;
            spans.push(Span { start: i, end });
            i = end;
            continue;
        }
        if bytes[i] == b'!'
            && content[i..].starts_with("![")
            && let Some(end) = markdown_link_end(content, i + 1)
        {
            spans.push(Span { start: i, end });
            i = end;
            continue;
        }
        if bytes[i] == b'['
            && let Some(end) = markdown_link_end(content, i)
        {
            spans.push(Span { start: i, end });
            i = end;
            continue;
        }
        i = next_char(content, i);
    }
    spans
}

fn markdown_link_end(content: &str, start: usize) -> Option<usize> {
    let rest = &content[start..];
    if !rest.starts_with('[') {
        return None;
    }
    let close = rest.find(']')?;
    let after = &rest[close + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let close_paren = after.find(')')?;
    Some(start + close + 1 + close_paren + 1)
}

fn find_first_safe_match(haystack: &str, needle: &str, forbidden: &[Span]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let needs_boundary = has_ascii_letter_edge(needle);
    let mut start = 0;
    while start + needle.len() <= haystack.len() {
        let rel = haystack[start..].find(needle)?;
        let pos = start + rel;
        let end = pos + needle.len();
        if !haystack.is_char_boundary(pos) || !haystack.is_char_boundary(end) {
            start = next_char(haystack, pos);
            continue;
        }
        if span_contains(forbidden, pos, end) {
            start = next_char(haystack, pos);
            continue;
        }
        if needs_boundary && !has_word_boundary(haystack, pos, end) {
            start = next_char(haystack, pos);
            continue;
        }
        return Some(pos);
    }
    None
}

fn has_ascii_letter_edge(s: &str) -> bool {
    let b = s.as_bytes();
    let first = *b.first().unwrap_or(&0);
    let last = *b.last().unwrap_or(&0);
    first.is_ascii_alphanumeric() || last.is_ascii_alphanumeric()
}

fn has_word_boundary(haystack: &str, start: usize, end: usize) -> bool {
    let bytes = haystack.as_bytes();
    let left_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
    let right_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
    left_ok && right_ok
}

fn span_contains(spans: &[Span], start: usize, end: usize) -> bool {
    spans.iter().any(|s| start < s.end && end > s.start)
}

fn shift_spans_after(spans: Vec<Span>, pos: usize, delta: isize) -> Vec<Span> {
    spans
        .into_iter()
        .map(|s| {
            if s.start >= pos {
                Span {
                    start: (s.start as isize + delta) as usize,
                    end: (s.end as isize + delta) as usize,
                }
            } else {
                s
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_first_safe_mention() {
        let refs = [LinkRef {
            slug: "entity/alpha".into(),
            match_text: "Alpha".into(),
        }];
        let (out, changed) =
            linkify_content("See Alpha in the lab. Alpha again.", &refs, "summary/x");
        assert!(changed);
        assert!(out.contains("[[entity/alpha|Alpha]]"));
        assert_eq!(out.matches("[[entity/alpha|Alpha]]").count(), 1);
        assert!(out.contains("Alpha again"));
    }

    #[test]
    fn skips_code_and_self() {
        let refs = [LinkRef {
            slug: "entity/alpha".into(),
            match_text: "Alpha".into(),
        }];
        let (out, changed) = linkify_content("use `Alpha` in code", &refs, "summary/x");
        assert!(!changed);
        assert_eq!(out, "use `Alpha` in code");
        let (out, changed) = linkify_content("Alpha page", &refs, "entity/alpha");
        assert!(!changed);
        assert_eq!(out, "Alpha page");
    }

    #[test]
    fn longer_name_wins() {
        let refs = [
            LinkRef {
                slug: "entity/alpha".into(),
                match_text: "Alpha".into(),
            },
            LinkRef {
                slug: "concept/alpha-switch".into(),
                match_text: "Alpha Switch".into(),
            },
        ];
        let (out, changed) = linkify_content("The Alpha Switch is ready.", &refs, "summary/x");
        assert!(changed);
        assert!(out.contains("[[concept/alpha-switch|Alpha Switch]]"));
        assert!(!out.contains("[[entity/alpha|Alpha]]"));
    }

    #[test]
    fn chinese_content_does_not_panic() {
        let refs = [LinkRef {
            slug: "entity/tian".into(),
            match_text: "天".into(),
        }];
        let (out, changed) = linkify_content("今天天气不错", &refs, "summary/x");
        assert!(changed);
        assert!(out.contains("[[entity/tian|天]]"));
        let (plain, changed) = linkify_content(
            "今天天气不错",
            &[LinkRef {
                slug: "entity/alpha".into(),
                match_text: "Alpha".into(),
            }],
            "summary/x",
        );
        assert!(!changed);
        assert_eq!(plain, "今天天气不错");
    }
}
