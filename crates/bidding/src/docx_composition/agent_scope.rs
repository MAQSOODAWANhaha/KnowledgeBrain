//! Business write authority is captured once, before executing a model batch.
//! Evidence coverage proves reading; it never grants ownership of another item.
use super::agent::Checkpoint;
use super::agent_work::Status;
use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(PartialEq, Eq)]
enum Phase {
    Planning,
    Assigned,
    Repair,
    RootRepair,
    GlobalPresentation,
    GlobalReview,
}

pub(super) struct WriteAuthority {
    phase: Phase,
    reviewing: bool,
    sources: BTreeSet<String>,
    coverage: Coverage,
    items: BTreeMap<String, PlanItem>,
    sections: BTreeSet<String>,
    references: BTreeSet<String>,
    relations: BTreeSet<String>,
}

impl WriteAuthority {
    pub(super) fn capture(
        input: &FrozenInput,
        result: &AnalysisResult,
        state: &Checkpoint,
    ) -> Result<Self, String> {
        let workspace = &state.workspace;
        let work = if workspace.reviewing {
            &state.review_work
        } else {
            &state.main_work
        };
        let active = work.as_ref().filter(|w| w.status == Status::Active);
        let mut items = BTreeMap::new();
        let mut sections = BTreeSet::new();
        let phase = if workspace.reviewing {
            if let Some(item) = active
                .and_then(|w| w.plan_item_id.as_ref())
                .and_then(|id| workspace.draft.plan.get(id))
            {
                items.insert(item.id.clone(), item.clone());
                Phase::Assigned
            } else {
                Phase::GlobalReview
            }
        } else if workspace.compile_feedback.as_ref().is_some_and(|feedback| {
            digest(&workspace.draft).ok().as_ref() == Some(&feedback.draft_sha256)
        }) {
            // Compile errors currently have no typed target. Exact-version
            // feedback truthfully authorizes a draft repair, not guessed IDs.
            Phase::RootRepair
        } else if !workspace.findings.is_empty() {
            let findings: BTreeSet<_> = workspace
                .findings
                .iter()
                .map(digest)
                .collect::<Result<_, _>>()?;
            for review in workspace.plan_reviews.values() {
                if review.finding_ids.iter().any(|id| findings.contains(id))
                    && let Some(item) = workspace.draft.plan.get(&review.item_id)
                {
                    items.insert(item.id.clone(), item.clone());
                }
            }
            for finding in &workspace.findings {
                for id in &finding.section_ids {
                    sections.insert(id.clone());
                    if let Some(item) = workspace.draft.plan.get(id) {
                        items.insert(id.clone(), item.clone());
                    }
                }
            }
            Phase::Repair
        } else if !plan_complete(result, &workspace.draft)? {
            Phase::Planning
        } else if let Some(item) = active
            .and_then(|w| w.plan_item_id.as_ref())
            .and_then(|id| workspace.draft.plan.get(id))
        {
            items.insert(item.id.clone(), item.clone());
            Phase::Assigned
        } else {
            Phase::GlobalPresentation
        };
        let mut references = BTreeSet::new();
        for item in items.values() {
            references.extend(item.obligation_refs.iter().cloned());
            if item.kind == PlanItemKind::Section {
                sections.insert(item.id.clone());
            }
        }
        let mut relations = BTreeSet::new();
        // Match typed endpoints, including response/proof indices. A shared
        // record or source page is not a dependency edge.
        for relation in result.analysis.relations.values() {
            if relation.kind != RelationKind::RequiresTemplate {
                continue;
            }
            let from = reference_key(&Reference {
                record_id: relation.from.clone(),
                target: relation.from_target.clone(),
            })?;
            let to = reference_key(&Reference {
                record_id: relation.to.clone(),
                target: relation.to_target.clone(),
            })?;
            if references.contains(&from) || references.contains(&to) {
                relations.insert(relation.id.clone());
            }
        }
        Ok(Self {
            phase,
            reviewing: workspace.reviewing,
            sources: active.map_or_else(
                || {
                    input
                        .source_units
                        .iter()
                        .map(|s| s.source_unit_revision_id.clone())
                        .collect()
                },
                |w| w.source_scope.iter().cloned().collect(),
            ),
            coverage: if workspace.reviewing {
                workspace.review_coverage.clone()
            } else {
                workspace.source_coverage.clone()
            },
            items,
            sections,
            references,
            relations,
        })
    }

    fn reference(&self, reference: &Reference) -> Result<(), String> {
        if matches!(self.phase, Phase::Planning | Phase::RootRepair)
            || self.references.contains(&reference_key(reference)?)
        {
            Ok(())
        } else {
            Err("reference is outside the batch's assigned obligations".into())
        }
    }

    fn grounds(&self, input: &FrozenInput, grounds: &[Span]) -> Result<(), String> {
        for span in grounds {
            if !self.reviewing && !self.sources.contains(&span.source_id) {
                return Err("write evidence is outside the batch's source scope".into());
            }
            crate::tender_analysis::tools::validate_span(input, &self.coverage, span)?;
        }
        Ok(())
    }

    pub(super) fn check(
        &self,
        input: &FrozenInput,
        result: &AnalysisResult,
        state: &Checkpoint,
        name: &str,
        args: &Value,
    ) -> Result<(), String> {
        let mutation = matches!(
            name,
            "put_composition_plan_item"
                | "put_section"
                | "delete_section"
                | "set_presentation"
                | "put_omission"
                | "delete_omission"
                | "omit_template_relation"
                | "delete_relation_omission"
                | "put_composition_review"
        );
        if !mutation {
            return Ok(());
        }
        if self.reviewing != state.workspace.reviewing {
            return Err("role changed after batch entry; write requires a new assignment".into());
        }
        if name == "put_composition_review" {
            if !self.reviewing
                || !args["item_id"]
                    .as_str()
                    .is_some_and(|id| self.items.contains_key(id))
            {
                return Err("review judgment must belong to the batch's assigned plan item".into());
            }
            // Findings may cite independently delivered supporting sources
            // outside this item's local source scope.
            let grounds: Vec<Span> =
                serde_json::from_value(args["grounds"].clone()).map_err(|e| e.to_string())?;
            self.grounds(input, &grounds)?;
            for finding in args["findings"].as_array().into_iter().flatten() {
                let sources: Vec<Span> = serde_json::from_value(finding["sources"].clone())
                    .map_err(|e| e.to_string())?;
                self.grounds(input, &sources)?;
            }
            return Ok(());
        }
        if self.reviewing {
            return Err("independent reviewer cannot mutate composition".into());
        }
        let mut payload = args.clone();
        payload
            .as_object_mut()
            .ok_or("object required")?
            .remove("expected_draft_sha256");
        match name {
            "put_composition_plan_item" => {
                // Null/empty IDs are allocated by the tool during planning.
                if payload["id"].is_null() {
                    payload["id"] = Value::String(String::new());
                }
                let item: PlanItem = serde_json::from_value(payload).map_err(|e| e.to_string())?;
                if !matches!(self.phase, Phase::Planning | Phase::RootRepair) {
                    let original = self
                        .items
                        .get(&item.id)
                        .ok_or("plan write is outside the batch's assigned items")?;
                    if item.kind != original.kind
                        || item
                            .obligation_refs
                            .iter()
                            .any(|r| !original.obligation_refs.contains(r))
                    {
                        return Err("plan update cannot expand captured item authority".into());
                    }
                }
                self.grounds(input, &item.grounds)
            }
            "put_section" | "delete_section" => {
                let id = payload["id"].as_str().ok_or("section id required")?;
                if self.phase != Phase::RootRepair
                    && (!matches!(self.phase, Phase::Assigned | Phase::Repair)
                        || !self.sections.contains(id))
                {
                    return Err("section write is outside the batch's assigned sections".into());
                }
                if name == "delete_section" {
                    return Ok(());
                }
                let section: Section =
                    serde_json::from_value(payload).map_err(|e| e.to_string())?;
                self.grounds(input, &section.grounds)?;
                for content in &section.content {
                    match content {
                        Content::Template {
                            record_id,
                            bindings,
                            ..
                        } => {
                            self.reference(&Reference {
                                record_id: record_id.clone(),
                                target: RelationTarget::Record,
                            })?;
                            for binding in bindings {
                                self.reference(&binding.need)?;
                            }
                        }
                        Content::Placeholder { needs } | Content::ResponseTable { needs, .. } => {
                            for need in needs {
                                self.reference(need)?;
                            }
                        }
                        Content::SourceResponse { need, .. } => self.reference(need)?,
                        Content::BidderBlank | Content::Preserved { .. } => {}
                    }
                }
                Ok(())
            }
            "set_presentation" => {
                if !matches!(
                    self.phase,
                    Phase::Planning | Phase::RootRepair | Phase::GlobalPresentation
                ) && !self
                    .items
                    .values()
                    .any(|i| i.kind == PlanItemKind::Presentation)
                {
                    return Err("presentation write needs presentation ownership".into());
                }
                let presentation: Presentation =
                    serde_json::from_value(payload).map_err(|e| e.to_string())?;
                self.grounds(input, &presentation.grounds)
            }
            "put_omission" | "delete_omission" => {
                let omission = if name == "put_omission" {
                    serde_json::from_value::<Omission>(payload).map_err(|e| e.to_string())?
                } else {
                    state
                        .workspace
                        .draft
                        .omissions
                        .get(payload["id"].as_str().ok_or("omission id required")?)
                        .ok_or("unknown omission")?
                        .clone()
                };
                self.reference(&omission.reference)?;
                self.grounds(input, &omission.grounds)
            }
            "omit_template_relation" | "delete_relation_omission" => {
                let omission = if name == "omit_template_relation" {
                    serde_json::from_value::<RelationOmission>(payload)
                        .map_err(|e| e.to_string())?
                } else {
                    state
                        .workspace
                        .draft
                        .relation_omissions
                        .get(
                            payload["id"]
                                .as_str()
                                .ok_or("relation omission id required")?,
                        )
                        .ok_or("unknown relation omission")?
                        .clone()
                };
                if result
                    .analysis
                    .relations
                    .get(&omission.relation_id)
                    .is_none_or(|r| r.kind != RelationKind::RequiresTemplate)
                    || (!matches!(self.phase, Phase::Planning | Phase::RootRepair)
                        && !self.relations.contains(&omission.relation_id))
                {
                    return Err(
                        "template relation is outside the batch's assigned endpoints".into(),
                    );
                }
                self.grounds(input, &omission.grounds)
            }
            _ => unreachable!(),
        }
    }
}
