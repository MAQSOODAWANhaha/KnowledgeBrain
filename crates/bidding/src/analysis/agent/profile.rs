//! Test-only inclusive wall timings; no source content or credentials recorded.
use std::{cell::RefCell, collections::BTreeMap, time::Instant};
thread_local! {
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
    ACTIVE.with(|s| *s.borrow_mut() = Some(BTreeMap::new()));
}
pub(crate) fn finish() -> BTreeMap<&'static str, (u64, u128)> {
    ACTIVE.with(|s| s.borrow_mut().take().unwrap_or_default())
}
