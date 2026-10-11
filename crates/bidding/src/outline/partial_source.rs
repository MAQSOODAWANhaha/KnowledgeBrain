//! Derived source-only review after Discover drains; no additional persisted state.
use super::*;
use crate::analysis::{
    FrozenInput,
    agent::{Checkpoint, discover_coordinator::Pending},
};
use std::collections::BTreeSet;

pub(crate) fn eligible(input: &FrozenInput, state: &Checkpoint) -> Option<BTreeSet<String>> {
    let work = state.outline_run.reading_packs.as_ref()?;
    if work.complete() || work.planning_error().is_some() {
        return None;
    }
    if state.outline_run.discover_workers.values().any(|worker| {
        matches!(
            &worker.pending,
            Some(Pending::Prepared { .. } | Pending::Sending { .. } | Pending::Received { .. })
        )
    }) {
        return None;
    }
    let mut failed = BTreeSet::new();
    let mut committed = BTreeSet::new();
    for id in work.pack_ids() {
        if work.status(&id) == Some(discover::PackStatus::Committed) {
            committed.insert(id);
            continue;
        }
        let worker = state.outline_run.discover_workers.get(&id)?;
        let Some(Pending::Failed { code, .. }) = &worker.pending else {
            return None;
        };
        // Business rejections remain retryable until the coordinator marks exhaustion.
        if code == "BUSINESS_REJECTED" {
            return None;
        }
        failed.insert(id);
    }
    if failed.is_empty() {
        return None;
    }
    use crate::analysis::source_manifest::{
        DocumentRelationKind, DocumentRelationStatus, RelationEndpoint,
    };
    let endpoint_matches = |endpoint: &RelationEndpoint, pack: &discover::ParsePack| {
        pack.atoms.iter().any(|atom| {
            endpoint.document_id == atom.document_id
                && endpoint.unit_id.as_ref().is_none_or(|key| {
                    input.source_units.iter().any(|source| {
                        source.source_unit_revision_id == atom.source_unit_revision_id
                            && source.locator["unit_id"] == *key
                    })
                })
        })
    };
    let mut blocked = failed;
    loop {
        let mut added = BTreeSet::new();
        for id in &committed {
            if blocked.contains(id) {
                continue;
            }
            let pack = work.canonical_pack(id)?;
            let relation_dep = input.document_relations.iter().any(|relation| {
                relation.kind == DocumentRelationKind::ExplicitReference
                    && relation.status == DocumentRelationStatus::Confirmed
                    && relation.to.as_ref().is_some_and(|to| {
                        blocked.iter().any(|other| {
                            work.canonical_pack(other).is_some_and(|other| {
                                (endpoint_matches(&relation.from, pack)
                                    && endpoint_matches(to, other))
                                    || (endpoint_matches(to, pack)
                                        && endpoint_matches(&relation.from, other))
                            })
                        })
                    })
            });
            let support_dep = work
                .requirement_records()
                .iter()
                .filter(|(key, _)| key.starts_with(&format!("{id}:")))
                .flat_map(|(_, record)| &record.condition_support)
                .any(|support| match &support.origin {
                    discover::ConditionSupportOrigin::ReviewedRequirement { requirement_id } => {
                        !work.requirement_records().contains_key(requirement_id)
                            || blocked
                                .iter()
                                .any(|id| requirement_id.starts_with(&format!("{id}:")))
                    }
                    discover::ConditionSupportOrigin::ConfirmedRelation { relation_id } => {
                        !input.document_relations.iter().any(|r| {
                            r.id == *relation_id
                                && r.kind == DocumentRelationKind::ExplicitReference
                                && r.status == DocumentRelationStatus::Confirmed
                        })
                    }
                });
            if relation_dep || support_dep {
                added.insert(id.clone());
            }
        }
        if added.is_empty() {
            break;
        }
        blocked.extend(added);
    }
    committed.retain(|id| !blocked.contains(id));
    Some(committed)
}

/// Shared entry policy for direct domain calls and every asynchronous tool route.
/// A continuation may omit mode; its persisted identity and page are validated
/// by the existing reader before delivery. This check never grants read credit.
pub(crate) fn validate_tool(
    input: &FrozenInput,
    state: &Checkpoint,
    name: &str,
    args: &serde_json::Value,
) -> Result<(), String> {
    let Some(ids) = eligible(input, state) else {
        return Ok(());
    };
    if !matches!(
        name,
        "read_requirements" | "read_evidence" | "read_source_view" | "submit_review"
    ) {
        return Err(
            "partial source review permits only source reading and source dispositions".into(),
        );
    }
    if name == "read_requirements" && args["mode"] != "packs" && !args["cursor"].is_string() {
        return Err("partial source review requires packs mode".into());
    }
    if name == "submit_review" {
        if ["requirement_ids", "template_reviews"]
            .iter()
            .any(|key| args[key].as_array().is_none_or(|rows| !rows.is_empty()))
        {
            return Err("partial review cannot grant requirement or template completion".into());
        }
        if args["pack_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(
                args["source_dispositions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|row| &row["pack_id"]),
            )
            .any(|id| id.as_str().is_none_or(|id| !ids.contains(id)))
        {
            return Err("partial review scope is not independent and committed".into());
        }
        if args["issues"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|row| row["requirement_ids"].as_array().into_iter().flatten())
            .any(|id| {
                id.as_str()
                    .is_none_or(|id| !ids.iter().any(|pack| id.starts_with(&format!("{pack}:"))))
            })
        {
            return Err("partial review issue names a requirement outside eligible scope".into());
        }
    }
    Ok(())
}

pub(crate) fn permits(
    input: &FrozenInput,
    state: &Checkpoint,
    reference: &evidence::EvidenceRef,
) -> bool {
    eligible(input, state).is_none_or(|ids| {
        ids.iter().any(|id| {
            state
                .outline_run
                .tool_draft
                .source_scopes
                .get(id)
                .is_some_and(|scope| evidence::covered_by_union(reference, &scope.evidence))
        })
    })
}

pub(crate) fn scope_observed(input: &FrozenInput, state: &Checkpoint, id: &str) -> bool {
    let draft = &state.outline_run.tool_draft;
    let Some(scope) = draft.source_scopes.get(id) else {
        return false;
    };
    let rows: Vec<_> = draft
        .source_dispositions
        .values()
        .filter(|row| row.pack_id == id)
        .collect();
    if rows.is_empty()
        || rows
            .iter()
            .any(|row| source_review::validate(input, draft, scope, row).is_err())
    {
        return false;
    }
    let refs: Vec<_> = rows
        .iter()
        .flat_map(|row| row.evidence.iter().cloned())
        .collect();
    scope
        .structure_keys
        .is_subset(&draft.check_reads.structure_keys)
        && scope
            .evidence
            .iter()
            .all(|reference| evidence::covered_by_union(reference, &refs))
}
pub(crate) fn exhausted(input: &FrozenInput, state: &Checkpoint) -> bool {
    eligible(input, state).is_some_and(|ids| ids.iter().all(|id| scope_observed(input, state, id)))
}

pub(crate) fn repair_stopped(state: &Checkpoint) -> bool {
    state
        .outline_run
        .repair_events
        .last()
        .is_some_and(|event| event["source_only_review"] == true && event["status"] == "reopened")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::analysis::agent::discover_coordinator::{Request, Scope, Worker};
    use serde_json::json;

    pub(crate) fn fixture() -> (FrozenInput, Checkpoint, String, Request) {
        let (input, mut state, good, _) = crate::outline::fixture_discovered(true);
        let mut wire =
            serde_json::to_value(state.outline_run.reading_packs.as_ref().unwrap()).unwrap();
        let mut failed = wire["packs"][&good].clone();
        failed["pack"]["id"] = json!("failed-pack");
        failed["pack"]["order"] = json!(1);
        failed["status"] = json!("failed");
        wire["packs"]["failed-pack"] = failed;
        state.outline_run.reading_packs = Some(serde_json::from_value(wire).unwrap());
        let request = Request {
            scope: Scope {
                run: "synthetic".into(),
                pack: "failed-pack".into(),
                revision: 0,
                generation: 0,
                request: "synthetic".into(),
            },
            body: vec![],
            input_reserve: 0,
            output_reserve: 0,
        };
        state.outline_run.discover_workers.insert(
            "failed-pack".into(),
            Worker {
                pending: Some(Pending::Failed {
                    request: request.clone(),
                    code: "DISCOVER_RETRY_EXHAUSTED".into(),
                }),
                ..Default::default()
            },
        );
        (input, state, good, request)
    }

    #[test]
    fn partial_source_empty_commit_requires_fresh_review_and_keeps_global_failure_after_reload() {
        let (input, mut state, good, _) = fixture();
        assert_eq!(
            eligible(&input, &state),
            Some(BTreeSet::from([good.clone()]))
        );
        super::super::agent::synchronize_discovery(&input, &mut state).unwrap();
        assert_eq!(
            super::super::agent::current(&input, &state),
            super::super::agent::Duty::Check
        );
        assert_eq!(state.outline_run.tool_draft.required_pack_ids.len(), 2);
        assert!(
            !super::super::source_review::complete(&input, &state.outline_run.tool_draft, &good)
                .unwrap()
        );
        assert!(!state.outline_run.reading_packs.as_ref().unwrap().complete());
        assert!(
            super::super::tools::template_ready(&input, &state.outline_run.tool_draft).is_err()
        );
        state.transcript.clear();
        let reloaded: Checkpoint = serde_json::from_value(json!(state)).unwrap();
        assert_eq!(eligible(&input, &reloaded), Some(BTreeSet::from([good])));
    }

    #[test]
    fn partial_source_never_treats_unknown_or_uncommitted_response_as_drained() {
        let (input, mut state, _, request) = fixture();
        for pending in [
            Pending::Prepared {
                request: request.clone(),
            },
            Pending::Sending {
                request: request.clone(),
            },
            Pending::Received {
                request: request.clone(),
                response: json!({}),
            },
            Pending::Failed {
                request: request.clone(),
                code: "BUSINESS_REJECTED".into(),
            },
        ] {
            state
                .outline_run
                .discover_workers
                .get_mut("failed-pack")
                .unwrap()
                .pending = Some(pending);
            assert!(eligible(&input, &state).is_none());
        }
    }
    #[test]
    fn partial_source_omission_requires_delivered_evidence_then_reopens_unique_requirements() {
        let (input, mut state, good, _) = fixture();
        super::super::agent::synchronize_discovery(&input, &mut state).unwrap();
        let scope = state.outline_run.tool_draft.source_scopes[&good].clone();
        assert!(!scope.evidence.is_empty());
        let review = super::super::source_review::Disposition {
            pack_id: good.clone(),
            version: super::super::source_review::version(
                &input,
                &state.outline_run.tool_draft,
                &good,
                &scope,
            )
            .unwrap(),
            evidence: scope.evidence.clone(),
            verdict: super::super::source_review::Verdict::Unresolved,
            requirement_ids: BTreeSet::new(),
            reason: "Source contains an obligation absent from discovery; re-extract".into(),
        };
        assert!(
            super::super::source_review::validate(
                &input,
                &state.outline_run.tool_draft,
                &scope,
                &review
            )
            .is_err()
        );
        // Explicit host-receipt seam: this domain test does not simulate transport.
        state.outline_run.tool_draft.check_reads.evidence = scope.evidence.clone();
        state.outline_run.tool_draft.check_reads.structure_keys = scope.structure_keys.clone();
        super::super::source_review::validate(
            &input,
            &state.outline_run.tool_draft,
            &scope,
            &review,
        )
        .unwrap();
        let key = super::super::source_review::key(&review).unwrap();
        state
            .outline_run
            .tool_draft
            .source_dispositions
            .insert(key, review);
        assert!(super::super::agent::finish_check_repair_batch(&input, &mut state).unwrap());
        assert!(!state.outline_run.reading_packs.as_ref().unwrap().complete());
        assert!(!state.done);
        assert_eq!(
            super::super::agent::current(&input, &state),
            super::super::agent::Duty::Discover
        );
        assert!(repair_stopped(&state));
        assert!(
            !state.outline_run.repair_events.last().unwrap()["source_only_findings"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn partial_source_matches_confirmed_native_endpoints_not_whole_document() {
        use crate::analysis::source_manifest::{
            DocumentRelation, DocumentRelationKind, DocumentRelationStatus, RelationEndpoint,
        };
        let (mut input, mut state, good, _) = fixture();
        let source_id = state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .canonical_pack(&good)
            .unwrap()
            .atoms[0]
            .source_unit_revision_id
            .clone();
        let source = input
            .source_units
            .iter_mut()
            .find(|s| s.source_unit_revision_id == source_id)
            .unwrap();
        source.locator["unit_id"] = json!("independent");
        let doc = source.document_id.clone();
        let mut failed_source = source.clone();
        failed_source.source_unit_revision_id = "failure-native-unit".into();
        failed_source.locator["unit_id"] = json!("failed");
        input.source_units.push(failed_source);
        let mut wire =
            serde_json::to_value(state.outline_run.reading_packs.as_ref().unwrap()).unwrap();
        for atom in wire["packs"]["failed-pack"]["pack"]["atoms"]
            .as_array_mut()
            .unwrap()
        {
            atom["source_unit_revision_id"] = json!("failure-native-unit");
        }
        state.outline_run.reading_packs = Some(serde_json::from_value(wire).unwrap());
        input.document_relations.push(DocumentRelation {
            id: "relation".into(),
            from: RelationEndpoint {
                document_id: doc.clone(),
                unit_id: Some("other-unit".into()),
            },
            to: Some(RelationEndpoint {
                document_id: doc,
                unit_id: Some("failed".into()),
            }),
            kind: DocumentRelationKind::ExplicitReference,
            status: DocumentRelationStatus::Confirmed,
            required: true,
            basis: "synthetic".into(),
            locator: json!({}),
        });
        assert!(eligible(&input, &state).unwrap().contains(&good));
        input.document_relations[0].from.unit_id = Some("independent".into());
        assert!(!eligible(&input, &state).unwrap().contains(&good));
        input.document_relations[0].status = DocumentRelationStatus::Unconfirmed;
        assert!(eligible(&input, &state).unwrap().contains(&good));
    }

    #[test]
    fn partial_source_stops_after_independent_coverage_without_global_pass() {
        let (input, mut state, good, _) = fixture();
        super::super::agent::synchronize_discovery(&input, &mut state).unwrap();
        let scope = state.outline_run.tool_draft.source_scopes[&good].clone();
        state.outline_run.tool_draft.check_reads.evidence = scope.evidence.clone();
        state.outline_run.tool_draft.check_reads.structure_keys = scope.structure_keys.clone();
        let row = source_review::Disposition {
            pack_id: good.clone(),
            version: source_review::version(&input, &state.outline_run.tool_draft, &good, &scope)
                .unwrap(),
            evidence: scope.evidence.clone(),
            verdict: source_review::Verdict::NoResponseObligation,
            requirement_ids: BTreeSet::new(),
            reason: "Synthetic no-response source".into(),
        };
        state
            .outline_run
            .tool_draft
            .source_dispositions
            .insert(source_review::key(&row).unwrap(), row);
        assert!(exhausted(&input, &state));
        let before = state.journal.accounting.clone();
        let reloaded: Checkpoint = serde_json::from_value(json!(state)).unwrap();
        assert!(exhausted(&input, &reloaded));
        assert!(!reloaded.done);
        assert!(tools::template_ready(&input, &reloaded.outline_run.tool_draft).is_err());
        assert_eq!(
            before.physical_calls,
            reloaded.journal.accounting.physical_calls
        );
        assert_eq!(
            before.reserved_input_tokens,
            reloaded.journal.accounting.reserved_input_tokens
        );
    }

    #[test]
    fn partial_source_cannot_skip_committed_workers_unresolved_transport() {
        let (input, mut state, good, request) = fixture();
        for pending in [
            Pending::Prepared {
                request: request.clone(),
            },
            Pending::Sending {
                request: request.clone(),
            },
            Pending::Received {
                request: request.clone(),
                response: json!({}),
            },
        ] {
            state.outline_run.discover_workers.insert(
                good.clone(),
                Worker {
                    pending: Some(pending),
                    ..Default::default()
                },
            );
            assert!(eligible(&input, &state).is_none());
        }
    }

    #[test]
    fn partial_source_document_issue_cannot_hide_unobserved_structure() {
        let (input, mut state, good, _) = fixture();
        super::super::agent::synchronize_discovery(&input, &mut state).unwrap();
        let scope = state.outline_run.tool_draft.source_scopes[&good].clone();
        assert!(!scope.structure_keys.is_empty());
        state.outline_run.tool_draft.check_reads.evidence = scope.evidence.clone();
        let row = source_review::Disposition {
            pack_id: good.clone(),
            version: source_review::version(&input, &state.outline_run.tool_draft, &good, &scope)
                .unwrap(),
            evidence: scope.evidence.clone(),
            verdict: source_review::Verdict::Unresolved,
            requirement_ids: BTreeSet::new(),
            reason: "Synthetic missing referenced document".into(),
        };
        state
            .outline_run
            .tool_draft
            .source_dispositions
            .insert(source_review::key(&row).unwrap(), row);
        state
            .outline_run
            .tool_draft
            .review_issues
            .push(ReviewIssue {
                id: "synthetic-missing".into(),
                code: "document_missing".into(),
                description: "Synthetic external attachment absent".into(),
                requirement_ids: vec![],
                evidence: scope.evidence.clone(),
                requested_information: "Supply the referenced attachment".into(),
            });
        assert!(
            !source_review::execution_complete(&input, &state.outline_run.tool_draft, &good)
                .unwrap()
        );
        assert!(!scope_observed(&input, &state, &good));
        let host = super::super::agent::host_packet(&input, &state, 32, json!({}), json!({}), None);
        assert_eq!(host["check_work"]["source_scope"]["pack_id"], good);
        assert!(!exhausted(&input, &state));
    }

    #[test]
    fn partial_source_async_projection_rejects_template_and_requirement_reads() {
        let (input, mut state, _, _) = fixture();
        super::super::agent::synchronize_discovery(&input, &mut state).unwrap();
        let before = serde_json::to_value(&state).unwrap();
        for (name, args) in [
            ("read_outline", json!({"mode":"summary"})),
            (
                "read_outline",
                json!({"mode":"slot_body","slot_id":"synthetic"}),
            ),
            (
                "read_outline",
                json!({"mode":"manual_task","requirement_id":"synthetic","task_id":"synthetic"}),
            ),
            ("read_requirements", json!({})),
        ] {
            let error =
                super::super::agent::full_read_projection(&input, &state, name, &args).unwrap_err();
            assert!(error.contains("partial source review"));
        }
        assert!(
            super::super::agent::full_read_projection(
                &input,
                &state,
                "read_requirements",
                &json!({"mode":"packs"})
            )
            .is_ok()
        );
        assert_eq!(before, serde_json::to_value(&state).unwrap());
    }
    #[test]
    fn partial_source_entry_policy_covers_registry_and_scoped_submissions() {
        let (input, mut state, good, _) = fixture();
        super::super::agent::synchronize_discovery(&input, &mut state).unwrap();
        let args = json!({"mode":"packs","requirement_ids":[],"template_reviews":[],"pack_ids":[good],"source_dispositions":[],"issues":[]});
        let before = json!(state);
        for spec in super::super::agent::registry() {
            assert_eq!(
                validate_tool(&input, &state, spec.name, &args).is_ok(),
                matches!(
                    spec.name,
                    "read_requirements" | "read_evidence" | "read_source_view" | "submit_review"
                ),
                "{}",
                spec.name
            );
        }
        assert!(
            validate_tool(
                &input,
                &state,
                "read_requirements",
                &json!({"cursor":"host-cursor"})
            )
            .is_ok()
        );
        assert!(
            validate_tool(
                &input,
                &state,
                "read_outline",
                &json!({"cursor":"host-cursor"})
            )
            .is_err()
        );
        for (field, value) in [
            ("pack_ids", json!(["failed-pack"])),
            ("requirement_ids", json!(["synthetic"])),
            ("template_reviews", json!([{}])),
            (
                "issues",
                json!([{"requirement_ids":["failed-pack:requirement"]}]),
            ),
        ] {
            let mut bad = args.clone();
            bad[field] = value;
            assert!(validate_tool(&input, &state, "submit_review", &bad).is_err());
        }
        assert_eq!(before, json!(state));
    }
}
