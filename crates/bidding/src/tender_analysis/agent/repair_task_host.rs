//! Main repair scheduling at real journal boundaries, never during sizing.
use super::*;

pub(super) const POLICY: &str = "main-repair-tasks-v1";

pub(in crate::tender_analysis) fn schedule(
    state: &mut Checkpoint,
    limits: &Limits,
) -> Result<(), String> {
    if state.role != Role::Main || state.done {
        state.repair.tasks.active = None;
        return Ok(());
    }
    let previous = state.repair.tasks.active.clone();
    repair::tasks::sync(state, limits)?;
    let tasks = repair::tasks::current(state, limits)?;
    let ready = |task: &&repair::tasks::Task| runnable(state, task);
    let next = tasks
        .iter()
        .filter(ready)
        .find(|task| Some(&task.id) == previous.as_ref())
        .or_else(|| tasks.iter().find(ready));
    if let Some(task) = next {
        repair::tasks::select(state, &task.id, limits)?;
        state.main_progress.watch = repair::tasks::active(state)
            .ok_or("selected repair task missing")?
            .1
            .watch
            .clone();
        if previous.as_ref() != Some(&task.id) {
            let deferred_sources = state
                .main_work
                .as_ref()
                .map(|work| {
                    work.deferred_sources
                        .iter()
                        .filter(|id| !task.source_scope.contains(id))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            state.main_work = Some(WorkState {
                source_scope: task.source_scope.clone(), deferred_sources,
                objective: format!("Resolve assigned review finding {} from original evidence", task.id),
                focus: Default::default(), output_refs: vec![], pending_refs: vec![],
                status: WorkStatus::Active,
                note: "Save a grounded revision or dispute; task allowance belongs to the finding, including cross-source work.".into(),
            });
            context::retain_outcomes(&state.analysis, state.main_work.as_mut().unwrap(), None);
        }
    } else {
        state.repair.tasks.active = None;
        if previous.is_some() && tasks.iter().all(|task| task.complete) {
            // The task watch remains in its entry. Source/reviewer blockers and
            // all publication gates still apply to the subsequent handoff.
            state.main_progress.watch = Default::default();
        }
    }
    Ok(())
}

// New task allowances cannot reopen an old source-level failure. The legacy
// recovery path remains available only outside fixed-task execution.
pub(super) fn check_scope(state: &Checkpoint, scope: &[String]) -> Result<(), String> {
    if state
        .main_progress
        .blockers
        .iter()
        .any(|blocker| blocker.scope.iter().any(|id| scope.contains(id)))
    {
        return Err("assigned repair task cannot enter a legacy blocked source; its prior allowance is retained".into());
    }
    Ok(())
}

fn runnable(state: &Checkpoint, task: &repair::tasks::Task) -> bool {
    !task.complete
        && !task.exhausted
        && !task.source_scope.is_empty()
        && check_scope(state, &task.source_scope).is_ok()
}

pub(super) fn exhausted(state: &Checkpoint, limits: &Limits) -> bool {
    if state.role == Role::Main && !state.findings_for_repair().is_empty() {
        // Unsynchronized new feedback is selected at prepare, not blocked by
        // the watch borrowed from the previous task or extraction scope.
        if let Ok(tasks) = repair::tasks::current(state, limits) {
            if tasks.is_empty() {
                return false;
            }
            if tasks.iter().any(|task| !task.complete) {
                return !tasks.iter().any(|task| runnable(state, task));
            }
        }
    }
    // Once all receipts are current, normal bounded source/review handoff
    // resumes. Completed repair tasks must not disable its terminal gate.
    state.execution().handoff_exhausted(&limits.progress())
}

pub(super) fn check_ready(state: &Checkpoint, limits: &Limits) -> Result<(), AgentError> {
    if exhausted(state, limits) {
        return Err(error(
            "AGENT_TURN_BUDGET_EXCEEDED",
            "repair task or subsequent handoff allowance exhausted; task allowances and legacy blockers retained",
        ));
    }
    Ok(())
}
