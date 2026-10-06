//! Complete tender parse. Image OCR is concurrent; published units stay in parser order.

use docparser::{StructuredSourceLocator, StructuredSourceUnit, StructuredSourceUnitKind};
use std::collections::{BTreeSet, HashSet};
use std::future::Future;

const DEFAULT_IMAGE_CONCURRENCY: usize = 4;
const MAX_IMAGE_CONCURRENCY: usize = 16;

/// How many image OCR calls may run at once. `1` keeps the work sequential.
pub fn image_concurrency() -> usize {
    std::env::var("KB_TENDER_PARSE_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_IMAGE_CONCURRENCY)
        .clamp(1, MAX_IMAGE_CONCURRENCY)
}

/// Run `f` over `items` with at most `limit` futures in flight.
///
/// Results follow completion order. Callers that need parser order must key
/// each result and reassemble it themselves.
pub async fn map_concurrent<T, R, F, Fut>(items: Vec<T>, limit: usize, f: F) -> Vec<R>
where
    T: Send,
    F: Fn(T) -> Fut,
    Fut: Future<Output = R>,
    R: Send,
{
    use futures::stream::{self, StreamExt};
    stream::iter(items)
        .map(f)
        .buffer_unordered(limit.max(1))
        .collect()
        .await
}

/// Parser units that must be published. Empty owning sections are structural
/// anchors, not missing content. Every other unit is required.
pub fn expected_unit_keys(
    units: &[StructuredSourceUnit],
    content_sections: &HashSet<u32>,
) -> BTreeSet<String> {
    units
        .iter()
        .filter(|unit| !is_empty_owning_section(unit, content_sections))
        .map(|unit| unit.key.clone())
        .collect()
}

pub fn assert_complete(
    expected: &BTreeSet<String>,
    published: &BTreeSet<String>,
) -> Result<(), String> {
    if expected == published {
        return Ok(());
    }
    let missing: Vec<_> = expected.difference(published).cloned().collect();
    let extra: Vec<_> = published.difference(expected).cloned().collect();
    Err(format!(
        "tender parse is incomplete: missing {missing:?}, extra {extra:?}"
    ))
}

fn is_empty_owning_section(unit: &StructuredSourceUnit, content_sections: &HashSet<u32>) -> bool {
    unit.kind == StructuredSourceUnitKind::Section
        && unit.text.is_empty()
        && matches!(
            &unit.locator,
            StructuredSourceLocator::Document {
                section_ordinal,
                table_ordinal: None,
                row_ordinal: None,
                form_ordinal: None,
                ..
            } if content_sections.contains(section_ordinal)
        )
}

pub(crate) fn content_sections(units: &[StructuredSourceUnit]) -> HashSet<u32> {
    units
        .iter()
        .filter_map(|unit| match (&unit.kind, &unit.locator) {
            (
                StructuredSourceUnitKind::TableRegion
                | StructuredSourceUnitKind::TableRow
                | StructuredSourceUnitKind::FormRegion,
                StructuredSourceLocator::Document {
                    section_ordinal, ..
                },
            ) => Some(*section_ordinal),
            (
                StructuredSourceUnitKind::ImageRegion,
                StructuredSourceLocator::Image {
                    compound_parent: Some(parent),
                    ..
                },
            ) => {
                use docparser::CompoundImageParent;
                match parent {
                    CompoundImageParent::Paragraph {
                        section_ordinal, ..
                    }
                    | CompoundImageParent::TableCell {
                        section_ordinal, ..
                    }
                    | CompoundImageParent::Form {
                        section_ordinal, ..
                    } => Some(*section_ordinal),
                }
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn image_work_overlaps_when_concurrency_allows_it() {
        let current = AtomicUsize::new(0);
        let max = AtomicUsize::new(0);
        map_concurrent(vec![0, 1, 2, 3], 4, |_| async {
            let now = current.fetch_add(1, Ordering::SeqCst) + 1;
            max.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(40)).await;
            current.fetch_sub(1, Ordering::SeqCst);
        })
        .await;
        assert!(
            max.load(Ordering::SeqCst) > 1,
            "concurrent parse must overlap image work"
        );
    }

    #[tokio::test]
    async fn concurrency_one_does_not_overlap() {
        let current = AtomicUsize::new(0);
        let max = AtomicUsize::new(0);
        map_concurrent(vec![0, 1, 2], 1, |_| async {
            let now = current.fetch_add(1, Ordering::SeqCst) + 1;
            max.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            current.fetch_sub(1, Ordering::SeqCst);
        })
        .await;
        assert_eq!(max.load(Ordering::SeqCst), 1);
    }
}
