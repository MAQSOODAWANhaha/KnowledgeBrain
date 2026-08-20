use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::policy::{ExtractionPolicy, family_score, resolve_must};
use super::types::{CandidateClause, ClauseFamily, ExtractedClause, ExtractedSection};

pub fn normalize_quote(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{3000}' | '\t' | '\n' | '\r' => ' ',
            '，' => ',',
            '。' => '.',
            '：' => ':',
            '；' => ';',
            _ => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("")
}

pub fn quote_in_body(quote: &str, body: &str) -> bool {
    !quote.is_empty() && body.contains(quote)
}

pub fn quotes_overlap(a: &str, b: &str) -> bool {
    let a = normalize_quote(a);
    let b = normalize_quote(b);
    !a.is_empty() && !b.is_empty() && (a == b || a.contains(&b) || b.contains(&a))
}

pub struct ReconcileResult {
    pub clauses: Vec<ExtractedClause>,
    pub rejected_invalid_quotes: usize,
    pub family_conflicts: usize,
}

pub fn reconcile_candidates(
    policy: &ExtractionPolicy,
    sections: &[ExtractedSection],
    candidates: Vec<CandidateClause>,
) -> ReconcileResult {
    let spans: HashMap<_, _> = sections
        .iter()
        .flat_map(|section| section.spans.iter())
        .map(|span| (span.id.as_str(), span))
        .collect();
    let mut rejected = 0;
    let mut valid = Vec::new();
    for candidate in candidates {
        let Some(span) = spans.get(candidate.span_id.as_str()) else {
            rejected += 1;
            continue;
        };
        let quote_valid = if span.body.contains('|') {
            candidate.quote == span.body
        } else {
            quote_in_body(&candidate.quote, &span.body)
        };
        if candidate.text.trim().is_empty() || !quote_valid {
            rejected += 1;
            continue;
        }
        valid.push(candidate);
    }

    let mut groups: Vec<Vec<CandidateClause>> = Vec::new();
    'candidate: for candidate in valid {
        for group in &mut groups {
            if group.iter().any(|existing| {
                existing.span_id == candidate.span_id
                    && quotes_overlap(&existing.quote, &candidate.quote)
            }) {
                group.push(candidate);
                continue 'candidate;
            }
        }
        groups.push(vec![candidate]);
    }

    let mut out = Vec::new();
    let mut conflicts = 0;
    for group in groups {
        let representative = group
            .iter()
            .max_by_key(|candidate| normalize_quote(&candidate.quote).chars().count())
            .expect("candidate group is non-empty");
        let Some(span) = spans.get(representative.span_id.as_str()) else {
            continue;
        };
        let proposed: HashSet<_> = group.iter().map(|item| item.proposed_family).collect();
        let (family, conflict) = if proposed.len() == 1 {
            (*proposed.iter().next().unwrap(), false)
        } else {
            choose_family(policy, &span.heading_path, &representative.quote, &group)
        };
        if conflict {
            conflicts += 1;
        }
        let proposed_must = group.iter().any(|item| item.must);
        let must = resolve_must(policy, &representative.quote, proposed_must);
        let mut extractors: Vec<_> = group.iter().map(|item| item.extractor.clone()).collect();
        extractors.sort();
        extractors.dedup();
        let mut families: Vec<_> = proposed
            .iter()
            .map(|family| family.as_str().to_string())
            .collect();
        families.sort();
        out.push(ExtractedClause {
            section_key: span.section_key.clone(),
            span_id: span.id.clone(),
            heading_path: span.heading_path.clone(),
            quote: representative.quote.clone(),
            text: representative.quote.clone(),
            family: family.as_str().into(),
            must,
            family_conflict: conflict,
            extraction_meta: json!({
                "proposed_families": families,
                "extractors": extractors,
                "policy_version": policy.version,
                "prompt_version": policy.prompt_version
            }),
        });
    }
    out.sort_by(|a, b| a.span_id.cmp(&b.span_id).then(a.quote.cmp(&b.quote)));
    if out.len() > policy.limits.max_file_clauses {
        out.truncate(policy.limits.max_file_clauses);
    }
    ReconcileResult {
        clauses: out,
        rejected_invalid_quotes: rejected,
        family_conflicts: conflicts,
    }
}

fn choose_family(
    policy: &ExtractionPolicy,
    heading_path: &str,
    quote: &str,
    group: &[CandidateClause],
) -> (ClauseFamily, bool) {
    // Arbitration order is part of the versioned extraction contract:
    // heading prior, body signals, then a server-owned extractor rank.
    match super::policy::hint_family(policy, heading_path) {
        "technical" => return (ClauseFamily::Technical, false),
        "commercial" => return (ClauseFamily::Commercial, false),
        _ => {}
    }
    let technical = family_score(policy, ClauseFamily::Technical, "", quote);
    let commercial = family_score(policy, ClauseFamily::Commercial, "", quote);
    match technical.cmp(&commercial) {
        std::cmp::Ordering::Greater => return (ClauseFamily::Technical, false),
        std::cmp::Ordering::Less => return (ClauseFamily::Commercial, false),
        std::cmp::Ordering::Equal => {}
    }
    let rank = |family| {
        group
            .iter()
            .filter(|candidate| candidate.proposed_family == family)
            .map(|candidate| extractor_rank(&candidate.extractor))
            .max()
            .unwrap_or_default()
    };
    match rank(ClauseFamily::Technical).cmp(&rank(ClauseFamily::Commercial)) {
        std::cmp::Ordering::Greater => (ClauseFamily::Technical, false),
        std::cmp::Ordering::Less => (ClauseFamily::Commercial, false),
        std::cmp::Ordering::Equal => (ClauseFamily::Technical, true),
    }
}

fn extractor_rank(extractor: &str) -> u8 {
    if extractor.ends_with("_agent") {
        3
    } else if extractor.ends_with("_span_sweep") {
        2
    } else if extractor == "heuristic" || extractor.ends_with("_heuristic") {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::outline::build_sections;
    use crate::extraction::policy::default_policy;
    use crate::extraction::types::ExtractionScope;

    #[test]
    fn cross_family_tie_is_visible() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "正文要求投标人提供说明。",
            &ExtractionScope::Document,
            policy,
        );
        let span = &sections[0].spans[0];
        let candidates = vec![
            CandidateClause {
                span_id: span.id.clone(),
                quote: "投标人提供说明".into(),
                text: "提供说明".into(),
                must: false,
                proposed_family: ClauseFamily::Technical,
                extractor: "technical_agent".into(),
            },
            CandidateClause {
                span_id: span.id.clone(),
                quote: "投标人提供说明".into(),
                text: "提供说明".into(),
                must: false,
                proposed_family: ClauseFamily::Commercial,
                extractor: "commercial_agent".into(),
            },
        ];
        let result = reconcile_candidates(policy, &sections, candidates);
        assert_eq!(result.family_conflicts, 1);
        assert!(result.clauses[0].family_conflict);
    }

    #[test]
    fn heading_prior_precedes_contradictory_body_signal() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "# 技术要求\n注册资本不得低于人民币500万元。",
            &ExtractionScope::Document,
            policy,
        );
        let span = &sections[0].spans[0];
        let candidates = ClauseFamily::ALL
            .into_iter()
            .map(|family| CandidateClause {
                span_id: span.id.clone(),
                quote: "注册资本不得低于人民币500万元".into(),
                text: "注册资本不得低于人民币500万元".into(),
                must: true,
                proposed_family: family,
                extractor: format!("{}_agent", family.as_str()),
            })
            .collect();
        let result = reconcile_candidates(policy, &sections, candidates);
        assert_eq!(result.clauses[0].family, "technical");
        assert!(!result.clauses[0].family_conflict);
    }

    #[test]
    fn server_owned_extractor_rank_breaks_an_unknown_heading_tie() {
        let policy = default_policy().unwrap();
        let sections = build_sections("投标人提供说明。", &ExtractionScope::Document, policy);
        let span = &sections[0].spans[0];
        let candidates = vec![
            CandidateClause {
                span_id: span.id.clone(),
                quote: "投标人提供说明".into(),
                text: "ignored normalization".into(),
                must: false,
                proposed_family: ClauseFamily::Technical,
                extractor: "technical_agent".into(),
            },
            CandidateClause {
                span_id: span.id.clone(),
                quote: "投标人提供说明".into(),
                text: "ignored normalization".into(),
                must: false,
                proposed_family: ClauseFamily::Commercial,
                extractor: "commercial_span_sweep".into(),
            },
        ];
        let result = reconcile_candidates(policy, &sections, candidates);
        assert_eq!(result.clauses[0].family, "technical");
        assert!(!result.clauses[0].family_conflict);
        assert_eq!(result.clauses[0].text, result.clauses[0].quote);
    }

    #[test]
    fn quote_must_be_exact_continuous_source_text() {
        assert!(quote_in_body("须支持万兆接口", "设备须支持万兆接口。"));
        assert!(!quote_in_body("须 支持万兆接口", "设备须支持万兆接口。"));
        assert!(!quote_in_body("须支持万兆接口.", "设备须支持万兆接口。"));
        assert!(quotes_overlap("须 支持万兆接口.", "须支持万兆接口。"));
    }

    #[test]
    fn synthesized_table_context_is_not_quotable_source() {
        let policy = default_policy().unwrap();
        let markdown =
            "| 指标 | 要求 |\n|---|---|\n| 接口 | 必须支持光口 |\n| 容量 | 容量不得低于1TB |";
        let sections = build_sections(markdown, &ExtractionScope::Document, policy);
        let span = sections
            .iter()
            .flat_map(|section| &section.spans)
            .find(|span| span.body.contains("容量不得低于1TB"))
            .unwrap();
        assert!(!span.context.is_empty());
        let result = reconcile_candidates(
            policy,
            &sections,
            vec![CandidateClause {
                span_id: span.id.clone(),
                quote: format!("{}\n{}", span.context, span.body),
                text: "容量不得低于1TB".into(),
                must: true,
                proposed_family: ClauseFamily::Technical,
                extractor: "technical_agent".into(),
            }],
        );
        assert_eq!(result.rejected_invalid_quotes, 1);
        assert!(result.clauses.is_empty());
    }

    #[test]
    fn key_value_table_requires_the_exact_row_quote() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "| 指标 | 参数 |\n|---|---|\n| 最大响应时间 | 2秒 |",
            &ExtractionScope::Document,
            policy,
        );
        let span = &sections[0].spans[0];
        let candidates = vec![
            CandidateClause {
                span_id: span.id.clone(),
                quote: "最大响应时间".into(),
                text: "最大响应时间".into(),
                must: true,
                proposed_family: ClauseFamily::Technical,
                extractor: "technical_agent".into(),
            },
            CandidateClause {
                span_id: span.id.clone(),
                quote: span.body.clone(),
                text: span.body.clone(),
                must: true,
                proposed_family: ClauseFamily::Technical,
                extractor: "technical_agent".into(),
            },
        ];
        let result = reconcile_candidates(policy, &sections, candidates);
        assert_eq!(result.rejected_invalid_quotes, 1);
        assert_eq!(result.clauses[0].quote, span.body);
        assert!(!result.clauses[0].must);
    }

    #[test]
    fn invalid_quote_is_rejected() {
        let policy = default_policy().unwrap();
        let sections = build_sections("必须支持万兆接口。", &ExtractionScope::Document, policy);
        let span = &sections[0].spans[0];
        let result = reconcile_candidates(
            policy,
            &sections,
            vec![CandidateClause {
                span_id: span.id.clone(),
                quote: "支持量子接口".into(),
                text: "支持量子接口".into(),
                must: true,
                proposed_family: ClauseFamily::Technical,
                extractor: "technical_agent".into(),
            }],
        );
        assert_eq!(result.rejected_invalid_quotes, 1);
        assert!(result.clauses.is_empty());
    }
}
