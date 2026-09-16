//! End-of-batch Main completion and independent-review handoff.
use super::*;

pub(super) fn check(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<(), String> {
    if !state.main_progress.blockers.is_empty() || !state.reviewer_progress.blockers.is_empty() {
        return Err(
            "execution blockers remain; they cannot be published as source uncertainty".into(),
        );
    }
    if state
        .work()
        .is_some_and(|work| !work.deferred_sources.is_empty())
    {
        return Err("resume deferred_sources before requesting independent review".into());
    }
    let gaps = tools::gaps(input, &state.analysis);
    if !gaps.is_empty() {
        return Err(format!(
            "{} structural/reading gaps remain; use check_gaps",
            gaps.len()
        ));
    }
    let feedback = repair_feedback_packet(state, &[], &config.limits)?;
    if feedback["unread"] != 0 {
        return Err(format!("repair feedback has unread findings: {feedback}"));
    }
    let repairs = repair::packet(state, &config.limits)?;
    if repairs["pending"] != 0 {
        return Err(format!("repair dispositions remain: {repairs}"));
    }
    Ok(())
}

pub(super) fn enter(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
) -> Result<Value, String> {
    check(input, config, state)?;
    state.role = Role::Reviewer;
    // Frozen sources are unchanged. Keep this reviewer's own receipts;
    // candidate digest checks invalidate precisely the edited versions.
    state.reviewer_work = None;
    if state.source_review.is_none() {
        state.source_review = Some(source_review::initialize(input, config)?);
    }
    source_review::select_next(input, config, state)?;
    Ok(json!({"reviewing":digest(&state.analysis)?}))
}

/// Capture explicit local completion. It cannot establish global completeness;
/// the final batch and collection gates are checked independently below.
pub(super) fn completion(
    input: &FrozenInput,
    state: &Checkpoint,
) -> Result<Option<String>, String> {
    let Some(work) = state.main_work.as_ref().filter(|work| {
        state.role == Role::Main
            && work.status == WorkStatus::Complete
            && work.deferred_sources.is_empty()
    }) else {
        return Ok(None);
    };
    if input.source_units.is_empty() {
        return Ok(None);
    }
    // Preserve unresolved source outcomes for the independent reviewer. They
    // are not execution failures and must not be silently discarded here.
    digest(&json!([state.analysis, work])).map(Some)
}

pub(super) fn finish(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    completed: Option<&str>,
    batch_failed: bool,
    disposition_submitted: bool,
) -> Result<(), String> {
    // Tool reads staged by Main must be delivered to Main, not promoted into
    // the independent reviewer's coverage after a role change.
    if batch_failed || state.role != Role::Main || state.pending_coverage.is_some() {
        return Ok(());
    }
    if let Some(completed) = completed {
        if completion(input, state)?.as_deref() != Some(completed) {
            return Ok(());
        }
    } else {
        // A disposition is the model's explicit account of a processed source.
        // Only normal extraction can use it to finish a package; repair has its
        // own task dispositions, dependency versions and independent gates.
        if !disposition_submitted || !main_work::normal(state) {
            return Ok(());
        }
        let Some(mut work) = state
            .main_work
            .clone()
            .filter(|work| work.status == WorkStatus::Active && work.deferred_sources.is_empty())
        else {
            return Ok(());
        };
        work.status = WorkStatus::Complete;
        context::retain_outcomes(&state.analysis, &mut work, state.main_work.as_ref());
        if context::validate(input, state, &work, config.limits.max_tool_result_bytes).is_err() {
            return Ok(());
        }
        state.main_work = Some(work);
        // As with explicit scope completion, release delivered old evidence
        // before preparing another package. Keep the current protocol group
        // intact so its write identities and outcomes still reach the model.
        if let Some(start) = state
            .transcript
            .iter()
            .rposition(|message| message["role"] == "assistant")
        {
            state.transcript.drain(..start);
        }
    }
    let work = state
        .main_work
        .as_ref()
        .expect("source-package completion checked");
    if context::validate(input, state, work, config.limits.max_tool_result_bytes).is_err()
        || check(input, config, state).is_err()
    {
        return Ok(());
    }
    enter(input, config, state)?;
    Ok(())
}
