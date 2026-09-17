//! Composition-local work and progress. No second queue or model summaries.
use super::agent::{Checkpoint, Limits};
use super::*;
use crate::agent_runtime::progress::{Progress, Recovery};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Locate,
    Compose,
    Review,
    Render,
    Handoff,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Complete,
    Blocked,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub source_scope: Vec<String>,
    pub section_scope: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_item_id: Option<String>,
    pub action: Action,
    pub objective: String,
    pub note: String,
    pub status: Status,
}

fn saved_implementation(state: &Checkpoint, item: &PlanItem) -> bool {
    let draft = &state.workspace.draft;
    match item.kind {
        PlanItemKind::Section => draft.sections.get(&item.id).is_some_and(|section| {
            section.title == item.title
                && section.parent == item.parent
                && section.order == item.order
                && section.placement == item.placement
                && (!section.content.is_empty()
                    || draft.plan.values().any(|child| {
                        child.kind == PlanItemKind::Section
                            && child.parent.as_ref() == Some(&item.id)
                    }))
        }),
        PlanItemKind::Presentation => draft
            .presentation
            .as_ref()
            .is_some_and(|p| p.title == item.title),
        PlanItemKind::ReportNote => item.exception.as_ref().is_some_and(|reason| {
            !item.obligation_refs.is_empty()
                && item.obligation_refs.iter().all(|key| {
                    draft.omissions.values().any(|omission| {
                        reference_key(&omission.reference).ok().as_ref() == Some(key)
                            && &omission.reason == reason
                    })
                })
        }),
    }
}

fn current_review(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &Checkpoint,
    item: &PlanItem,
) -> bool {
    state
        .workspace
        .plan_reviews
        .get(&item.id)
        .is_some_and(|review| {
            state
                .workspace
                .validate_plan_review(input, result, item, review)
                .is_ok()
        })
}

pub(super) fn host_assigned_item<'a>(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &'a Checkpoint,
) -> Option<&'a PlanItem> {
    // Planning covers the complete obligation inventory before individual
    // implementations can be dispatched. Invalid plans are reported by the
    // request/tool validators; they cannot confer a local assignment either.
    if !state.workspace.reviewing
        && !matches!(plan_complete(result, &state.workspace.draft), Ok(true))
    {
        return None;
    }
    let work = if state.workspace.reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    // Keep one saved assignment for the entire batch. Only batch-end completion
    // or the existing explicit completion path releases it.
    if let Some(item) = work
        .as_ref()
        .filter(|w| w.status == Status::Active)
        .and_then(|w| w.plan_item_id.as_ref())
        .and_then(|id| state.workspace.draft.plan.get(id))
    {
        return Some(item);
    }
    let mut items: Vec<_> = state.workspace.draft.plan.values().collect();
    items.sort_by(|a, b| (a.order, &a.id).cmp(&(b.order, &b.id)));
    items.into_iter().find(|item| {
        if state.workspace.reviewing {
            !current_review(input, result, state, item)
        } else {
            item.parent.as_ref().is_none_or(|parent| {
                state
                    .workspace
                    .draft
                    .plan
                    .get(parent)
                    .is_some_and(|item| saved_implementation(state, item))
            }) && !saved_implementation(state, item)
        }
    })
}

pub(super) fn scope_sections<'a>(state: &'a Checkpoint, scope: &[String]) -> Vec<&'a Section> {
    state
        .workspace
        .draft
        .sections
        .values()
        .filter(|s| s.grounds.iter().any(|g| scope.contains(&g.source_id)))
        .collect()
}
fn dependencies(state: &Checkpoint, scope: &[String]) -> Result<String, String> {
    let plan: Vec<_> = state
        .workspace
        .draft
        .plan
        .values()
        .filter(|item| item.grounds.iter().any(|g| scope.contains(&g.source_id)))
        .collect();
    digest(&json!([
        scope_sections(state, scope),
        plan,
        state.workspace.draft.presentation,
        state.workspace.draft.omissions,
        state.workspace.plan_reviews
    ]))
}

/// Local saved work is distinct from global compiled obligation coverage.
fn validate_completion(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &Checkpoint,
    work: &Work,
) -> Result<(), String> {
    let coverage = if state.workspace.reviewing {
        &state.workspace.review_coverage
    } else {
        &state.workspace.source_coverage
    };
    let unread = crate::tender_analysis::tools::reading_gaps(input, coverage)
        .iter()
        .any(|gap| {
            let source = gap["source_id"].as_str().or_else(|| {
                input
                    .structured_forms
                    .iter()
                    .find(|f| f["form_definition_revision_id"] == gap["form_id"])
                    .and_then(|f| f["source_unit_revision_id"].as_str())
            });
            source.is_some_and(|id| work.source_scope.iter().any(|s| s == id))
        });
    let sections = scope_sections(state, &work.source_scope);
    let item = work
        .plan_item_id
        .as_ref()
        .and_then(|id| state.workspace.draft.plan.get(id));
    if unread
        || item.map_or(sections.is_empty(), |item| {
            !saved_implementation(state, item)
        })
    {
        return Err("complete local work needs delivered original evidence and a saved source-backed implementation".into());
    }
    if state.workspace.reviewing
        && item.is_some_and(|item| !current_review(input, result, state, item))
    {
        return Err(
            "complete review work requires a current independent plan item judgment".into(),
        );
    }
    if state.workspace.reviewing
        && state
            .workspace
            .review_gaps(input, result)?
            .iter()
            .any(|gap| {
                gap["key"]
                    .as_str()
                    .is_some_and(|key| sections.iter().any(|s| key == format!("section:{}", s.id)))
            })
    {
        return Err("independently inspect the current local sections before completion".into());
    }
    Ok(())
}

pub(super) fn set_work(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    let mut work: Work = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    if work.source_scope.is_empty()
        || work.objective.trim().is_empty()
        || work.status == Status::Blocked
        || serde_json::to_vec(&work).map_err(|e| e.to_string())?.len() > max_bytes
    {
        return Err(
            "bounded source scope/objective required; execution blocking is host-maintained".into(),
        );
    }
    for id in &work.source_scope {
        if !input
            .source_units
            .iter()
            .any(|s| &s.source_unit_revision_id == id)
        {
            return Err("unknown composition source scope".into());
        }
    }
    if !state.workspace.reviewing
        && !plan_complete(result, &state.workspace.draft)?
        && (work.plan_item_id.is_some() || !work.section_scope.is_empty())
    {
        return Err("finish the obligation plan before assigning a section; planning work has no plan_item_id or section_scope".into());
    }
    if let Some(assigned) = host_assigned_item(input, result, state) {
        if work
            .plan_item_id
            .as_ref()
            .is_some_and(|id| id != &assigned.id)
            || work
                .section_scope
                .iter()
                .any(|id| assigned.kind != PlanItemKind::Section || id != &assigned.id)
        {
            return Err("host assigns the current plan item; set_composition_work cannot pick the next item".into());
        }
        if !assigned
            .grounds
            .iter()
            .any(|g| work.source_scope.contains(&g.source_id))
        {
            return Err("assigned plan item lies outside source scope".into());
        }
        work.plan_item_id = Some(assigned.id.clone());
    } else if let Some(id) = &work.plan_item_id {
        let item = state
            .workspace
            .draft
            .plan
            .get(id)
            .ok_or("unknown focused plan item")?;
        if !item
            .grounds
            .iter()
            .any(|g| work.source_scope.contains(&g.source_id))
        {
            return Err("focused plan item lies outside source scope".into());
        }
    }
    for id in &work.section_scope {
        let item = state
            .workspace
            .draft
            .plan
            .get(id)
            .ok_or("unknown focused section plan")?;
        if item.kind != PlanItemKind::Section {
            return Err("section_scope accepts only section plan IDs".into());
        }
        if !item
            .grounds
            .iter()
            .any(|g| work.source_scope.contains(&g.source_id))
        {
            return Err("focused section lies outside source scope".into());
        }
    }
    let progress = state.execution();
    let mut retry = None;
    for blocker in &progress.blockers {
        if blocker
            .scope
            .iter()
            .any(|id| work.source_scope.contains(id))
        {
            if dependencies(state, &blocker.scope)? == blocker.dependencies_sha256 {
                return Err(
                    "blocked composition dependencies unchanged; select an independent scope"
                        .into(),
                );
            }
            retry = Some(blocker.watch.clone());
        }
    }
    if work.status == Status::Complete {
        validate_completion(input, result, state, &work)?;
    }
    let resume = progress.watch.recovery == Recovery::Blocked;
    let slot = if state.workspace.reviewing {
        &mut state.review_work
    } else {
        &mut state.main_work
    };
    *slot = Some(work);
    if resume {
        state.execution_mut().resume(retry.as_ref());
    }
    Ok(json!({"saved":true}))
}

/// Finish ordinary assigned work against the final batch state. Repair and
/// blocked dispatch keep their existing explicit protocol until their durable
/// owners are integrated; a saved old section is not a repaired finding.
pub(super) fn complete_assigned(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut Checkpoint,
) -> bool {
    if state.pending_delivery.is_some()
        || state.workspace.done
        || !state.main_progress.blockers.is_empty()
        || !state.review_progress.blockers.is_empty()
        || (!state.workspace.reviewing
            && (!state.workspace.findings.is_empty() || state.workspace.compile_feedback.is_some()))
    {
        return false;
    }
    let work = if state.workspace.reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    let Some(work) = work.as_ref().filter(|work| {
        work.plan_item_id
            .as_ref()
            .is_some_and(|id| state.workspace.draft.plan.contains_key(id))
            && work.status == Status::Active
    }) else {
        return false;
    };
    if validate_completion(input, result, state, work).is_err() {
        return false;
    }
    let slot = if state.workspace.reviewing {
        &mut state.review_work
    } else {
        &mut state.main_work
    };
    slot.as_mut().expect("validated assignment").status = Status::Complete;
    true
}

/// Install the next scope only after the completed batch's progress has been
/// attributed to its old work. Installing a work note grants no reading receipt.
pub(super) fn install_next(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut Checkpoint,
    max_bytes: usize,
) -> Result<(), String> {
    if state.workspace.done
        || state.pending_delivery.is_some()
        || !state.main_progress.blockers.is_empty()
        || !state.review_progress.blockers.is_empty()
        || (!state.workspace.reviewing
            && (!state.workspace.findings.is_empty() || state.workspace.compile_feedback.is_some()))
    {
        return Ok(());
    }
    if !state.workspace.reviewing && !plan_complete(result, &state.workspace.draft)? {
        // A changed plan returns to planning at the batch boundary. It must not
        // leave an old section assignment authorizing an incomplete new plan.
        if let Some(work) = &mut state.main_work {
            work.plan_item_id = None;
            work.section_scope.clear();
            work.action = Action::Locate;
            work.status = Status::Active;
        }
        return Ok(());
    }
    let Some(item) = host_assigned_item(input, result, state) else {
        let work = if state.workspace.reviewing {
            &mut state.review_work
        } else {
            &mut state.main_work
        };
        if work
            .as_ref()
            .is_some_and(|work| work.status == Status::Complete)
        {
            *work = None;
        }
        return Ok(());
    };
    let current = if state.workspace.reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    if current.as_ref().is_some_and(|work| {
        work.status == Status::Active && work.plan_item_id.as_ref() == Some(&item.id)
    }) {
        return Ok(());
    }
    let work = Work {
        source_scope: item
            .grounds
            .iter()
            .map(|span| span.source_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        section_scope: if item.kind == PlanItemKind::Section {
            vec![item.id.clone()]
        } else {
            Vec::new()
        },
        plan_item_id: Some(item.id.clone()),
        action: if state.workspace.reviewing {
            Action::Review
        } else {
            Action::Compose
        },
        objective: "Complete the host-assigned plan item using its original evidence.".into(),
        note: String::new(),
        status: Status::Active,
    };
    set_work(input, result, state, &json!(work), max_bytes)?;
    Ok(())
}

/// Read versions come from the workspace at response entry, before new tools:
/// delivery of this batch cannot count as progress until the following response.
pub(super) fn observe(
    state: &mut Checkpoint,
    reviewing: bool,
    delivered: Vec<String>,
    local_completion: Option<String>,
    limits: &Limits,
) -> Result<(), String> {
    let work = if reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    let mut scope = work
        .as_ref()
        .map(|w| w.source_scope.clone())
        .unwrap_or_default();
    scope.sort();
    let deps = dependencies(state, &scope)?;
    let completed = work.as_ref().is_some_and(|w| w.status == Status::Complete)
        || state.workspace.reviewing != reviewing
        || state.workspace.done;
    let mut versions = delivered;
    let mut local_completion_current = false;
    for section in scope_sections(state, &scope) {
        let version = digest(section)?;
        local_completion_current |= local_completion.as_ref() == Some(&version);
        versions.push(version);
    }
    for item in state.workspace.draft.plan.values() {
        versions.push(digest(item)?);
    }
    if reviewing {
        for review in state.workspace.plan_reviews.values() {
            versions.push(digest(review)?);
        }
    }
    for value in state.workspace.draft.omissions.values() {
        versions.push(digest(value)?);
    }
    for value in state.workspace.draft.relation_omissions.values() {
        versions.push(digest(value)?);
    }
    versions.push(digest(&state.workspace.draft.presentation)?);
    for finding in &state.workspace.findings {
        versions.push(digest(finding)?);
    }
    if let Some(artifact) = &state.workspace.artifact {
        versions.push(artifact.manifest.docx_sha256.clone());
    }
    let completion = completed
        .then(|| {
            digest(&json!([
                scope,
                deps,
                state.workspace.reviewing,
                state.workspace.done
            ]))
        })
        .transpose()?;
    let progress = if reviewing {
        &mut state.review_progress
    } else {
        &mut state.main_progress
    };
    progress.observe(
        versions,
        completion.or(local_completion.filter(|_| local_completion_current)),
        &limits.progress(),
    );
    if completed {
        progress
            .blockers
            .retain(|b| !b.scope.iter().all(|id| scope.contains(id)));
    }
    progress.block(scope, deps);
    if progress.watch.recovery == Recovery::Blocked {
        let work = if reviewing {
            &mut state.review_work
        } else {
            &mut state.main_work
        };
        if let Some(work) = work {
            work.status = Status::Blocked;
        }
    }
    Ok(())
}

pub(super) fn delivered_versions(state: &Checkpoint) -> Result<Vec<String>, String> {
    let coverage = if state.workspace.reviewing {
        &state.workspace.review_coverage
    } else {
        &state.workspace.source_coverage
    };
    let mut versions = vec![];
    for (kind, value) in json!(coverage)
        .as_object()
        .ok_or("coverage object missing")?
    {
        if kind == "view_failures" {
            continue;
        }
        if let Some(entries) = value.as_object() {
            for (id, value) in entries {
                versions.push(digest(&json!([kind, id, value]))?);
            }
        }
    }
    for (id, version) in &state.workspace.inspected {
        versions.push(digest(&json!([id, version]))?);
    }
    Ok(versions)
}

pub(super) fn packet(input: &FrozenInput, result: &AnalysisResult, state: &Checkpoint) -> Value {
    let progress = state.execution();
    let work = if state.workspace.reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    let assigned =
        host_assigned_item(input, result, state).map(|item| json!({"id":item.id,"kind":item.kind}));
    json!({"work":work,"assigned_plan_item":assigned,"watch":progress.watch,"blocker_count":progress.blockers.len(),
        "instruction":"Use assigned_plan_item for plan_item_id; section_scope contains only section IDs and is empty for presentation/report work. Complete the current item before handoff. On replan, narrow to its source-backed implementation or inspect a specific missing field. On blocked, select independent sources. Execution blockers prevent acceptance and are not source uncertainty."})
}

impl Checkpoint {
    pub(super) fn execution(&self) -> &Progress {
        if self.workspace.reviewing {
            &self.review_progress
        } else {
            &self.main_progress
        }
    }
    fn execution_mut(&mut self) -> &mut Progress {
        if self.workspace.reviewing {
            &mut self.review_progress
        } else {
            &mut self.main_progress
        }
    }
}

pub(super) fn focused_completion(
    state: &Checkpoint,
    name: &str,
    output: &Value,
) -> Result<Option<String>, String> {
    let work = if state.workspace.reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    let Some(work) = work.as_ref().filter(|w| w.status == Status::Active) else {
        return Ok(None);
    };
    if name == "put_section"
        && work.action == Action::Compose
        && let Some(id) = output["id"].as_str()
        && work
            .plan_item_id
            .as_deref()
            .is_none_or(|assigned| assigned == id)
        && let Some(section) = state.workspace.draft.sections.get(id)
        && section
            .grounds
            .iter()
            .any(|s| work.source_scope.contains(&s.source_id))
    {
        return digest(section).map(Some);
    }
    Ok(None)
}

pub(super) fn execution_gaps(
    state: &Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    if args.as_object().is_none_or(|o| o.len() != 2) {
        return Err("only offset and limit are accepted".into());
    }
    let number = |key: &str| {
        args[key]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("{key} must be a nonnegative integer"))
    };
    let rows:Vec<_>=state.execution().blockers.iter().map(|b|json!({"source_scope":b.scope,"dependencies_sha256":b.dependencies_sha256,"reason":"execution allowance exhausted; not source uncertainty"})).collect();
    crate::tender_analysis::tools::bounded_page(
        &rows,
        number("offset")?,
        number("limit")?,
        max_bytes,
    )
}

pub(super) fn check_read_scope(
    input: &FrozenInput,
    state: &Checkpoint,
    name: &str,
    args: &Value,
) -> Result<(), String> {
    let work = if state.workspace.reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    let Some(work) = work else {
        return Ok(());
    };
    let source = match name {
        "read_source" | "read_source_view" => args["source_id"].as_str(),
        "read_form" => input
            .structured_forms
            .iter()
            .find(|f| f["form_definition_revision_id"] == args["form_id"])
            .and_then(|f| f["source_unit_revision_id"].as_str()),
        _ => return Ok(()),
    }
    .ok_or("known source identity required")?;
    // Independent review may need another source to check this item. Reading
    // support evidence never changes its assigned judgment or write authority.
    if state.workspace.reviewing && work.status == Status::Active {
        return Ok(());
    }
    if work.status != Status::Active || !work.source_scope.iter().any(|id| id == source) {
        return Err(
            "source lies outside active composition work; explicitly expand or hand off the scope"
                .into(),
        );
    }
    Ok(())
}

/// Keep complete protocol groups. Prefer navigation and duplicate evidence over
/// unique source/candidate/render details; the latest pending group is ineligible.
pub(super) fn evict_history(
    transcript: &mut Vec<Value>,
    work: Option<&Work>,
    allow_unique: bool,
) -> bool {
    use std::collections::BTreeSet;
    let starts: Vec<_> = transcript
        .iter()
        .enumerate()
        .filter(|(_, m)| m["role"] == "assistant")
        .map(|(i, _)| i)
        .collect();
    if starts.len() < 2 {
        return false;
    }
    let groups: Vec<_> = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            (
                if i == 0 { 0 } else { start },
                starts.get(i + 1).copied().unwrap_or(transcript.len()),
            )
        })
        .collect();
    let evidence: Vec<BTreeSet<String>> = groups
        .iter()
        .map(|&(start, end)| {
            let mut keys = BTreeSet::new();
            for message in &transcript[start..end] {
                if message["source_view_refs"].is_array() {
                    keys.insert(message["source_view_refs"].to_string());
                }
                let Some(output) = message["content"]
                    .as_str()
                    .and_then(|v| serde_json::from_str::<Value>(v).ok())
                    .filter(|v| v["ok"] == true)
                else {
                    continue;
                };
                let value = &output["result"];
                let in_scope =
                    |id: &str| work.is_none_or(|w| w.source_scope.iter().any(|s| s == id));
                if value["source_id"].as_str().is_some_and(in_scope)
                    && (value["text"].is_string() || value["form_id"].is_string())
                    && let Ok(key) = digest(value)
                {
                    keys.insert(key);
                }
                for item in value["items"].as_array().into_iter().flatten() {
                    let detail = value["view"] == "detail"
                        && (item["sources"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|span| span["source_id"].as_str().is_some_and(in_scope))
                            || item["from"].is_string());
                    let composition = item["key"].is_string() && item.get("value").is_some();
                    if (detail || composition)
                        && let Ok(key) = digest(item)
                    {
                        keys.insert(key);
                    }
                }
            }
            keys
        })
        .collect();
    let redundant = (0..groups.len() - 1).find(|&i| {
        evidence[i].iter().all(|key| {
            evidence
                .iter()
                .enumerate()
                .any(|(j, other)| j != i && other.contains(key))
        })
    });
    let Some(index) = redundant.or_else(|| allow_unique.then_some(0)) else {
        return false;
    };
    let (start, end) = groups[index];
    transcript.drain(start..end);
    true
}

/// Persist normalization with the completed batch. Request sizing uses only a
/// transcript copy and can select a smaller window without editing this state.
pub(super) fn normalize_history(state: &mut Checkpoint, limits: &Limits) -> Result<(), String> {
    let replan = state.execution().watch.needs_replan_context();
    let work = if state.workspace.reviewing {
        // A review item can depend on independently delivered other sources.
        None
    } else {
        state.main_work.as_ref()
    };
    if replan {
        while evict_history(&mut state.transcript, work, false) {}
    }
    while serde_json::to_vec(&state.transcript)
        .map_err(|e| e.to_string())?
        .len()
        > limits.max_context_bytes
    {
        if !evict_history(&mut state.transcript, work, true) {
            break;
        }
    }
    Ok(())
}

/// One batch of role-local evidence, accepted only from the next frozen request.
/// Payloads stay in transcript/source_views; this stores receipt identities only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingDelivery {
    pub reviewing: bool,
    pub coverage: Coverage,
    pub inspected: std::collections::BTreeMap<String, String>,
    pub messages: std::collections::BTreeMap<String, String>,
    pub view_ids: Vec<String>,
}
impl PendingDelivery {
    pub(super) fn new(state: &Checkpoint) -> Self {
        Self {
            reviewing: state.workspace.reviewing,
            coverage: if state.workspace.reviewing {
                state.workspace.review_coverage.clone()
            } else {
                state.workspace.source_coverage.clone()
            },
            inspected: state.workspace.inspected.clone(),
            messages: Default::default(),
            view_ids: vec![],
        }
    }
}

pub(super) fn delivers_evidence(name: &str) -> bool {
    matches!(
        name,
        "collection_index"
            | "read_source"
            | "read_form"
            | "read_source_view"
            | "inspect_analysis"
            | "inspect_composition"
            | "inspect_rendered"
            | "inspect_rendered_cells"
            | "inspect_placements"
            | "inspect_findings"
    )
}

/// Rereading the exact same role-local evidence does not require a model turn
/// solely to acknowledge an empty receipt delta. Keep its tool results and
/// read cost; never discard a batch that adds or changes any qualification.
pub(super) fn discard_redundant_pending(state: &mut Checkpoint) -> Result<(), String> {
    let Some(pending) = &state.pending_delivery else {
        return Ok(());
    };
    let current = if state.workspace.reviewing {
        &state.workspace.review_coverage
    } else {
        &state.workspace.source_coverage
    };
    if pending.reviewing == state.workspace.reviewing
        && pending.inspected == state.workspace.inspected
        && digest(&pending.coverage)? == digest(current)?
    {
        state.pending_delivery = None;
    }
    Ok(())
}

pub(super) fn accept_pending(
    state: &mut Checkpoint,
    result: &AnalysisResult,
) -> Result<(), String> {
    let Some(pending) = &state.pending_delivery else {
        return Ok(());
    };
    let journal = state
        .journal
        .pending
        .as_ref()
        .ok_or("pending composition evidence lacks a frozen request")?;
    if pending.reviewing != state.workspace.reviewing
        || journal.role
            != if pending.reviewing {
                "reviewer"
            } else {
                "main"
            }
    {
        return Err("pending composition evidence belongs to another role".into());
    }
    let body: Value = serde_json::from_slice(state.journal.body().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let messages = body["messages"]
        .as_array()
        .ok_or("frozen composition messages missing")?;
    for (id, expected) in &pending.messages {
        if !messages.iter().any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == *id
                && message["content"]
                    .as_str()
                    .is_some_and(|content| digest(&content).ok().as_ref() == Some(expected))
        }) {
            return Err(
                "pending composition evidence was not delivered in the frozen request".into(),
            );
        }
    }
    for id in &pending.view_ids {
        let view = result
            .source_views
            .get(id)
            .ok_or("pending original view missing")?;
        let url = format!("data:image/jpeg;base64,{}", view.jpeg_base64);
        if !messages.iter().any(|message| {
            message["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["image_url"]["url"] == url))
        }) {
            return Err("pending original pixels were not delivered in the frozen request".into());
        }
    }
    let pending = state
        .pending_delivery
        .take()
        .expect("validated pending delivery");
    if pending.reviewing {
        state.workspace.review_coverage = pending.coverage;
    } else {
        state.workspace.source_coverage = pending.coverage;
    }
    state.workspace.inspected = pending.inspected;
    Ok(())
}

/// Successful complete batches may perform mechanical transitions without a
/// model round. Failed compilation is ordinary durable feedback for this draft.
pub(super) fn advance_main(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut Checkpoint,
    limits: &Limits,
) -> Result<(), String> {
    if state.workspace.reviewing
        || state.workspace.done
        || state.pending_delivery.is_some()
        || !state.main_progress.blockers.is_empty()
        || !state.review_progress.blockers.is_empty()
        || state.workspace.draft.presentation.is_none()
        || !plan_complete(result, &state.workspace.draft)?
        || !state
            .workspace
            .draft
            .plan
            .values()
            .all(|item| saved_implementation(state, item))
    {
        return Ok(());
    }
    let draft_sha256 = digest(&state.workspace.draft)?;
    if state
        .workspace
        .compile_feedback
        .as_ref()
        .is_some_and(|feedback| feedback.draft_sha256 == draft_sha256)
        || state
            .workspace
            .plan_reviews
            .values()
            .any(|review| review.draft_sha256 == draft_sha256)
    {
        return Ok(());
    }
    let tool_limits = super::tools::ToolLimits {
        max_result_bytes: limits.max_tool_result_bytes,
        max_docx_bytes: limits.max_docx_bytes,
        max_review_rounds: limits.max_review_rounds,
    };
    let outcome = (|| {
        if !state.workspace.artifact.as_ref().is_some_and(|artifact| {
            implementation_manifest_complete(&state.workspace.draft, &artifact.manifest)
        }) {
            state
                .workspace
                .invoke(input, result, "compile_docx", &json!({}), tool_limits)?;
        }
        state.workspace.invoke(
            input,
            result,
            "request_composition_review",
            &json!({}),
            tool_limits,
        )?;
        Ok::<(), String>(())
    })();
    match outcome {
        Ok(()) => {
            if let Some(work) = &mut state.main_work {
                work.status = Status::Complete;
            }
            state.review_work = None;
        }
        Err(error) => {
            state.workspace.compile_feedback = Some(super::tools::CompileFeedback {
                draft_sha256,
                error,
            });
        }
    }
    Ok(())
}
