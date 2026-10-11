//! Test-only inclusive wall timings; no source content or credentials recorded.
use std::{cell::RefCell, collections::BTreeMap, time::Instant};
thread_local! {
    static COUNTERS: RefCell<BTreeMap<&'static str, u64>> = const { RefCell::new(BTreeMap::new()) };
    static ACTIVE: RefCell<Option<BTreeMap<&'static str, (u64,u128)>>> = const { RefCell::new(None) };
}
pub(crate) struct Span(&'static str, Option<Instant>);
pub(crate) fn enter(name: &'static str) -> Span {
    Span(
        name,
        ACTIVE.with(|s| s.borrow().is_some()).then(Instant::now),
    )
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(start) = self.1 {
            ACTIVE.with(|s| {
                if let Some(rows) = s.borrow_mut().as_mut() {
                    let row = rows.entry(self.0).or_default();
                    row.0 += 1;
                    row.1 += start.elapsed().as_micros();
                }
            });
        }
    }
}
pub(crate) fn reset() {
    COUNTERS.with(|s| s.borrow_mut().clear());
    ACTIVE.with(|s| *s.borrow_mut() = Some(BTreeMap::new()));
}
pub(crate) fn finish() -> BTreeMap<&'static str, (u64, u128)> {
    ACTIVE.with(|s| s.borrow_mut().take().unwrap_or_default())
}

/// Offline projection of every already-planned pack, with no dispatch or save.
pub(crate) async fn all_pack_requests<J: super::Journal>(
    input: &crate::analysis::FrozenInput,
    config: &super::Config,
    planned: &super::Checkpoint,
    journal: &J,
    output_directory: &std::path::Path,
) -> Result<Vec<serde_json::Value>, crate::agent_error::AgentError> {
    super::discover_parallel::profile_all_pack_requests(
        input,
        config,
        planned,
        journal,
        output_directory,
    )
    .await
}

// Numeric quantities have their own units; they are never microsecond spans.
pub(crate) fn observe(name: &'static str, value: usize) {
    if ACTIVE.with(|s| s.borrow().is_some()) {
        COUNTERS.with(|s| *s.borrow_mut().entry(name).or_default() += value as u64);
    }
}
pub(crate) fn observe_max(name: &'static str, value: usize) {
    if ACTIVE.with(|s| s.borrow().is_some()) {
        COUNTERS.with(|s| {
            let mut values = s.borrow_mut();
            let previous = values.entry(name).or_default();
            *previous = (*previous).max(value as u64);
        });
    }
}
pub(crate) fn finish_counters() -> BTreeMap<&'static str, u64> {
    COUNTERS.with(|s| std::mem::take(&mut *s.borrow_mut()))
}
