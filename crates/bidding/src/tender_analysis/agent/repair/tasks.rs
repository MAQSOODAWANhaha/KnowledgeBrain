//! Stable finding identities and bounded Main repair attempts. Scheduling is
//! explicit; request sizing and candidate reads must not mutate this ledger.
use super::*;
use crate::agent_runtime::progress::ProgressWatch;

#[cfg(test)]
mod tests;

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

struct Seed {
    id: String,
    sha: String,
    inherited: Option<ProgressWatch>,
}

fn spent(watch: &ProgressWatch) -> bool {
    watch.focus_turns > 0
        || watch.no_progress_turns > 0
        || watch.replans > 0
        || watch.recovery != Recovery::Running
}

fn combine(into: &mut Entry, other: &Entry) -> Result<(), String> {
    into.committed_turns = into
        .committed_turns
        .checked_add(other.committed_turns)
        .ok_or("repair task spent turns overflow")?;
    into.attempted_dependencies
        .extend(other.attempted_dependencies.iter().cloned());
    into.inherited_blocked |= other.inherited_blocked;
    into.watch.focus_turns = into.watch.focus_turns.max(other.watch.focus_turns);
    into.watch.no_progress_turns = into
        .watch
        .no_progress_turns
        .max(other.watch.no_progress_turns);
    into.watch.replans = into.watch.replans.max(other.watch.replans);
    if other.watch.recovery == Recovery::Blocked || into.inherited_blocked {
        into.watch.recovery = Recovery::Blocked;
    } else if other.watch.recovery == Recovery::Replan && into.watch.recovery == Recovery::Running {
        into.watch.recovery = Recovery::Replan;
    }
    Ok(())
}

impl State {
    fn synchronize(&mut self, generation: String, seeds: &[Seed]) -> Result<(), String> {
        if self.feedback_sha256.as_ref() == Some(&generation) {
            return Ok(());
        }
        let mut next = self.clone();
        let by_id: BTreeMap<_, _> = seeds
            .iter()
            .map(|s| (s.id.as_str(), s.sha.as_str()))
            .collect();
        let mut groups: BTreeMap<&str, Vec<&Seed>> = BTreeMap::new();
        for seed in seeds {
            groups.entry(&seed.sha).or_default().push(seed);
        }
        let mut claimed = BTreeSet::new();
        for (sha, group) in groups {
            let previous: BTreeSet<_> = group
                .iter()
                .filter_map(|s| self.aliases.get(&s.id))
                .cloned()
                .chain(
                    self.entries
                        .iter()
                        .filter(|(id, entry)| {
                            entry.finding_sha256 == sha && self.aliases.get(*id) == Some(*id)
                        })
                        .map(|(id, _)| id.clone()),
                )
                .collect();
            let canonical = previous
                .iter()
                .find(|id| {
                    !claimed.contains(*id)
                        && by_id.get(id.as_str()).is_none_or(|current| *current == sha)
                })
                .cloned()
                .unwrap_or_else(|| group[0].id.clone());
            if !claimed.insert(canonical.clone()) {
                return Err("ambiguous repair task identity".into());
            }
            let mut entry = Entry {
                finding_sha256: sha.into(),
                watch: ProgressWatch::default(),
                committed_turns: 0,
                attempted_dependencies: BTreeSet::new(),
                inherited_blocked: false,
            };
            for id in &previous {
                combine(
                    &mut entry,
                    self.entries
                        .get(id)
                        .ok_or("repair alias has no ledger entry")?,
                )?;
            }
            // A retired alias that splits again inherits its shared full spend;
            // retaining an older entry must never reduce already recorded spend.
            if let Some(old) = self.entries.get(&canonical) {
                entry.committed_turns = entry.committed_turns.max(old.committed_turns);
                entry.watch.replans = entry.watch.replans.max(old.watch.replans);
                entry
                    .attempted_dependencies
                    .extend(old.attempted_dependencies.iter().cloned());
                entry.inherited_blocked |= old.inherited_blocked;
            }
            if previous.is_empty() {
                for prior in group.iter().filter_map(|s| s.inherited.as_ref()) {
                    entry.watch = prior.clone();
                    entry.inherited_blocked = true;
                }
            }
            if entry.inherited_blocked {
                entry.watch.recovery = Recovery::Blocked;
            }
            for (id, target) in &mut next.aliases {
                if previous.contains(target)
                    && by_id.get(id.as_str()).is_none_or(|current| *current == sha)
                {
                    *target = canonical.clone();
                }
            }
            for seed in group {
                next.aliases.insert(seed.id.clone(), canonical.clone());
            }
            next.aliases.insert(canonical.clone(), canonical.clone());
            next.entries.insert(canonical, entry);
        }
        next.active = None;
        next.feedback_sha256 = Some(generation);
        *self = next;
        Ok(())
    }

    fn enter(&mut self, id: &str, dependency: &str, cap: usize) -> Result<(), String> {
        let entry = self.entries.get_mut(id).ok_or("unknown repair task")?;
        if entry.exhausted(dependency, cap) {
            return Err("repair task allowance exhausted".into());
        }
        if entry.watch.recovery == Recovery::Blocked {
            entry.watch = ProgressWatch {
                replans: entry.watch.replans,
                ..Default::default()
            };
        }
        entry.attempted_dependencies.insert(dependency.into());
        self.active = Some(id.into());
        Ok(())
    }

    fn commit(
        &mut self,
        id: &str,
        turn: usize,
        watch: ProgressWatch,
        cap: usize,
    ) -> Result<(), String> {
        if self.last_committed_turn == Some(turn) {
            return Ok(());
        }
        if self.last_committed_turn.is_some_and(|last| last > turn) {
            return Err("repair task commit order regressed".into());
        }
        let entry = self
            .entries
            .get_mut(id)
            .ok_or("charged repair task is missing")?;
        if watch.replans < entry.watch.replans {
            return Err("repair task replan allowance cannot be refunded".into());
        }
        entry.committed_turns = entry
            .committed_turns
            .checked_add(1)
            .ok_or("repair task committed turns overflow")?;
        entry.watch = watch;
        if entry.committed_turns >= cap || entry.inherited_blocked {
            entry.watch.recovery = Recovery::Blocked;
        }
        self.last_committed_turn = Some(turn);
        Ok(())
    }
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

pub(in crate::tender_analysis) fn sync(
    state: &mut Checkpoint,
    limits: &Limits,
) -> Result<(), String> {
    limit(limits)?;
    if state.role != Role::Main {
        return Ok(());
    }
    let seeds = findings(state)?
        .into_iter()
        .map(|(id, sha, finding)| {
            let scope = finding_scope(state, finding, &sha);
            let inherited = state
                .main_progress
                .blockers
                .iter()
                .find(|b| b.scope.iter().any(|s| scope.contains(s)))
                .map(|b| b.watch.clone())
                .or_else(|| {
                    (state.repair.tasks.feedback_sha256.is_none()
                        && spent(&state.main_progress.watch)
                        && state
                            .main_work
                            .as_ref()
                            .is_some_and(|w| w.source_scope.iter().any(|s| scope.contains(s))))
                    .then(|| state.main_progress.watch.clone())
                });
            Seed { id, sha, inherited }
        })
        .collect::<Vec<_>>();
    let generation = feedback_version(state)?;
    state.repair.tasks.synchronize(generation, &seeds)
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

pub(in crate::tender_analysis) fn select(
    state: &mut Checkpoint,
    id: &str,
    limits: &Limits,
) -> Result<(), String> {
    let task = current(state, limits)?
        .into_iter()
        .find(|t| t.id == id && !t.complete)
        .ok_or("select a current unfinished repair task")?;
    state
        .repair
        .tasks
        .enter(id, &task.dependencies_sha256, limit(limits)?)
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

pub(in crate::tender_analysis) fn after_batch(
    state: &mut Checkpoint,
    charged_task_id: &str,
    committed_turn: usize,
    watch: ProgressWatch,
    limits: &Limits,
) -> Result<(), String> {
    state
        .repair
        .tasks
        .commit(charged_task_id, committed_turn, watch, limit(limits)?)
}
