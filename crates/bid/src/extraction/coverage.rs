use std::collections::HashSet;

use super::outline::{has_veto_term, table_is_chrome};
use super::policy::{ExtractionPolicy, family_score, fold_text, hint_family, resolve_must};
use super::types::{
    CandidateClause, ClauseFamily, ExtractedClause, ExtractedSection, ExtractedSpan,
};

pub fn candidate_spans(sections: &[ExtractedSection]) -> Vec<&ExtractedSpan> {
    sections
        .iter()
        .flat_map(|section| section.spans.iter())
        .filter(|span| span.candidate)
        .collect()
}

pub fn uncovered_spans<'a>(
    sections: &'a [ExtractedSection],
    clauses: &[ExtractedClause],
) -> Vec<&'a ExtractedSpan> {
    let covered: HashSet<_> = clauses
        .iter()
        .map(|clause| clause.span_id.as_str())
        .collect();
    candidate_spans(sections)
        .into_iter()
        .filter(|span| !covered.contains(span.id.as_str()))
        .collect()
}

pub fn heuristic_candidates(
    policy: &ExtractionPolicy,
    spans: &[&ExtractedSpan],
) -> Vec<CandidateClause> {
    let mut out = Vec::new();
    for span in spans {
        for sentence in requirement_sentences(&span.body) {
            let evidence = if span.context.is_empty() {
                sentence.to_string()
            } else {
                format!("{}\n{}", span.context, sentence)
            };
            if !looks_like_requirement(policy, &span.heading_path, &evidence) {
                continue;
            }
            let body_technical = family_score(policy, ClauseFamily::Technical, "", &evidence);
            let body_commercial = family_score(policy, ClauseFamily::Commercial, "", &evidence);
            let heading_technical =
                family_score(policy, ClauseFamily::Technical, &span.heading_path, "");
            let heading_commercial =
                family_score(policy, ClauseFamily::Commercial, &span.heading_path, "");
            let (technical, commercial) = if body_technical == body_commercial {
                (heading_technical, heading_commercial)
            } else {
                (body_technical, body_commercial)
            };
            let must = resolve_must(policy, &evidence, false);
            match technical.cmp(&commercial) {
                std::cmp::Ordering::Greater => {
                    out.push(candidate(span, sentence, ClauseFamily::Technical, must))
                }
                std::cmp::Ordering::Less => {
                    out.push(candidate(span, sentence, ClauseFamily::Commercial, must))
                }
                std::cmp::Ordering::Equal => {
                    out.push(candidate(span, sentence, ClauseFamily::Technical, must));
                    out.push(candidate(span, sentence, ClauseFamily::Commercial, must));
                }
            }
        }
    }
    out
}

fn candidate(
    span: &ExtractedSpan,
    sentence: &str,
    family: ClauseFamily,
    must: bool,
) -> CandidateClause {
    CandidateClause {
        span_id: span.id.clone(),
        quote: sentence.to_string(),
        text: sentence.to_string(),
        must,
        proposed_family: family,
        extractor: "heuristic".into(),
    }
}

fn looks_like_requirement(policy: &ExtractionPolicy, heading: &str, text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() < 6
        || trimmed
            .chars()
            .all(|c| matches!(c, '-' | '|' | ':' | '：' | ' '))
    {
        return false;
    }
    if table_is_chrome(policy, trimmed) {
        return false;
    }
    let folded = fold_text(trimmed);
    let body_signal = family_score(policy, ClauseFamily::Technical, "", trimmed) > 0
        || family_score(policy, ClauseFamily::Commercial, "", trimmed) > 0;
    let triggered = policy
        .coverage
        .trigger_terms
        .iter()
        .any(|term| folded.contains(&fold_text(term)));
    if hint_family(policy, heading) == "skip" {
        return body_signal || has_veto_term(policy, trimmed);
    }
    triggered || body_signal
}

fn requirement_sentences(body: &str) -> Vec<&str> {
    let trimmed = body.trim();
    if trimmed.starts_with('|') && trimmed.ends_with('|') && !trimmed.contains('\n') {
        return vec![trimmed];
    }
    body.split(['。', '；', ';', '\n'])
        .map(str::trim)
        .filter(|part| !part.is_empty() && !part.contains("---"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::outline::build_sections;
    use crate::extraction::policy::default_policy;
    use crate::extraction::types::ExtractionScope;

    #[test]
    fn heuristic_runs_per_uncovered_span() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "# 技术要求\n必须支持万兆接口。\n\n吞吐量不得低于40G。",
            &ExtractionScope::Document,
            policy,
        );
        let spans = candidate_spans(&sections);
        let clauses = heuristic_candidates(policy, &spans);
        assert!(clauses.iter().any(|clause| clause.quote.contains("万兆")));
        assert!(clauses.iter().any(|clause| clause.quote.contains("40G")));
    }

    #[test]
    fn optional_phrase_is_not_must() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "# 技术要求\n优先支持万兆接口。",
            &ExtractionScope::Document,
            policy,
        );
        let spans = candidate_spans(&sections);
        let clauses = heuristic_candidates(policy, &spans);
        assert!(clauses.iter().all(|clause| !clause.must));
        assert!(!resolve_must(policy, "该接口无需额外许可", true));
    }

    #[test]
    fn neutral_inventory_table_is_not_a_requirement_under_family_headings() {
        let policy = default_policy().unwrap();
        for heading in ["技术参数", "商务要求"] {
            let markdown = format!("# {heading}\n| 序号 | 名称 |\n|---|---|\n| 1 | 路由器 |");
            let sections = build_sections(&markdown, &ExtractionScope::Document, policy);
            assert!(candidate_spans(&sections).is_empty(), "heading={heading}");
        }
    }

    #[test]
    fn technical_key_value_row_remains_an_exact_candidate() {
        let policy = default_policy().unwrap();
        for row in [
            "| 最大响应时间 | 2秒；峰值不超过3秒 |",
            "| 最大响应时间 | 2秒;峰值不超过3秒 |",
        ] {
            let markdown = format!("# 技术参数\n| 指标 | 参数 |\n|---|---|\n{row}");
            let sections = build_sections(&markdown, &ExtractionScope::Document, policy);
            let spans = candidate_spans(&sections);
            assert_eq!(spans.len(), 1);
            let clauses = heuristic_candidates(policy, &spans);
            assert_eq!(clauses[0].quote, row);
        }
    }
}
