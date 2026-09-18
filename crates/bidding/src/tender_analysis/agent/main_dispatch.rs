//! Main owns one stable source/global root or references the existing repair
//! ledger. Preparation projects identity; only received/committed may install it.
use super::*;
use crate::agent_runtime::progress::ProgressWatch;
use std::collections::BTreeSet;

pub(super) const POLICY: &str = "main-dispatch-v1";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub active: Option<Active>,
    pub entries: BTreeMap<String, Entry>,
    pub last_committed_turn: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Active {
    Ordinary(String),
    Repair(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub source_id: Option<String>,
    pub spent_batches: usize,
    pub spent_replans: usize,
    pub watch: ProgressWatch,
    pub attempted_dependencies: BTreeSet<String>,
    pub completed_dependencies: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub owner: Active,
    pub source_scope: Vec<String>,
    pub dependencies_sha256: String,
}

fn key(input: &FrozenInput, source: Option<&str>) -> Result<String, String> {
    digest(&json!([POLICY, digest(input)?, source]))
}

fn pack_of(input: &FrozenInput, limits: &Limits, source_id: &str) -> Vec<String> {
    crate::tender_analysis::pack::pack_containing(
        input,
        limits.pack_max_units,
        limits.pack_max_chars,
        source_id,
    )
}

fn pack_owner_key(input: &FrozenInput, pack: &[String]) -> Result<String, String> {
    match pack {
        [] => key(input, None),
        [id] => key(input, Some(id)),
        many => {
            let mut ids = many.to_vec();
            ids.sort();
            key(input, Some(&digest(&ids)?))
        }
    }
}

// Field grounds and template regions are evidence dependencies too. Only
// schema-shaped spans count, never prose mentioning another source.
fn span_sources(value: &Value, sources: &mut BTreeSet<String>) {
    match value {
        Value::Object(fields) => {
            if fields.contains_key("start")
                && fields.contains_key("end")
                && let Some(id) = fields.get("source_id").and_then(Value::as_str)
            {
                sources.insert(id.into());
            }
            for value in fields.values() {
                span_sources(value, sources);
            }
        }
        Value::Array(values) => {
            for value in values {
                span_sources(value, sources);
            }
        }
        _ => {}
    }
}

/// Follow saved semantic links rather than assuming distinct source IDs are
/// independent. This is dependency closure, not a guess about unread prose.
fn dependencies(state: &Checkpoint, scope: &[String]) -> Result<(Vec<String>, Vec<Value>), String> {
    let mut sources: BTreeSet<_> = scope.iter().cloned().collect();
    let mut records = BTreeSet::new();
    let mut relations = BTreeSet::new();
    loop {
        let before = (sources.len(), records.len(), relations.len());
        for (id, record) in &state.analysis.records {
            let value = json!(record);
            let mut cited = BTreeSet::new();
            span_sources(&value, &mut cited);
            if records.contains(id) || cited.iter().any(|id| sources.contains(id)) {
                records.insert(id.clone());
                sources.extend(cited);
                if let RecordData::Template {
                    parent: Some(parent),
                    ..
                } = &record.data
                {
                    records.insert(parent.clone());
                }
            }
        }
        for (id, relation) in &state.analysis.relations {
            if records.contains(&relation.from)
                || records.contains(&relation.to)
                || relation
                    .grounds
                    .iter()
                    .any(|s| sources.contains(&s.source_id))
            {
                relations.insert(id.clone());
                records.extend([relation.from.clone(), relation.to.clone()]);
                sources.extend(relation.grounds.iter().map(|s| s.source_id.clone()));
            }
        }
        if before == (sources.len(), records.len(), relations.len()) {
            break;
        }
    }
    let mut values = Vec::new();
    for id in records {
        values.push(
            state
                .analysis
                .records
                .get(&id)
                .map(|r| json!(r))
                .unwrap_or_else(|| json!({"id":id,"missing":true})),
        );
    }
    for id in relations {
        values.push(
            state
                .analysis
                .relations
                .get(&id)
                .map(|r| json!(r))
                .unwrap_or_else(|| json!({"id":id,"missing":true})),
        );
    }
    for id in &sources {
        if let Some(disposition) = state.analysis.dispositions.get(id) {
            values.push(json!({"source_id":id,"disposition":disposition}));
        }
    }
    Ok((sources.into_iter().collect(), values))
}

fn remaining_in_pack(
    input: &FrozenInput,
    state: &Checkpoint,
    limits: &Limits,
    pack: &[String],
) -> Result<Vec<String>, String> {
    let mut remaining = Vec::new();
    for id in pack {
        if source_complete(input, state, std::slice::from_ref(id))? {
            continue;
        }
        if blocked_scope(input, state, limits, std::slice::from_ref(id))? {
            continue;
        }
        let mut trial = remaining.clone();
        trial.push(id.clone());
        if blocked_scope(input, state, limits, &trial)? {
            continue;
        }
        remaining.push(id.clone());
    }
    Ok(remaining)
}

fn root_projection(
    input: &FrozenInput,
    limits: &Limits,
    state: &Checkpoint,
    id: &str,
    entry: &Entry,
) -> Result<Projection, String> {
    let source_scope = match &entry.source_id {
        None => input
            .source_units
            .iter()
            .map(|s| s.source_unit_revision_id.clone())
            .collect(),
        Some(source) => pack_of(input, limits, source),
    };
    let dependencies_sha256 = if entry.source_id.is_some() {
        digest(&dependencies(state, &source_scope)?.1)?
    } else {
        rule_contract::scope_sha256(input, &state.analysis)?
    };
    Ok(Projection {
        owner: Active::Ordinary(id.into()),
        source_scope,
        dependencies_sha256,
    })
}

pub(in crate::tender_analysis) fn source_complete(
    input: &FrozenInput,
    state: &Checkpoint,
    scope: &[String],
) -> Result<bool, String> {
    if !repair::recovery_completion_gaps(state, scope)?.is_empty() {
        return Ok(false);
    }
    let (_, values) = dependencies(state, scope)?;
    let record_ids: BTreeSet<_> = values.iter().filter_map(|v| v["id"].as_str()).collect();
    Ok(!tools::gaps(input, &state.analysis).iter().any(|gap| {
        let source = gap["source_id"].as_str().or_else(|| {
            input
                .structured_forms
                .iter()
                .find(|f| f["form_definition_revision_id"] == gap["form_id"])
                .and_then(|f| f["source_unit_revision_id"].as_str())
        });
        source.is_some_and(|id| scope.iter().any(|s| s == id))
            || gap["id"].as_str().is_some_and(|id| record_ids.contains(id))
    }))
}

fn blocked_scope(
    input: &FrozenInput,
    state: &Checkpoint,
    limits: &Limits,
    scope: &[String],
) -> Result<bool, String> {
    let (closed, _) = dependencies(state, scope)?;
    if context::scope_is_blocked(state, &closed)? {
        return Ok(true);
    }
    for entry in state.dispatch.entries.values() {
        let Some(source) = &entry.source_id else {
            continue;
        };
        let overlap: Vec<String> = pack_of(input, limits, source)
            .into_iter()
            .filter(|id| scope.contains(id) || closed.contains(id))
            .collect();
        if overlap.is_empty() {
            continue;
        }
        let dependency = digest(&dependencies(state, &overlap)?.1)?;
        if entry.completed_dependencies.as_ref() != Some(&dependency)
            && exhausted(entry, &dependency, limits)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn exhausted(entry: &Entry, dependency: &str, limits: &Limits) -> Result<bool, String> {
    let cap = if limits.pack_max_turns > 0 && entry.source_id.is_some() {
        limits.pack_max_turns
    } else {
        repair::tasks::limit(limits)?
    };
    Ok(entry.spent_batches >= cap
        || entry.spent_replans > limits.max_focus_replans
        || (entry.watch.recovery == Recovery::Blocked
            && entry.attempted_dependencies.contains(dependency)))
}

fn leftover_incomplete(input: &FrozenInput, state: &Checkpoint) -> Result<bool, String> {
    for source in &input.source_units {
        if !source_complete(
            input,
            state,
            std::slice::from_ref(&source.source_unit_revision_id),
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn main_global_checks_complete(state: &Checkpoint) -> bool {
    crate::tender_analysis::rule_contract::ANALYSIS_GLOBAL_CHECK_KEYS
        .iter()
        .all(|key| state.analysis.main_global_checks.contains_key(*key))
}

pub(in crate::tender_analysis) fn any_source_complete(
    input: &FrozenInput,
    state: &Checkpoint,
) -> Result<bool, String> {
    for source in &input.source_units {
        if source_complete(
            input,
            state,
            std::slice::from_ref(&source.source_unit_revision_id),
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(in crate::tender_analysis) fn source_is_host_closed(
    input: &FrozenInput,
    state: &Checkpoint,
    limits: &Limits,
    source_id: &str,
) -> Result<bool, String> {
    let pack = pack_of(input, limits, source_id);
    let Some(entry) = state.dispatch.entries.get(&pack_owner_key(input, &pack)?) else {
        return Ok(false);
    };
    let source = source_id.to_owned();
    let dependency = digest(&dependencies(state, std::slice::from_ref(&source))?.1)?;
    exhausted(entry, &dependency, limits)
}

fn seeds(input: &FrozenInput, limits: &Limits) -> Result<BTreeMap<String, Entry>, String> {
    let mut entries = BTreeMap::new();
    for pack in
        crate::tender_analysis::pack::packs(input, limits.pack_max_units, limits.pack_max_chars)
    {
        entries.insert(
            pack_owner_key(input, &pack)?,
            Entry {
                source_id: pack.first().cloned(),
                ..Default::default()
            },
        );
    }
    entries.insert(key(input, None)?, Entry::default());
    Ok(entries)
}

#[derive(Debug)]
pub(in crate::tender_analysis) enum Continuation {
    Continue(Projection),
    EnterReview,
    Stop,
}

fn continuation(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Continuation, String> {
    let repairs = repair::tasks::current(state, &config.limits)?;
    if let Some(task) = repairs
        .iter()
        .filter(|t| !t.complete && !t.exhausted)
        .find(|t| repair_task_host::check_scope(state, &t.source_scope).is_ok())
    {
        return Ok(Continuation::Continue(Projection {
            owner: Active::Repair(task.id.clone()),
            source_scope: task.source_scope.clone(),
            dependencies_sha256: task.dependencies_sha256.clone(),
        }));
    }
    if state.review_rounds == 0
        && config.limits.reviewer_reserve > 0
        && any_source_complete(input, state)?
        && state.turn.saturating_add(config.limits.reviewer_reserve) >= config.limits.max_turns
    {
        return Ok(Continuation::EnterReview);
    }
    let initial;
    let entries = if state.dispatch.entries.is_empty() {
        initial = seeds(input, &config.limits)?;
        &initial
    } else {
        &state.dispatch.entries
    };
    for pack in crate::tender_analysis::pack::packs(
        input,
        config.limits.pack_max_units,
        config.limits.pack_max_chars,
    ) {
        let remaining = remaining_in_pack(input, state, &config.limits, &pack)?;
        if remaining.is_empty() {
            continue;
        }
        let id = pack_owner_key(input, &pack)?;
        let Some(entry) = entries.get(&id) else {
            continue;
        };
        let mut projection = root_projection(input, &config.limits, state, &id, entry)?;
        projection.source_scope = remaining;
        projection.dependencies_sha256 = digest(&dependencies(state, &projection.source_scope)?.1)?;
        if !exhausted(entry, &projection.dependencies_sha256, &config.limits)? {
            return Ok(Continuation::Continue(projection));
        }
    }
    let any_complete = any_source_complete(input, state)?;
    if !any_complete {
        return Ok(Continuation::Stop);
    }
    if state.review_rounds > 0 && repairs.iter().any(|task| !task.complete && !task.exhausted) {
        return Ok(Continuation::Stop);
    }
    if leftover_incomplete(input, state)? {
        return Ok(Continuation::EnterReview);
    }
    if !main_global_checks_complete(state) {
        let id = key(input, None)?;
        let entry = entries.get(&id).ok_or("global dispatch root missing")?;
        let projection = root_projection(input, &config.limits, state, &id, entry)?;
        if !exhausted(entry, &projection.dependencies_sha256, &config.limits)? {
            return Ok(Continuation::Continue(projection));
        }
    }
    Ok(Continuation::EnterReview)
}

fn next(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Option<Projection>, String> {
    match continuation(input, config, state)? {
        Continuation::Continue(projection) => Ok(Some(projection)),
        Continuation::EnterReview | Continuation::Stop => Ok(None),
    }
}

pub(in crate::tender_analysis) fn projection(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Option<Projection>, String> {
    if state.role != Role::Main || state.done {
        return Ok(None);
    }
    match &state.dispatch.active {
        Some(Active::Ordinary(id)) => root_projection(
            input,
            &config.limits,
            state,
            id,
            state
                .dispatch
                .entries
                .get(id)
                .ok_or("active dispatch root missing")?,
        )
        .map(Some),
        Some(Active::Repair(id)) => repair::tasks::current(state, &config.limits)?
            .into_iter()
            .find(|t| &t.id == id)
            .map(|t| Projection {
                owner: Active::Repair(t.id),
                source_scope: t.source_scope,
                dependencies_sha256: t.dependencies_sha256,
            })
            .ok_or_else(|| "active dispatch repair task missing".to_string())
            .map(Some),
        None => next(input, config, state),
    }
}

pub(in crate::tender_analysis) fn work(
    _input: &FrozenInput,
    _limits: &Limits,
    state: &Checkpoint,
    assigned: &Projection,
) -> WorkState {
    if (state.dispatch.active.as_ref() == Some(&assigned.owner)
        || (state.dispatch.active.is_none()
            && state.main_work.as_ref().is_some_and(|w| {
                assigned
                    .source_scope
                    .iter()
                    .all(|id| w.source_scope.contains(id))
            })))
        && let Some(work) = &state.main_work
    {
        return work.clone();
    }
    let mut work = WorkState {source_scope:assigned.source_scope.clone(), deferred_sources:vec![],
        objective: match &assigned.owner {
            Active::Repair(id) => format!("Resolve assigned review finding {id} from original evidence"),
            Active::Ordinary(id) if state.dispatch.entries.get(id).is_some_and(|e| e.source_id.is_none()) =>
                "Close collection metadata, cross-source relationships and current global analysis checks.".into(),
            _ => "Extract source-grounded records, relationships and disposition from the assigned frozen source.".into(),
        },focus:Default::default(),output_refs:vec![],pending_refs:vec![],status:WorkStatus::Active,note:String::new()};
    context::retain_outcomes(&state.analysis, &mut work, state.main_work.as_ref());
    work
}

pub(in crate::tender_analysis) fn check_scope(
    input: &FrozenInput,
    state: &Checkpoint,
    limits: &Limits,
    scope: &[String],
) -> Result<(), String> {
    if let Some(Active::Ordinary(id)) = &state.dispatch.active
        && let Some(source) = state
            .dispatch
            .entries
            .get(id)
            .and_then(|e| e.source_id.as_ref())
    {
        let pack = pack_of(input, limits, source);
        if !scope.iter().any(|id| pack.contains(id)) {
            return Err("expanded work must retain the assigned pack; only the host changes owner at batch end".into());
        }
    }
    if let Some(Active::Repair(_)) = &state.dispatch.active {
        repair_task_host::check_scope(state, scope)?;
    }
    if blocked_scope(input, state, limits, scope)? {
        return Err(
            "work scope depends on a blocked root; preserve its owner and allowance".into(),
        );
    }
    Ok(())
}

pub(in crate::tender_analysis) fn check_action(
    input: &FrozenInput,
    state: &Checkpoint,
    name: &str,
    args: &Value,
) -> Result<(), String> {
    if state.role != Role::Main || state.dispatch.active.is_none() || name == "set_work_note" {
        return Ok(());
    }
    let work = state
        .main_work
        .as_ref()
        .ok_or("assigned Main work missing")?;
    if context::scope_is_blocked(state, &work.source_scope)?
        && !matches!(
            name,
            "inspect_review" | "check_gaps" | "source_index" | "collection_index" | "set_work_note"
        )
    {
        return Err("blocked work permits only recovery navigation until its dependencies or delivered history allow retry".into());
    }
    if !matches!(
        name,
        "put_record" | "put_relation" | "delete_record" | "delete_relation" | "set_disposition"
    ) {
        return Ok(());
    }
    let expanded = evidence_refs::expand(input, args)?;
    let mut sources = BTreeSet::new();
    span_sources(&expanded, &mut sources);
    if name == "set_disposition"
        && let Some(id) = args["source_id"].as_str()
    {
        sources.insert(id.into());
    }
    let existing = match name {
        "put_record" | "delete_record" => args["id"]
            .as_str()
            .and_then(|id| state.analysis.records.get(id))
            .map(|r| json!(r)),
        "put_relation" | "delete_relation" => args["id"]
            .as_str()
            .and_then(|id| state.analysis.relations.get(id))
            .map(|r| json!(r)),
        _ => None,
    };
    if let Some(existing) = existing {
        span_sources(&existing, &mut sources);
    }
    if name == "put_relation" {
        for id in [args["from"].as_str(), args["to"].as_str()]
            .into_iter()
            .flatten()
        {
            if let Some(record) = state.analysis.records.get(id) {
                span_sources(&json!(record), &mut sources);
            }
        }
    }
    if sources.iter().any(|id| !work.source_scope.contains(id)) {
        return Err("write references sources outside the assigned work; expand scope for the grounded cross-reference without changing owner".into());
    }
    Ok(())
}
