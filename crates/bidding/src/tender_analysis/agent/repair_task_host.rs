//! Main repair scheduling at real journal boundaries, never during sizing.
use super::*;

pub(super) const POLICY: &str = "main-repair-tasks-v1";

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
