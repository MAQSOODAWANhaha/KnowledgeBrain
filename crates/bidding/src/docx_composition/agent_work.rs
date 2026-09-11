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
    pub action: Action,
    pub objective: String,
    pub note: String,
    pub status: Status,
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
    digest(&scope_sections(state, scope))
}

pub(super) fn set_work(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    let work: Work = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
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
    for id in &work.section_scope {
        let section = state
            .workspace
            .draft
            .sections
            .get(id)
            .ok_or("unknown focused section")?;
        if !section
            .grounds
            .iter()
            .any(|s| work.source_scope.contains(&s.source_id))
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
        if unread || sections.is_empty() {
            return Err("complete local work needs delivered original evidence and saved source-backed sections".into());
        }
        if state.workspace.reviewing
            && state
                .workspace
                .review_gaps(input, result)?
                .iter()
                .any(|gap| {
                    gap["key"].as_str().is_some_and(|key| {
                        sections.iter().any(|s| key == format!("section:{}", s.id))
                    })
                })
        {
            return Err(
                "independently inspect the current local sections before completion".into(),
            );
        }
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
    for section in scope_sections(state, &scope) {
        versions.push(digest(section)?);
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
        completion.or(local_completion),
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

pub(super) fn packet(state: &Checkpoint) -> Value {
    let progress = state.execution();
    let work = if state.workspace.reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    json!({"work":work,"watch":progress.watch,"blocker_count":progress.blockers.len(),
        "instruction":"On replan, narrow to a source-backed section and save it or inspect a specific missing field. On blocked, select independent sources. Execution blockers prevent acceptance and are not source uncertainty."})
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
pub(super) fn evict_history(state: &mut Checkpoint, allow_unique: bool) -> bool {
    use std::collections::BTreeSet;
    let starts: Vec<_> = state
        .transcript
        .iter()
        .enumerate()
        .filter(|(_, m)| m["role"] == "assistant")
        .map(|(i, _)| i)
        .collect();
    if starts.len() < 2 {
        return false;
    }
    let work = if state.workspace.reviewing {
        &state.review_work
    } else {
        &state.main_work
    };
    let groups: Vec<_> = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            (
                if i == 0 { 0 } else { start },
                starts.get(i + 1).copied().unwrap_or(state.transcript.len()),
            )
        })
        .collect();
    let evidence: Vec<BTreeSet<String>> = groups
        .iter()
        .map(|&(start, end)| {
            let mut keys = BTreeSet::new();
            for message in &state.transcript[start..end] {
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
                let in_scope = |id: &str| {
                    work.as_ref()
                        .is_none_or(|w| w.source_scope.iter().any(|s| s == id))
                };
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
    state.transcript.drain(start..end);
    true
}
