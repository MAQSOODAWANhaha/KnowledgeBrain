//! End-of-batch Main completion and independent-review handoff.
use super::*;

pub(super) fn check(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<(), String> {
    let some_source_finished = main_dispatch::any_source_complete(input, state)?;
    for blocker in state
        .main_progress
        .blockers
        .iter()
        .chain(state.reviewer_progress.blockers.iter())
    {
        let mut closed = some_source_finished;
        if closed {
            for id in &blocker.scope {
                closed = closed
                    && (main_dispatch::source_complete(input, state, std::slice::from_ref(id))?
                        || main_dispatch::source_is_host_closed(input, state, &config.limits, id)?);
            }
        }
        if !closed {
            return Err(
                "execution blockers remain; they cannot be published as source uncertainty".into(),
            );
        }
    }
    if state
        .work()
        .is_some_and(|work| !work.deferred_sources.is_empty())
    {
        return Err("resume deferred_sources before requesting independent review".into());
    }
    let gaps = tools::gaps(input, &state.analysis);
    for gap in &gaps {
        let source = gap["source_id"].as_str().or_else(|| {
            input
                .structured_forms
                .iter()
                .find(|form| form["form_definition_revision_id"] == gap["form_id"])
                .and_then(|form| form["source_unit_revision_id"].as_str())
        });
        let closed = match source {
            Some(id) => {
                some_source_finished
                    && main_dispatch::source_is_host_closed(input, state, &config.limits, id)?
            }
            None => false,
        };
        if !closed {
            return Err(format!(
                "{} structural/reading gaps remain; use check_gaps",
                gaps.len()
            ));
        }
    }
    let feedback = repair_feedback_packet(state, &[], &config.limits)?;
    if feedback["unread"] != 0 {
        return Err(format!("repair feedback has unread findings: {feedback}"));
    }
    let repairs = repair::packet(state, &config.limits)?;
    if repairs["pending"] != 0 {
        return Err(format!("repair dispositions remain: {repairs}"));
    }
    let checks: Vec<_> = state
        .analysis
        .main_global_checks
        .values()
        .cloned()
        .collect();
    let findings: Vec<_> = state.review_draft.values().cloned().collect();
    rule_contract::validate_inventory(input, &state.analysis, &checks, &findings)?;
    for check in &checks {
        rule_contract::validate_evidence(input, &state.analysis, &state.analysis.coverage, check)?;
    }
    Ok(())
}

pub(super) fn enter(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
) -> Result<Value, String> {
    check(input, config, state)?;
    state.pending_coverage = None;
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
