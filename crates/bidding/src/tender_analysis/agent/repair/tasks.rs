//! Stable finding identities and bounded Main repair attempts. Scheduling is
//! explicit; request sizing and candidate reads must not mutate this ledger.
use super::*;
use crate::agent_runtime::progress::ProgressWatch;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub active: Option<String>,
    pub aliases: BTreeMap<String, String>,
    pub entries: BTreeMap<String, Entry>,
    pub feedback_sha256: Option<String>,
    pub last_committed_turn: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub finding_sha256: String,
    pub watch: ProgressWatch,
    pub committed_turns: usize,
    pub attempted_dependencies: BTreeSet<String>,
    pub inherited_blocked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub finding_sha256: String,
    pub source_scope: Vec<String>,
    pub dependencies_sha256: String,
    pub complete: bool,
    pub exhausted: bool,
    pub committed_turns: usize,
}

pub(in crate::tender_analysis) fn limit(limits: &Limits) -> Result<usize, String> {
    let attempts = limits
        .max_focus_replans
        .checked_add(1)
        .and_then(|n| limits.max_focus_turns.checked_mul(n))
        .ok_or("repair task turn limit overflow")?;
    let limit = limits.max_turns.min(attempts);
    if limit == 0 {
        return Err("repair task turn limit must be positive".into());
    }
    Ok(limit)
}

impl Entry {
    fn exhausted(&self, dependency: &str, cap: usize) -> bool {
        self.inherited_blocked
            || self.committed_turns >= cap
            || (self.watch.recovery == Recovery::Blocked
                && self.attempted_dependencies.contains(dependency))
    }
}

fn findings(state: &Checkpoint) -> Result<Vec<(String, String, &Finding)>, String> {
    let mut draft_ids: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (id, draft) in &state.review_draft {
        draft_ids.entry(digest(draft)?).or_default().push(id);
    }
    let mut out = vec![];
    for finding in state.findings_for_repair() {
        let sha = digest(finding)?;
        let ids = draft_ids
            .get(&sha)
            .ok_or("repair feedback has no persisted host finding ID")?;
        for id in ids {
            out.push(((*id).to_owned(), sha.clone(), finding));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    Ok(out)
}

fn dependencies(state: &Checkpoint, finding: &Finding, sha: &str) -> Result<String, String> {
    let mut keys = BTreeSet::new();
    for affected in &finding.affected {
        keys.insert(if state.analysis.relations.contains_key(&affected.id) {
            format!("relation:{}", affected.id)
        } else {
            format!("record:{}", affected.id)
        });
    }
    if let Some(receipt) = state.repair.results.get(sha) {
        keys.extend(receipt.candidate_versions.keys().cloned());
    }
    if keys.is_empty() {
        return context::raw_scope_dependencies(state, &finding_scope(state, finding, sha));
    }
    let values: BTreeMap<_, _> = keys
        .into_iter()
        .map(|key| {
            let value = if context::reference(&state.analysis, &key).is_ok() {
                Some(source_review::candidate_reference_values(state, &key)?)
            } else {
                None
            };
            Ok((key, value))
        })
        .collect::<Result<_, String>>()?;
    digest(&values)
}

pub(in crate::tender_analysis) fn current(
    state: &Checkpoint,
    limits: &Limits,
) -> Result<Vec<Task>, String> {
    let cap = limit(limits)?;
    let generation = feedback_version(state)?;
    if state.repair.tasks.feedback_sha256.as_ref() != Some(&generation) {
        return Ok(vec![]);
    }
    let mut out = BTreeMap::new();
    for (alias, sha, finding) in findings(state)? {
        let id = state
            .repair
            .tasks
            .aliases
            .get(&alias)
            .ok_or("current finding has no repair task")?;
        let entry = state
            .repair
            .tasks
            .entries
            .get(id)
            .ok_or("repair task entry missing")?;
        if entry.finding_sha256 != sha {
            return Err("repair task finding changed inside the frozen phase".into());
        }
        let dependency = dependencies(state, finding, &sha)?;
        out.insert(
            id.clone(),
            Task {
                id: id.clone(),
                finding_sha256: sha.clone(),
                source_scope: finding_scope(state, finding, &sha),
                exhausted: entry.exhausted(&dependency, cap),
                dependencies_sha256: dependency,
                complete: state.repair.feedback_sha256.as_ref() == Some(&generation)
                    && valid(state, finding, &sha)?,
                committed_turns: entry.committed_turns,
            },
        );
    }
    Ok(out.into_values().collect())
}

pub(in crate::tender_analysis) fn active(state: &Checkpoint) -> Option<(&str, &Entry)> {
    let id = state.repair.tasks.active.as_deref()?;
    Some((id, state.repair.tasks.entries.get(id)?))
}

pub(in crate::tender_analysis) fn check_put(
    state: &Checkpoint,
    sha: &str,
    limits: &Limits,
) -> Result<(), String> {
    let id = state
        .repair
        .tasks
        .active
        .as_deref()
        .ok_or("select a repair task before saving its disposition")?;
    if !current(state, limits)?
        .iter()
        .any(|t| t.id == id && t.finding_sha256 == sha && !t.exhausted)
    {
        return Err(
            "repair disposition must belong to the active task with remaining allowance".into(),
        );
    }
    Ok(())
}
