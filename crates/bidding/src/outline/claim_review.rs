//! Bounded claim/evidence comparisons. Selection marks identify source context,
//! never its business meaning; the reviewer decides applicability and polarity.
use super::{
    discover::RequirementRecord,
    evidence::{EvidenceRef, SourceExcerpt, input_digest, resolve_evidence},
};
use crate::analysis::FrozenInput;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub quote_handle: String,
    pub role: String,
    pub excerpts: Vec<SourceExcerpt>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub claim_handle: String,
    pub original_text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewUnit {
    pub version: String,
    pub requirement_id: String,
    pub original_requirement: RequirementRecord,
    pub claims: Vec<Claim>,
    pub evidence_units: Vec<Unit>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub quote_handle: String,
    pub polarity: String,
    pub applicability: String,
    pub condition: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub claim_handle: String,
    pub evidence_handles: Vec<String>,
    pub verdict: String,
    pub action: String,
    pub resulting_claim: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredClaim {
    pub claim_handle: String,
    pub obligation: String,
    pub applicability: String,
    pub original_fragments: Vec<String>,
    pub primary_handles: Vec<String>,
    pub support_handles: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub requirement_id: String,
    pub version: String,
    pub declared_claims: Vec<DeclaredClaim>,
    pub observations: Vec<Observation>,
    pub decisions: Vec<Decision>,
}

fn intersects_original(reference: &EvidenceRef, original: &EvidenceRef) -> bool {
    reference.same_carrier(original)
        && match (reference.range(), original.range()) {
            (Some((left, right)), Some((start, end))) => left < end && start < right,
            (None, None) => true,
            _ => false,
        }
}

pub fn build(
    input: &FrozenInput,
    id: &str,
    record: &RequirementRecord,
) -> Result<ReviewUnit, String> {
    resolve_evidence(input, &record.evidence)?;
    let digest = input_digest(input)?;
    let mut groups: Vec<(String, Vec<EvidenceRef>)> = Vec::new();
    let mut seen = BTreeSet::new();
    for evidence in record.evidence.iter().chain(
        record
            .condition_support
            .iter()
            .flat_map(|support| support.evidence.iter()),
    ) {
        let refs = match evidence {
            EvidenceRef::Text { unit_id, .. } => {
                let source = input
                    .source_units
                    .iter()
                    .find(|s| s.source_unit_revision_id == *unit_id)
                    .ok_or("source missing")?;
                vec![EvidenceRef::Text {
                    input_digest: digest.clone(),
                    unit_id: unit_id.clone(),
                    start_byte: 0,
                    end_byte: source.text.len(),
                }]
            }
            EvidenceRef::GridCell {
                table_id,
                anchor_row,
                ..
            } => super::source_support::row_body(input, table_id, *anchor_row)?,
            EvidenceRef::ImageRegion { .. } => vec![evidence.clone()],
        };
        let key = super::canonical_sha256(&refs)?;
        if seen.insert(key) {
            let role = if record.evidence.contains(evidence) {
                "requirement_source"
            } else {
                "condition_support"
            };
            groups.push((role.into(), refs));
        }
    }
    // Scope retrieval to the cited documents. Clause identifiers and rare label
    // phrases link complete rows; checkbox glyphs alone never make a row relevant.
    let mut documents = BTreeSet::new();
    let mut owner_ids = BTreeSet::new();
    let mut source_text = record.description.clone();
    for group in &groups {
        for reference in &group.1 {
            let source_id = match reference {
                EvidenceRef::Text { unit_id, .. } => Some(unit_id.as_str()),
                EvidenceRef::GridCell { table_id, .. } => input
                    .structured_forms
                    .iter()
                    .find(|f| f["form_definition_revision_id"] == *table_id)
                    .and_then(|f| f["source_unit_revision_id"].as_str()),
                _ => None,
            };
            if let Some(source) = source_id.and_then(|id| {
                input
                    .source_units
                    .iter()
                    .find(|s| s.source_unit_revision_id == id)
            }) {
                documents.insert(source.document_id.clone());
                owner_ids.insert(source.source_unit_revision_id.clone());
                source_text.push('\n');
                source_text.push_str(&source.text);
            }
        }
    }
    // A cell's complete row can still omit a separate applicability paragraph.
    // Show native structural context as candidates; comparisons must decide its
    // applicability and fresh Check reads remain mandatory for any support.
    for source in super::discover::section_context_sources(input, &owner_ids) {
        let refs = vec![EvidenceRef::Text {
            input_digest: digest.clone(),
            unit_id: source.source_unit_revision_id.clone(),
            start_byte: 0,
            end_byte: source.text.len(),
        }];
        if seen.insert(super::canonical_sha256(&refs)?) {
            groups.push(("related_section_context".into(), refs));
        }
    }
    let clause = regex::Regex::new(r"(?m)^[ \t#]*([0-9]+(?:[ \t]*\.[ \t]*[0-9]+)+)")
        .map_err(|e| e.to_string())?;
    let codes: BTreeSet<String> = clause
        .captures_iter(&source_text)
        .map(|c| c[1].chars().filter(|c| !c.is_whitespace()).collect())
        .collect();
    let grams = |s: &str| -> BTreeSet<String> {
        let chars: Vec<_> = s.chars().filter(|c| !c.is_whitespace()).collect();
        chars.windows(4).map(|w| w.iter().collect()).collect()
    };
    let mut candidates = Vec::new();
    let mut frequencies = std::collections::BTreeMap::<String, usize>::new();
    for form in &input.structured_forms {
        let Some(table) = form["form_definition_revision_id"].as_str() else {
            continue;
        };
        let Some(source) = form["source_unit_revision_id"].as_str().and_then(|id| {
            input
                .source_units
                .iter()
                .find(|s| s.source_unit_revision_id == id)
        }) else {
            continue;
        };
        if !documents.contains(&source.document_id) {
            continue;
        }
        let Some(cells) = form["definition"]["cells"].as_array() else {
            continue;
        };
        let rows: BTreeSet<_> = cells.iter().filter_map(|c| c["row"].as_u64()).collect();
        for row in rows {
            let row_cells = cells
                .iter()
                .filter(|c| c["row"].as_u64() == Some(row))
                .collect::<Vec<_>>();
            let last = row_cells
                .iter()
                .filter_map(|c| c["column"].as_u64())
                .max()
                .unwrap_or(0);
            let label = row_cells
                .iter()
                .filter(|c| c["column"].as_u64().unwrap_or(last) < last)
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let terms = grams(&label);
            for term in &terms {
                *frequencies.entry(term.clone()).or_default() += 1;
            }
            let row_code = clause.captures(&label).map(|c| {
                c[1].chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
            });
            candidates.push((table.to_owned(), row as usize, terms, row_code));
        }
    }
    let source_grams = grams(&source_text);
    for (table, row, terms, code) in candidates {
        let clause_match = code.is_some_and(|code| codes.contains(&code));
        let label_match = terms
            .iter()
            .any(|t| frequencies[t] == 1 && source_grams.contains(t));
        if !clause_match && !label_match {
            continue;
        }
        let refs = super::source_support::row_body(input, &table, row)?;
        let key = super::canonical_sha256(&refs)?;
        if seen.insert(key) {
            groups.push((
                if clause_match {
                    "related_clause_row"
                } else {
                    "related_label_candidate"
                }
                .into(),
                refs,
            ));
        }
    }
    // Rebuild structural support for Check from canonical sources. A discovered
    // support range is a separate auxiliary quote, never another primary owner.
    // Selection is one hop; added support is not recursively expanded.
    let originals = groups
        .iter()
        .flat_map(|(_, refs)| refs.iter().cloned())
        .collect::<Vec<_>>();
    let mut supports = super::source_support::select(input, &record.evidence)?;
    supports.extend(super::source_support::select_structure(input, &originals)?);
    for support in supports {
        let refs = vec![support.evidence];
        if seen.insert(super::canonical_sha256(&refs)?) {
            groups.push(("related_structural_context".into(), refs));
        }
    }
    let evidence_units = groups
        .into_iter()
        .enumerate()
        .map(|(n, (role, refs))| {
            Ok(Unit {
                quote_handle: format!("q{n}"),
                role,
                excerpts: resolve_evidence(input, &refs)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let claims = vec![Claim {
        claim_handle: "requirement".into(),
        original_text: record.description.clone(),
    }];
    let version = super::canonical_sha256(&(&digest, id, record, &claims, &evidence_units))?;
    Ok(ReviewUnit {
        version,
        requirement_id: id.into(),
        original_requirement: record.clone(),
        claims,
        evidence_units,
    })
}

pub(crate) fn condition_support_options(
    input: &FrozenInput,
    unit: &ReviewUnit,
    comparison: &Comparison,
    reads: &[EvidenceRef],
) -> Result<Vec<super::discover::ConditionSupport>, String> {
    validate(unit, comparison)?;
    let mut options = Vec::new();
    for related in &unit.evidence_units {
        if !related.role.starts_with("related_") && related.role != "condition_support" {
            continue;
        }
        if !comparison
            .observations
            .iter()
            .any(|o| o.quote_handle == related.quote_handle && o.applicability == "relevant")
        {
            continue;
        }
        if !comparison
            .decisions
            .iter()
            .any(|d| d.evidence_handles.contains(&related.quote_handle))
        {
            continue;
        }
        let evidence = related
            .excerpts
            .iter()
            .map(|e| e.evidence.clone())
            .collect::<Vec<_>>();
        super::evidence::validate_evidence(&evidence, input, reads)
            .map_err(|e| format!("condition support requires current Check reads: {e}"))?;
        options.push(super::discover::ConditionSupport {
            origin: super::discover::ConditionSupportOrigin::ReviewedRequirement {
                requirement_id: unit.requirement_id.clone(),
            },
            review_version: unit.version.clone(),
            evidence,
        });
    }
    Ok(options)
}

pub fn validate(unit: &ReviewUnit, comparison: &Comparison) -> Result<bool, String> {
    if comparison.requirement_id != unit.requirement_id || comparison.version != unit.version {
        return Err("claim comparison identity changed".into());
    }
    let expected: BTreeSet<_> = unit
        .evidence_units
        .iter()
        .map(|u| u.quote_handle.as_str())
        .collect();
    let observed: BTreeSet<_> = comparison
        .observations
        .iter()
        .map(|o| o.quote_handle.as_str())
        .collect();
    if observed != expected || observed.len() != comparison.observations.len() {
        return Err("every complete evidence unit needs one observation".into());
    }
    for o in &comparison.observations {
        if ![
            "required",
            "not_required",
            "conditional",
            "optional",
            "unclear",
            "mixed",
        ]
        .contains(&o.polarity.as_str())
            || !["relevant", "not_relevant", "unclear"].contains(&o.applicability.as_str())
        {
            return Err("invalid polarity or applicability".into());
        }
    }
    let claims: BTreeSet<_> = comparison
        .declared_claims
        .iter()
        .map(|c| c.claim_handle.as_str())
        .collect();
    let decisions: BTreeSet<_> = comparison
        .decisions
        .iter()
        .map(|d| d.claim_handle.as_str())
        .collect();
    if claims.is_empty()
        || claims.len() != comparison.declared_claims.len()
        || claims != decisions
        || claims.len() != comparison.decisions.len()
    {
        return Err("each explicitly declared independent claim needs exactly one decision".into());
    }
    let text = &unit.original_requirement.description;
    let mut covered = vec![false; text.len()];
    for claim in &comparison.declared_claims {
        if claim.claim_handle.trim().is_empty()
            || claim.obligation.trim().is_empty()
            || claim.applicability.trim().is_empty()
            || claim.primary_handles.is_empty()
            || claim.original_fragments.is_empty()
        {
            return Err(
                "claim requires obligation, applicability, original fragments and primary evidence"
                    .into(),
            );
        }
        let references = claim
            .primary_handles
            .iter()
            .chain(&claim.support_handles)
            .collect::<Vec<_>>();
        if references.iter().any(|h| !expected.contains(h.as_str()))
            || references.iter().collect::<BTreeSet<_>>().len() != references.len()
        {
            return Err("claim evidence identity is invalid or duplicated".into());
        }
        for handle in &claim.primary_handles {
            let primary = unit
                .evidence_units
                .iter()
                .find(|quote| quote.quote_handle == *handle)
                .expect("validated quote handle");
            if primary.role != "requirement_source"
                || !primary.excerpts.iter().any(|excerpt| {
                    unit.original_requirement
                        .evidence
                        .iter()
                        .any(|original| intersects_original(&excerpt.evidence, original))
                })
            {
                return Err("primary evidence must intersect the original requirement source; auxiliary quotes remain support".into());
            }
        }
        for fragment in &claim.original_fragments {
            if fragment.is_empty() {
                return Err("empty original claim fragment".into());
            }
            let matches = text.match_indices(fragment).collect::<Vec<_>>();
            if matches.is_empty() {
                return Err("claim fragment is not exact original requirement text".into());
            }
            for (start, part) in matches {
                covered[start..start + part.len()].fill(true);
            }
        }
    }
    if covered.iter().any(|covered| !*covered) {
        return Err(
            "declared claims must cover the complete original requirement without omissions".into(),
        );
    }
    let mut supported = true;
    for d in &comparison.decisions {
        let claim = comparison
            .declared_claims
            .iter()
            .find(|c| c.claim_handle == d.claim_handle)
            .unwrap();
        if claim
            .primary_handles
            .iter()
            .chain(&claim.support_handles)
            .any(|h| !d.evidence_handles.contains(h))
        {
            return Err("decision must judge all declared primary and supporting evidence".into());
        }
        if d.evidence_handles.is_empty()
            || d.evidence_handles
                .iter()
                .any(|h| !expected.contains(h.as_str()))
        {
            return Err("decision lacks valid quote handles".into());
        }
        let valid = match d.verdict.as_str() {
            "supports" => d.action == "retain",
            "contradicts" => [
                "retract_positive_obligation",
                "mark_not_applicable",
                "narrow_condition",
                "manual_review",
            ]
            .contains(&d.action.as_str()),
            "uncertain" => d.action == "manual_review",
            _ => false,
        };
        if !valid {
            return Err(format!(
                "claim {}: {} allows {}; preserve the evidence verdict, choose its action; manual_review remains unresolved",
                d.claim_handle,
                d.verdict,
                match d.verdict.as_str() {
                    "supports" => "retain",
                    "contradicts" =>
                        "retract_positive_obligation|mark_not_applicable|narrow_condition|manual_review",
                    "uncertain" => "manual_review",
                    _ => "valid verdict required",
                }
            ));
        }
        if d.verdict == "supports"
            && !claim.primary_handles.iter().any(|handle| {
                comparison.observations.iter().any(|observation| {
                    observation.quote_handle == *handle && observation.applicability == "relevant"
                })
            })
        {
            return Err(
                "supports/retain requires explicitly relevant original primary evidence".into(),
            );
        }
        supported &= d.verdict == "supports";
    }
    // Applicability is model-declared, but an explicitly relevant unit cannot
    // disappear from every decision while the comparison claims full support.
    // Unresolved comparisons may retain the omission for a subsequent repair.
    if supported
        && comparison.observations.iter().any(|observation| {
            observation.applicability == "relevant"
                && !comparison.decisions.iter().any(|decision| {
                    decision
                        .evidence_handles
                        .contains(&observation.quote_handle)
                })
        })
    {
        return Err("a supported comparison must judge every explicitly relevant evidence unit, including conditions and exceptions".into());
    }
    Ok(supported)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (FrozenInput, RequirementRecord) {
        let mut input = crate::outline::tests::input();
        input.source_units[0].text = "4.2.2 提交材料。可选附加说明。".into();
        let mut table_source = input.source_units[0].clone();
        table_source.source_unit_revision_id = "table-source".into();
        table_source.text = String::new();
        input.source_units.push(table_source);
        input.structured_forms = vec![
            json!({"source_unit_revision_id":"table-source","form_definition_revision_id":"table","definition":{"row_count":2,"column_count":3,"cells":[
   {"row":0,"column":0,"row_span":1,"col_span":1,"text":"4.2.2","header_role":"none"},{"row":0,"column":1,"row_span":1,"col_span":1,"text":"材料适用范围","header_role":"none"},{"row":0,"column":2,"row_span":1,"col_span":1,"text":"本项目不适用；条件成立时才提交。","header_role":"none"},
   {"row":1,"column":0,"row_span":1,"col_span":1,"text":"9.9.9","header_role":"none"},{"row":1,"column":1,"row_span":1,"col_span":1,"text":"无关其他事项","header_role":"none"},{"row":1,"column":2,"row_span":1,"col_span":1,"text":"☑是 □否","header_role":"none"}]}}),
        ];
        let record = RequirementRecord {
            condition_support: Vec::new(),
            obligation_strength: "mandatory".into(),
            extraction_quality: "explicit".into(),
            description: "提交材料；可选附加说明。".into(),
            evidence: vec![EvidenceRef::Text {
                input_digest: input_digest(&input).unwrap(),
                unit_id: "source".into(),
                start_byte: 0,
                end_byte: 5,
            }],
            source_section_id: "section".into(),
            kind: "material".into(),
            dedup_group: "fixture".into(),
        };
        (input, record)
    }
    #[test]
    fn related_unmarked_row_is_complete_and_unrelated_marked_row_is_excluded() {
        let (input, record) = fixture();
        let unit = build(&input, "r", &record).unwrap();
        assert_eq!(unit.evidence_units.len(), 2);
        assert_eq!(
            unit.evidence_units[0].excerpts[0].quote,
            input.source_units[0].text
        );
        assert_eq!(unit.evidence_units[1].excerpts.len(), 3);
        assert!(unit.evidence_units[1].excerpts[2].quote.contains("不适用"));
        assert_eq!(unit.claims.len(), 1);
        let mut foreign = record;
        foreign.evidence[0] = EvidenceRef::Text {
            input_digest: "wrong".into(),
            unit_id: "source".into(),
            start_byte: 0,
            end_byte: 5,
        };
        assert!(build(&input, "r", &foreign).is_err());
    }
    #[test]
    fn comparison_rejects_skips_forged_handles_stale_versions_and_uncertainty_as_permission() {
        let (input, record) = fixture();
        let unit = build(&input, "r", &record).unwrap();
        let mut comparison = Comparison {
            declared_claims: vec![DeclaredClaim {
                claim_handle: "requirement".into(),
                obligation: unit.original_requirement.description.clone(),
                applicability: "fixture scope".into(),
                original_fragments: vec![unit.original_requirement.description.clone()],
                primary_handles: vec!["q0".into()],
                support_handles: vec!["q1".into()],
            }],
            requirement_id: "r".into(),
            version: unit.version.clone(),
            observations: unit
                .evidence_units
                .iter()
                .map(|u| Observation {
                    quote_handle: u.quote_handle.clone(),
                    polarity: "mixed".into(),
                    applicability: "relevant".into(),
                    condition: "source conditions".into(),
                })
                .collect(),
            decisions: unit
                .claims
                .iter()
                .map(|c| Decision {
                    claim_handle: c.claim_handle.clone(),
                    evidence_handles: vec!["q0".into(), "q1".into()],
                    verdict: "supports".into(),
                    action: "retain".into(),
                    resulting_claim: c.original_text.clone(),
                })
                .collect(),
        };
        let reads = unit
            .evidence_units
            .iter()
            .flat_map(|u| u.excerpts.iter().map(|e| e.evidence.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            condition_support_options(&input, &unit, &comparison, &reads)
                .unwrap()
                .len(),
            1,
            "supporting decisions also preserve applicable conditions"
        );
        assert!(
            condition_support_options(&input, &unit, &comparison, &[]).is_err(),
            "unread cannot grant support"
        );
        let mut unrelated = comparison.clone();
        unrelated.observations[1].applicability = "not_relevant".into();
        assert!(
            condition_support_options(&input, &unit, &unrelated, &reads)
                .unwrap()
                .is_empty()
        );
        let mut stale = comparison.clone();
        stale.version = "stale".into();
        assert!(condition_support_options(&input, &unit, &stale, &reads).is_err());
        let mut changed = input.clone();
        changed.source_units[0].text.push('!');
        assert!(condition_support_options(&changed, &unit, &comparison, &reads).is_err());
        let mut attached = record.clone();
        attached.condition_support =
            condition_support_options(&input, &unit, &comparison, &reads).unwrap();
        let fresh = build(&input, "r", &attached).unwrap();
        assert!(
            fresh
                .evidence_units
                .iter()
                .any(|u| u.role == "condition_support")
        );
        let mut omitted = comparison.clone();
        omitted.declared_claims[0].original_fragments = vec!["提交材料；".into()];
        assert!(
            validate(&unit, &omitted).is_err(),
            "uncovered original text is rejected"
        );
        let mut independent = comparison.clone();
        independent.declared_claims = vec![
            DeclaredClaim {
                claim_handle: "materials".into(),
                obligation: "提交材料".into(),
                applicability: "project scope".into(),
                original_fragments: vec!["提交材料；".into()],
                primary_handles: vec!["q0".into()],
                support_handles: vec!["q1".into()],
            },
            DeclaredClaim {
                claim_handle: "explanation".into(),
                obligation: "提供附加说明".into(),
                applicability: "optional".into(),
                original_fragments: vec!["可选附加说明。".into()],
                primary_handles: vec!["q0".into()],
                support_handles: vec![],
            },
        ];
        independent.decisions = independent
            .declared_claims
            .iter()
            .map(|c| Decision {
                claim_handle: c.claim_handle.clone(),
                evidence_handles: vec!["q0".into(), "q1".into()],
                verdict: "supports".into(),
                action: "retain".into(),
                resulting_claim: c.obligation.clone(),
            })
            .collect();
        assert_eq!(
            validate(&unit, &independent),
            Ok(true),
            "model-declared claim identities need not match punctuation or host span handles"
        );
        independent.decisions.pop();
        assert!(
            validate(&unit, &independent).is_err(),
            "every declared claim needs a judgment"
        );
        assert_eq!(validate(&unit, &comparison), Ok(true));
        comparison.decisions[0].verdict = "contradicts".into();
        comparison.decisions[0].action = "mark_not_applicable".into();
        assert_eq!(validate(&unit, &comparison), Ok(false));
        comparison.decisions[0].action = "manual_review".into();
        assert_eq!(
            validate(&unit, &comparison),
            Ok(false),
            "contradiction plus manual review stays unresolved without changing verdict"
        );
        comparison.decisions[0].verdict = "uncertain".into();
        comparison.decisions[0].action = "retain".into();
        assert!(validate(&unit, &comparison).is_err());
        comparison.decisions[0].action = "manual_review".into();
        assert_eq!(validate(&unit, &comparison), Ok(false));
        let mut bad = comparison.clone();
        bad.observations.pop();
        assert!(validate(&unit, &bad).is_err());
        let mut bad = comparison.clone();
        bad.decisions[0].evidence_handles = vec!["forged".into()];
        assert!(validate(&unit, &bad).is_err());
        comparison.version = "stale".into();
        assert!(validate(&unit, &comparison).is_err());
    }

    fn supported_comparison(unit: &ReviewUnit) -> Comparison {
        let primary = unit
            .evidence_units
            .iter()
            .find(|quote| quote.role == "requirement_source")
            .unwrap()
            .quote_handle
            .clone();
        let handles = unit
            .evidence_units
            .iter()
            .map(|quote| quote.quote_handle.clone())
            .collect::<Vec<_>>();
        Comparison {
            requirement_id: unit.requirement_id.clone(),
            version: unit.version.clone(),
            declared_claims: vec![DeclaredClaim {
                claim_handle: "claim".into(),
                obligation: unit.original_requirement.description.clone(),
                applicability: "Synthetic declared scope".into(),
                original_fragments: vec![unit.original_requirement.description.clone()],
                primary_handles: vec![primary.clone()],
                support_handles: handles
                    .iter()
                    .filter(|handle| **handle != primary)
                    .cloned()
                    .collect(),
            }],
            observations: handles
                .iter()
                .map(|handle| Observation {
                    quote_handle: handle.clone(),
                    polarity: "required".into(),
                    applicability: "relevant".into(),
                    condition: String::new(),
                })
                .collect(),
            decisions: vec![Decision {
                claim_handle: "claim".into(),
                evidence_handles: handles,
                verdict: "supports".into(),
                action: "retain".into(),
                resulting_claim: unit.original_requirement.description.clone(),
            }],
        }
    }

    #[test]
    fn primary_requires_original_role_and_range_intersection_even_with_relevant_auxiliary() {
        let (input, record) = fixture();
        let unit = build(&input, "r", &record).unwrap();
        let good = supported_comparison(&unit);
        assert_eq!(validate(&unit, &good), Ok(true));
        let mut auxiliary = good.clone();
        auxiliary.declared_claims[0].primary_handles = vec!["q1".into()];
        auxiliary.declared_claims[0].support_handles = vec!["q0".into()];
        assert!(
            validate(&unit, &auxiliary)
                .unwrap_err()
                .contains("primary evidence")
        );
        let mut mislabeled = unit.clone();
        mislabeled.evidence_units[1].role = "requirement_source".into();
        assert!(
            validate(&mislabeled, &auxiliary)
                .unwrap_err()
                .contains("intersect"),
            "role alone cannot authorize another carrier"
        );
        let mut disjoint = unit.clone();
        if let EvidenceRef::Text {
            start_byte,
            end_byte,
            ..
        } = &mut disjoint.evidence_units[0].excerpts[0].evidence
        {
            *start_byte = 6;
            *end_byte = 9;
        } else {
            panic!("text fixture");
        }
        assert!(
            validate(&disjoint, &good)
                .unwrap_err()
                .contains("intersect"),
            "same carrier alone cannot authorize a disjoint range"
        );
        for applicability in ["not_relevant", "unclear"] {
            let mut bad = good.clone();
            bad.observations[0].applicability = applicability.into();
            assert!(
                validate(&unit, &bad)
                    .unwrap_err()
                    .contains("relevant original primary")
            );
            bad.decisions[0].verdict = "uncertain".into();
            bad.decisions[0].action = "manual_review".into();
            assert_eq!(
                validate(&unit, &bad),
                Ok(false),
                "irrelevant or unclear original remains unresolved"
            );
        }
    }

    #[test]
    fn relevant_exception_cannot_disappear_but_negative_obligation_can_be_retained() {
        let (input, record) = fixture();
        let unit = build(&input, "r", &record).unwrap();
        let mut comparison = supported_comparison(&unit);
        comparison.observations[1].polarity = "not_required".into();
        comparison.declared_claims[0].support_handles.clear();
        comparison.decisions[0].evidence_handles = vec!["q0".into()];
        assert!(
            validate(&unit, &comparison)
                .unwrap_err()
                .contains("every explicitly relevant")
        );
        comparison.decisions[0].verdict = "uncertain".into();
        comparison.decisions[0].action = "manual_review".into();
        assert_eq!(validate(&unit, &comparison), Ok(false));
        let mut negative = record;
        negative.description =
            "No additional document is required under the stated exception.".into();
        let negative_unit = build(&input, "negative", &negative).unwrap();
        let mut retained = supported_comparison(&negative_unit);
        for observation in &mut retained.observations {
            observation.polarity = "not_required".into();
        }
        assert_eq!(
            validate(&negative_unit, &retained),
            Ok(true),
            "polarity does not infer contradiction from natural-language text"
        );
    }

    #[test]
    fn native_rowspan_and_layered_headers_are_fresh_auxiliary_check_context() {
        let (mut input, mut record) = fixture();
        input.structured_forms[0]["definition"] = json!({"row_count":4,"column_count":3,"cells":[
            {"row":0,"column":0,"row_span":1,"column_span":3,"text":"Optional service applicability","header_role":"column_header"},
            {"row":1,"column":0,"row_span":3,"column_span":1,"text":"Only for the selected service","header_role":"row_header"},
            {"row":1,"column":1,"row_span":1,"column_span":1,"text":"Response document","header_role":"column_header"},
            {"row":1,"column":2,"row_span":1,"column_span":1,"text":"Delivery notes","header_role":"column_header"},
            {"row":2,"column":1,"row_span":1,"column_span":1,"text":"Unrelated row material","header_role":"none"},
            {"row":2,"column":2,"row_span":1,"column_span":1,"text":"Unrelated row details","header_role":"none"},
            {"row":3,"column":1,"row_span":1,"column_span":1,"text":"Provide signed form","header_role":"none"},
            {"row":3,"column":2,"row_span":1,"column_span":1,"text":"Submit a copy","header_role":"none"}
        ]});
        record.description = "Provide signed form for the selected optional service.".into();
        record.evidence = vec![EvidenceRef::GridCell {
            input_digest: input_digest(&input).unwrap(),
            table_id: "table".into(),
            anchor_row: 3,
            anchor_column: 1,
            start_byte: 0,
            end_byte: 19,
        }];
        let unit = build(&input, "merged", &record).unwrap();
        let refs = unit
            .evidence_units
            .iter()
            .flat_map(|quote| {
                quote
                    .excerpts
                    .iter()
                    .map(|excerpt| excerpt.evidence.clone())
            })
            .collect::<Vec<_>>();
        for (row, column) in [(0, 0), (1, 0), (1, 1), (1, 2), (3, 1), (3, 2)] {
            assert!(refs.iter().any(|reference| matches!(reference, EvidenceRef::GridCell { anchor_row, anchor_column, .. } if (*anchor_row,*anchor_column)==(row,column))), "missing native structure {row},{column}");
        }
        assert!(
            !refs
                .iter()
                .any(|reference| matches!(reference, EvidenceRef::GridCell { anchor_row: 2, .. })),
            "do not copy unrelated body rows"
        );
        assert!(
            unit.evidence_units
                .iter()
                .any(|quote| quote.role == "related_structural_context")
        );
        assert_eq!(
            unit.original_requirement.evidence, record.evidence,
            "Check context never expands primary ownership"
        );
        for quote in &unit.evidence_units {
            let intersects_owned = quote.excerpts.iter().any(|excerpt| {
                record
                    .evidence
                    .iter()
                    .any(|original| intersects_original(&excerpt.evidence, original))
            });
            if !intersects_owned {
                assert_ne!(
                    quote.role, "requirement_source",
                    "a header-only quote cannot inherit primary role from an adjacent owned cell"
                );
            }
        }
        let comparison = supported_comparison(&unit);
        assert_eq!(validate(&unit, &comparison), Ok(true));
        assert!(
            condition_support_options(&input, &unit, &comparison, &record.evidence).is_err(),
            "Discover or original-cell reads do not grant Check header credit"
        );
        assert!(
            !condition_support_options(&input, &unit, &comparison, &refs)
                .unwrap()
                .is_empty()
        );
        let mut auxiliary_primary = comparison;
        let auxiliary = unit
            .evidence_units
            .iter()
            .find(|quote| quote.role == "related_structural_context")
            .unwrap()
            .quote_handle
            .clone();
        auxiliary_primary.declared_claims[0].primary_handles = vec![auxiliary.clone()];
        auxiliary_primary.declared_claims[0]
            .support_handles
            .retain(|handle| *handle != auxiliary);
        assert!(validate(&unit, &auxiliary_primary).is_err());
    }

    #[test]
    fn optional_candidate_cannot_pull_its_cross_document_required_relation() {
        let (mut input, mut record) = fixture();
        input.structured_forms.clear();
        input.source_units.truncate(1);
        input.source_units[0].ordinal = 2;
        input.source_units[0].locator["unit_id"] = json!("a");
        input.source_units[0].locator["section_id"] = json!("synthetic-shared-section");
        let mut candidate = input.source_units[0].clone();
        candidate.source_unit_revision_id = "candidate-c".into();
        candidate.locator["unit_id"] = json!("c");
        candidate.ordinal = 1;
        candidate.text = "Earlier optional context.".into();
        let mut other = candidate.clone();
        other.source_unit_revision_id = "other-d".into();
        other.document_id = "other-document".into();
        other.locator["unit_id"] = json!("d");
        other.text = "Cross-document support of candidate only.".into();
        input.source_units.extend([candidate, other]);
        input.document_relations = vec![serde_json::from_value(json!({"id":"candidate-reference","from":{"document_id":"doc","unit_id":"c"},"to":{"document_id":"other-document","unit_id":"d"},"kind":"explicit_reference","status":"confirmed","required":true,"basis":"synthetic explicit reference","locator":{}})).unwrap()];
        let update = |input: &FrozenInput, record: &mut RequirementRecord| {
            record.evidence = vec![EvidenceRef::Text {
                input_digest: input_digest(input).unwrap(),
                unit_id: "source".into(),
                start_byte: 0,
                end_byte: 5,
            }];
        };
        update(&input, &mut record);
        let unit = build(&input, "r", &record).unwrap();
        let has = |unit: &ReviewUnit, id: &str| {
            unit.evidence_units
                .iter()
                .flat_map(|u| &u.excerpts)
                .any(|e| matches!(&e.evidence,EvidenceRef::Text {unit_id,..} if unit_id == id))
        };
        assert!(
            has(&unit, "candidate-c"),
            "fixture must actually select the semantic candidate"
        );
        assert!(
            !has(&unit, "other-d"),
            "candidate relation is not an original requirement dependency"
        );
        input.document_relations[0].from.unit_id = Some("a".into());
        update(&input, &mut record);
        assert!(
            has(&build(&input, "r", &record).unwrap(), "other-d"),
            "a direct original dependency remains visible"
        );
    }
}
