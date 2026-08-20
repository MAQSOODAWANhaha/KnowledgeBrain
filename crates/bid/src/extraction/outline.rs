use std::collections::HashMap;

use super::policy::{ExtractionPolicy, family_score, fold_text, hint_family};
use super::types::{ClauseFamily, ExtractedSection, ExtractedSpan, ExtractionScope};

pub fn build_sections(
    markdown: &str,
    scope: &ExtractionScope,
    policy: &ExtractionPolicy,
) -> Vec<ExtractedSection> {
    if let ExtractionScope::Section {
        section_key,
        heading_path,
        hint_family,
        body,
    } = scope
    {
        return vec![section_from(
            section_key.clone(),
            heading_path.clone(),
            hint_family.clone(),
            body.clone(),
            policy,
        )];
    }

    let mut raw = Vec::<(String, String)>::new();
    let mut stack = Vec::<(usize, String)>::new();
    let mut current_path = String::from("正文");
    let mut body = String::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some((level, title)) = parse_heading(trimmed)
            && !numbered_requirement_line(policy, trimmed)
        {
            flush(&current_path, &mut body, &mut raw);
            stack.retain(|(old_level, _)| *old_level < level);
            stack.push((level, title));
            current_path = stack
                .iter()
                .map(|(_, part)| part.as_str())
                .collect::<Vec<_>>()
                .join(" / ");
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&current_path, &mut body, &mut raw);
    if raw.is_empty() && !markdown.trim().is_empty() {
        raw.push(("正文".into(), markdown.trim().into()));
    }

    let mut occurrences = HashMap::<String, usize>::new();
    raw.into_iter()
        .map(|(heading_path, body)| {
            let folded = fold_text(&heading_path);
            let occurrence = occurrences.entry(folded.clone()).or_default();
            *occurrence += 1;
            let digest = domain::sha256_hex(folded.as_bytes());
            let key = format!("sec-{}-{}", &digest[..16], occurrence);
            let hint = hint_family(policy, &heading_path).to_string();
            section_from(key, heading_path, hint, body, policy)
        })
        .collect()
}

fn flush(path: &str, body: &mut String, out: &mut Vec<(String, String)>) {
    let text = body.trim();
    if !text.is_empty() {
        out.push((path.to_string(), text.to_string()));
    }
    body.clear();
}

fn section_from(
    key: String,
    heading_path: String,
    hint_family: String,
    body: String,
    policy: &ExtractionPolicy,
) -> ExtractedSection {
    let blocks = split_blocks(&body, policy.limits.max_span_chars, policy);
    let spans = blocks
        .into_iter()
        .enumerate()
        .map(|(ordinal, block)| {
            let candidate_text = if block.context.is_empty() {
                block.body.clone()
            } else {
                format!("{}\n{}", block.context, block.body)
            };
            let candidate = is_candidate_span(policy, &hint_family, &heading_path, &candidate_text);
            ExtractedSpan {
                id: format!("{key}:span-{:04}", ordinal + 1),
                section_key: key.clone(),
                heading_path: heading_path.clone(),
                ordinal,
                context: block.context,
                body: block.body,
                candidate,
            }
        })
        .collect();
    ExtractedSection {
        key,
        heading_path,
        hint_family,
        body,
        spans,
        extract_status: "pending".into(),
        error_message: String::new(),
    }
}

pub(super) fn has_veto_term(policy: &ExtractionPolicy, text: &str) -> bool {
    let folded = fold_text(text);
    policy
        .must
        .veto
        .iter()
        .any(|term| folded.contains(&fold_text(term)))
}

fn is_candidate_span(policy: &ExtractionPolicy, hint: &str, _heading: &str, body: &str) -> bool {
    let body_chars = body.trim().chars().count();
    if body_chars < 6 || body_chars > policy.limits.max_span_chars {
        return false;
    }
    let technical = family_score(policy, ClauseFamily::Technical, "", body);
    let commercial = family_score(policy, ClauseFamily::Commercial, "", body);
    if hint == "skip" {
        // Only reopen a procedural heading when the body carries a real
        // family signal or an explicit rejection term.
        return technical > 0 || commercial > 0 || has_veto_term(policy, body);
    }
    if body.contains('|') && table_is_chrome(policy, body) {
        return false;
    }
    let folded = fold_text(body);
    policy
        .coverage
        .trigger_terms
        .iter()
        .any(|term| folded.contains(&fold_text(term)))
        || technical > 0
        || commercial > 0
}

pub(super) fn table_is_chrome(policy: &ExtractionPolicy, text: &str) -> bool {
    let cells: Vec<_> = text
        .split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .collect();
    !cells.is_empty()
        && cells.iter().all(|cell| {
            cell.chars().all(|c| matches!(c, '-' | ':' | '：'))
                || policy
                    .outline
                    .table_chrome_terms
                    .iter()
                    .any(|term| fold_text(cell) == fold_text(term))
        })
}

#[derive(Debug)]
struct SpanBlock {
    context: String,
    body: String,
}

fn split_blocks(body: &str, max_chars: usize, policy: &ExtractionPolicy) -> Vec<SpanBlock> {
    let lines: Vec<&str> = body.lines().collect();
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.contains('|') {
            push_limited(&mut blocks, &mut paragraph, max_chars, policy);
            let start = i;
            while i < lines.len() && (lines[i].contains('|') || lines[i].trim().is_empty()) {
                i += 1;
            }
            let table: Vec<&str> = lines[start..i]
                .iter()
                .copied()
                .filter(|line| !line.trim().is_empty())
                .collect();
            let separator = table
                .iter()
                .position(|line| line.contains("---"))
                .map(|idx| idx + 1)
                .unwrap_or(0);
            let header = table[..separator.min(table.len())].join("\n");
            let rows = &table[separator.min(table.len())..];
            for row in rows {
                let cells: Vec<_> = row
                    .split('|')
                    .map(str::trim)
                    .filter(|cell| !cell.is_empty())
                    .collect();
                let self_contained: Vec<_> = cells
                    .iter()
                    .copied()
                    .filter(|cell| table_cell_is_self_contained(policy, cell))
                    .collect();
                if self_contained.is_empty() {
                    // Key/value requirements depend on sibling cells. Preserve the
                    // exact row even above the normal span budget; splitting would
                    // make a provenance-valid quote impossible.
                    blocks.push(SpanBlock {
                        context: header.clone(),
                        body: row.trim().to_string(),
                    });
                } else {
                    for cell in self_contained {
                        split_sentence_blocks(cell, &header, max_chars, policy, &mut blocks);
                    }
                }
            }
            continue;
        }
        let trimmed = line.trim();
        let list_item = is_list_item(trimmed);
        if trimmed.is_empty() || list_item {
            push_limited(&mut blocks, &mut paragraph, max_chars, policy);
            if list_item {
                split_chars(trimmed, "", max_chars, &mut blocks);
            }
        } else {
            if !paragraph.is_empty() {
                paragraph.push('\n');
            }
            paragraph.push_str(line);
            if paragraph.chars().count() >= max_chars {
                push_limited(&mut blocks, &mut paragraph, max_chars, policy);
            }
        }
        i += 1;
    }
    push_limited(&mut blocks, &mut paragraph, max_chars, policy);
    if blocks.is_empty() && !body.trim().is_empty() && !body.contains('|') {
        split_sentence_blocks(body.trim(), "", max_chars, policy, &mut blocks);
    }
    blocks
}

fn push_limited(
    out: &mut Vec<SpanBlock>,
    buf: &mut String,
    max_chars: usize,
    policy: &ExtractionPolicy,
) {
    if !buf.trim().is_empty() {
        split_sentence_blocks(buf.trim(), "", max_chars, policy, out);
    }
    buf.clear();
}

fn table_cell_is_self_contained(policy: &ExtractionPolicy, cell: &str) -> bool {
    if cell.chars().count() < 6 {
        return false;
    }
    let has_family_subject = family_score(policy, ClauseFamily::Technical, "", cell) > 0
        || family_score(policy, ClauseFamily::Commercial, "", cell) > 0;
    if !has_family_subject {
        return false;
    }
    let folded = fold_text(cell);
    policy
        .must
        .hard
        .iter()
        .chain(policy.must.optional.iter())
        .any(|term| folded.contains(&fold_text(term)))
        || policy
            .outline
            .table_predicates
            .iter()
            .any(|term| folded.contains(&fold_text(term)))
}

fn split_sentence_blocks(
    text: &str,
    base_context: &str,
    max_chars: usize,
    policy: &ExtractionPolicy,
    out: &mut Vec<SpanBlock>,
) {
    for sentence in sentence_slices(policy, text) {
        let inherited = text
            .find(sentence)
            .filter(|offset| *offset > 0)
            .map(|offset| text[..offset].trim())
            .unwrap_or("");
        let context = match (base_context.is_empty(), inherited.is_empty()) {
            (true, true) => String::new(),
            (false, true) => base_context.to_string(),
            (true, false) => format!("preceding_requirement: {inherited}"),
            (false, false) => {
                format!("{base_context}\npreceding_requirement: {inherited}")
            }
        };
        split_chars(sentence, &context, max_chars, out);
    }
}

fn sentence_slices<'a>(policy: &ExtractionPolicy, text: &'a str) -> Vec<&'a str> {
    let mut coarse = Vec::new();
    let mut start = 0;
    for (idx, ch) in text.char_indices() {
        if matches!(ch, '。' | '；' | ';' | '\n') {
            let end = idx + ch.len_utf8();
            let sentence = text[start..end].trim();
            if !sentence.is_empty() {
                coarse.push(sentence);
            }
            start = end;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        coarse.push(tail);
    }

    let mut anchored = Vec::new();
    for sentence in coarse {
        let mut cursor = 0;
        while cursor < sentence.len() {
            let next = policy
                .outline
                .sentence_anchors
                .iter()
                .filter_map(|anchor| sentence[cursor..].find(anchor).map(|at| cursor + at))
                .filter(|at| *at > cursor)
                .min();
            if let Some(next) = next {
                let part = sentence[cursor..next].trim();
                if !part.is_empty() {
                    anchored.push(part);
                }
                cursor = next;
            } else {
                let part = sentence[cursor..].trim();
                if !part.is_empty() {
                    anchored.push(part);
                }
                break;
            }
        }
    }
    anchored
        .into_iter()
        .flat_map(|slice| enumerated_requirement_slices(policy, slice))
        .collect()
}

fn enumerated_requirement_slices<'a>(policy: &ExtractionPolicy, text: &'a str) -> Vec<&'a str> {
    let separators: Vec<(usize, usize)> = text
        .char_indices()
        .filter_map(|(offset, ch)| {
            if ch != '、' {
                return None;
            }
            let numbered_prefix = offset <= 3
                && text[..offset]
                    .trim_start_matches(['(', '（'])
                    .chars()
                    .all(|c| c.is_ascii_digit() || is_chinese_number(c));
            (!numbered_prefix).then_some((offset, ch.len_utf8()))
        })
        .collect();
    let Some((first_offset, _)) = separators.first().copied() else {
        return vec![text];
    };
    let folded_prefix = fold_text(&text[..first_offset]);
    if !policy
        .outline
        .enumeration_prefix_terms
        .iter()
        .any(|term| folded_prefix.contains(&fold_text(term)))
    {
        return vec![text];
    }

    let mut separators = separators;
    separators.extend(
        text.char_indices()
            .filter(|(offset, ch)| *offset > first_offset && matches!(*ch, '和' | '及'))
            .map(|(offset, ch)| (offset, ch.len_utf8())),
    );
    separators.sort_unstable_by_key(|(offset, _)| *offset);
    let mut out = Vec::new();
    let mut start = 0;
    for (offset, width) in separators {
        let item = text[start..offset].trim();
        if !item.is_empty() {
            out.push(item);
        }
        start = offset + width;
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    if out.len() > 1 { out } else { vec![text] }
}

fn split_chars(text: &str, context: &str, max_chars: usize, out: &mut Vec<SpanBlock>) {
    let chars: Vec<char> = text.chars().collect();
    for chunk in chars.chunks(max_chars.max(1)) {
        let part: String = chunk.iter().collect();
        if !part.trim().is_empty() {
            out.push(SpanBlock {
                context: context.to_string(),
                body: part,
            });
        }
    }
}

fn is_list_item(line: &str) -> bool {
    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("• ") {
        return true;
    }
    let mut chars = line.chars();
    let mut saw_digit = false;
    for c in chars.by_ref() {
        if c.is_ascii_digit() {
            saw_digit = true;
            continue;
        }
        return saw_digit && matches!(c, '.' | '、' | ')' | '）');
    }
    false
}

fn numbered_requirement_line(policy: &ExtractionPolicy, line: &str) -> bool {
    let list_style = line.find([')', '）']).is_some_and(|pos| {
        pos > 0
            && line[..pos]
                .trim_start_matches(['(', '（'])
                .chars()
                .all(|c| c.is_ascii_digit() || is_chinese_number(c))
    });
    let numbered = numeric_prefix(line).is_some()
        || line
            .find('、')
            .is_some_and(|pos| pos > 0 && line[..pos].chars().all(is_chinese_number))
        || list_style;
    if !numbered {
        return false;
    }
    let folded = fold_text(line);
    if policy
        .outline
        .numbered_heading_suffixes
        .iter()
        .any(|suffix| folded.ends_with(&fold_text(suffix)))
    {
        return false;
    }
    let hard = policy
        .must
        .hard
        .iter()
        .any(|term| folded.contains(&fold_text(term)))
        || has_veto_term(policy, line);
    let predicate = policy
        .outline
        .numbered_requirement_predicates
        .iter()
        .any(|term| folded.contains(&fold_text(term)));
    hard || (line.chars().count() >= 8 && predicate)
}

fn is_chinese_number(c: char) -> bool {
    "一二三四五六七八九十百零〇".contains(c)
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    if line.is_empty() {
        return None;
    }
    if line.starts_with('#') {
        let level = line.chars().take_while(|c| *c == '#').count().clamp(1, 6);
        let title = line[level..].trim();
        return (!title.is_empty()).then(|| (level, title.to_string()));
    }
    if line.starts_with('第') && line.contains('章') {
        return Some((1, line.to_string()));
    }
    if line.starts_with('第') && line.contains('节') {
        return Some((2, line.to_string()));
    }
    if let Some((number, rest)) = numeric_prefix(line)
        && line.chars().count() <= 48
        && !line.contains(['。', '；', ';'])
    {
        let level = number.matches('.').count() + 1;
        let title = format!("{number} {}", rest.trim());
        return (!rest.trim().is_empty()).then(|| (level.min(6), title));
    }
    if let Some(pos) = line.find('、')
        && pos > 0
        && line[..pos].chars().all(is_chinese_number)
    {
        return Some((3, line.to_string()));
    }
    if ((line.starts_with('（') && line.contains('）'))
        || (line.starts_with('(') && line.contains(')')))
        && line
            .chars()
            .skip(1)
            .take_while(|c| *c != '）' && *c != ')')
            .all(is_chinese_number)
    {
        return Some((4, line.to_string()));
    }
    if let Some(pos) = line.find([')', '）'])
        && pos > 0
        && line[..pos].chars().all(|c| c.is_ascii_digit())
        && line.chars().count() <= 48
        && !line.contains(['。', '；', ';'])
    {
        return Some((5, line.to_string()));
    }
    if line.starts_with("**") && line.ends_with("**") && line.len() > 4 {
        return Some((3, line.trim_matches('*').trim().to_string()));
    }
    None
}

fn numeric_prefix(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }
    if i == 0 || i >= bytes.len() {
        return None;
    }
    let next = line[i..].chars().next()?;
    if !next.is_whitespace() && !matches!(next, '、' | ')' | '）') {
        return None;
    }
    let raw_number = &line[..i];
    let number = raw_number.strip_suffix('.').unwrap_or(raw_number);
    if number.is_empty() || number.ends_with('.') {
        return None;
    }
    Some((number, line[i..].trim_start_matches([' ', '、', ')', '）'])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::policy::default_policy;

    #[test]
    fn hierarchy_and_chinese_headings_are_stable() {
        let md =
            "# 第三章\n## 技术要求\n一、性能\n吞吐量不得低于40G。\n（一）接口\n- 支持万兆接口\n";
        let sections = build_sections(md, &ExtractionScope::Document, default_policy().unwrap());
        assert!(
            sections
                .iter()
                .any(|s| s.heading_path.contains("第三章 / 技术要求 / 一、性能"))
        );
        assert!(sections.iter().all(|s| !s.key.is_empty()));
        assert!(
            sections
                .iter()
                .flat_map(|s| &s.spans)
                .all(|span| !span.id.is_empty())
        );
    }

    #[test]
    fn one_quote_will_not_cover_sibling_spans() {
        let md = "# 技术要求\n第一段必须支持接口。\n\n第二段吞吐量不得低于40G。";
        let sections = build_sections(md, &ExtractionScope::Document, default_policy().unwrap());
        assert_eq!(sections[0].spans.len(), 2);
        assert!(sections[0].spans.iter().all(|s| s.candidate));
    }

    #[test]
    fn numbered_headings_and_requirements_are_distinguished() {
        let md = "1. 总则\n说明文字。\n1.2 技术要求\n1）设备支持万兆接口\n1.2. 性能指标\n2）吞吐量不得低于40G\n3. 设备应支持IPv6\n四、设备应兼容IPv6\n（五）设备应提供双电源";
        let sections = build_sections(md, &ExtractionScope::Document, default_policy().unwrap());
        assert!(
            sections
                .iter()
                .any(|section| section.heading_path.contains("1 总则"))
        );
        assert!(
            sections
                .iter()
                .any(|section| section.heading_path.contains("1.2 技术要求"))
        );
        let body = sections
            .iter()
            .flat_map(|section| section.spans.iter())
            .map(|span| span.body.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(body.contains("1）设备支持万兆接口"));
        assert!(body.contains("2）吞吐量不得低于40G"));
        assert!(body.contains("3. 设备应支持IPv6"));
        assert!(body.contains("四、设备应兼容IPv6"));
        assert!(body.contains("（五）设备应提供双电源"));
    }

    #[test]
    fn signal_bearing_numbered_headings_remain_outline_nodes() {
        let md = "1. 投标人资格要求\n投标人须具有营业执照。\n2. 设备接口要求\n设备应支持万兆接口。\n三、类似项目业绩要求\n近三年应具有类似项目业绩。";
        let sections = build_sections(md, &ExtractionScope::Document, default_policy().unwrap());
        let paths: Vec<_> = sections
            .iter()
            .map(|section| section.heading_path.as_str())
            .collect();
        assert!(paths.iter().any(|path| path.contains("1 投标人资格要求")));
        assert!(paths.iter().any(|path| path.contains("2 设备接口要求")));
        assert!(
            paths
                .iter()
                .any(|path| path.contains("三、类似项目业绩要求"))
        );
    }

    #[test]
    fn dense_prose_and_table_rows_are_requirement_sized() {
        let md = "# 技术要求\n设备必须支持万兆接口。吞吐量不得低于40G。\n\n| 指标 | 要求 |\n|---|---|\n| 接口 | 必须支持光口 |\n| 容量 | 容量不得低于1TB |";
        let sections = build_sections(md, &ExtractionScope::Document, default_policy().unwrap());
        let spans = &sections[0].spans;
        assert_eq!(spans.len(), 4, "{spans:?}");
        assert!(spans.iter().any(|span| span.body.contains("必须支持光口")));
        assert!(
            spans
                .iter()
                .any(|span| span.body.contains("容量不得低于1TB"))
        );
        let capacity = spans
            .iter()
            .find(|span| span.body.contains("容量不得低于1TB"))
            .unwrap();
        assert!(capacity.context.contains("| 指标 | 要求 |"));
        assert_eq!(capacity.body, "容量不得低于1TB");
        assert!(!md.contains(&format!("{}\n{}", capacity.context, capacity.body)));
    }

    #[test]
    fn key_value_tables_keep_exact_rows_and_headers_are_never_quotable() {
        let md = "# 技术要求\n| 指标 | 参数 |\n|---|---|\n| 最大响应时间 | 2秒 |\n| 端口数量 | 不少于24个 |";
        let sections = build_sections(md, &ExtractionScope::Document, default_policy().unwrap());
        let spans = &sections[0].spans;
        assert_eq!(spans.len(), 2, "{spans:?}");
        assert_eq!(spans[0].body, "| 最大响应时间 | 2秒 |");
        assert_eq!(spans[1].body, "| 端口数量 | 不少于24个 |");
        assert!(
            spans
                .iter()
                .all(|span| span.context.contains("| 指标 | 参数 |"))
        );

        let header_only = build_sections(
            "# 技术要求\n| 指标 | 参数 |\n|---|---|",
            &ExtractionScope::Document,
            default_policy().unwrap(),
        );
        assert!(header_only[0].spans.is_empty());
    }

    #[test]
    fn coordinated_requirements_are_separate_coverage_units() {
        let md = "# 技术要求\n设备必须支持万兆接口，并应提供双电源热插拔且不得使用弱算法。";
        let sections = build_sections(md, &ExtractionScope::Document, default_policy().unwrap());
        let spans = &sections[0].spans;
        assert_eq!(spans.len(), 3, "{spans:?}");
        assert!(spans.iter().all(|span| span.candidate));
        assert!(spans.iter().any(|span| span.body.contains("万兆接口")));
        assert!(spans.iter().any(|span| span.body.contains("双电源")));
        assert!(spans.iter().any(|span| span.body.contains("弱算法")));

        let inherited = build_sections(
            "# 技术要求\n设备必须支持万兆接口，并提供双电源热插拔。",
            &ExtractionScope::Document,
            default_policy().unwrap(),
        );
        assert_eq!(inherited[0].spans.len(), 2);
        assert!(inherited[0].spans[1].candidate);
        assert!(
            inherited[0].spans[1]
                .context
                .contains("设备必须支持万兆接口")
        );

        let enumerated = build_sections(
            "# 商务要求\n须提供营业执照、ISO认证和业绩证明。",
            &ExtractionScope::Document,
            default_policy().unwrap(),
        );
        assert_eq!(enumerated[0].spans.len(), 3, "{:?}", enumerated[0].spans);
        assert!(enumerated[0].spans.iter().all(|span| span.candidate));
        assert_eq!(enumerated[0].spans[1].body, "ISO认证");
        assert!(enumerated[0].spans[1].context.contains("须提供营业执照"));
    }

    #[test]
    fn outline_strategy_is_driven_by_policy() {
        let mut policy = default_policy().unwrap().clone();
        policy.outline.sentence_anchors = vec!["并自定义".into()];
        let sections = build_sections(
            "# 技术要求\n设备必须支持接口并自定义提供电源。",
            &ExtractionScope::Document,
            &policy,
        );
        assert_eq!(sections[0].spans.len(), 2);
    }

    #[test]
    fn oversized_key_value_row_is_preserved_but_not_offered_to_agent() {
        let policy = default_policy().unwrap();
        let value = "A".repeat(policy.limits.max_tool_output_chars + 100);
        let markdown = format!("| 指标 | 参数 |\n|---|---|\n| 容量 | {value} |");
        let sections = build_sections(&markdown, &ExtractionScope::Document, policy);
        assert_eq!(sections[0].spans.len(), 1);
        assert_eq!(sections[0].spans[0].body, format!("| 容量 | {value} |"));
        assert!(!sections[0].spans[0].candidate);
    }

    #[test]
    fn long_span_is_fully_partitioned() {
        let body = "须支持接口。".repeat(2000);
        let sections = build_sections(&body, &ExtractionScope::Document, default_policy().unwrap());
        assert!(sections[0].spans.len() > 1);
        assert!(
            sections[0]
                .spans
                .iter()
                .all(|s| s.body.chars().count() <= 8000)
        );
    }
}
