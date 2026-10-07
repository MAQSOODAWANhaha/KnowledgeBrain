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

/// Parser order of units that must be published. This is the publication order.
pub fn publication_order(units: &[StructuredSourceUnit]) -> Vec<String> {
    let sections = content_sections(units);
    units
        .iter()
        .filter(|unit| !is_empty_owning_section(unit, &sections))
        .map(|unit| unit.key.clone())
        .collect()
}

/// Published keys must equal the parser order. A missing unit fails the file.
/// A reorder also fails: OCR may finish out of order, but publication may not.
pub fn assert_publication_order(expected: &[String], published: &[String]) -> Result<(), String> {
    if expected == published {
        return Ok(());
    }
    let expected_set: BTreeSet<_> = expected.iter().cloned().collect();
    let published_set: BTreeSet<_> = published.iter().cloned().collect();
    if expected_set != published_set {
        return assert_complete(&expected_set, &published_set);
    }
    Err(format!(
        "tender parse order differs from the parser: expected {expected:?}, published {published:?}"
    ))
}

/// Put finished image results back into parser order.
pub fn place_in_publication_order<T>(
    order: &[String],
    finished: Vec<(String, T)>,
) -> Result<Vec<T>, String> {
    let mut by_key = std::collections::HashMap::new();
    for (key, value) in finished {
        if by_key.insert(key.clone(), value).is_some() {
            return Err(format!("tender parse published {key} twice"));
        }
    }
    let mut published = Vec::with_capacity(order.len());
    for key in order {
        let Some(value) = by_key.remove(key) else {
            return Err(format!("tender parse is incomplete: missing {key}"));
        };
        published.push(value);
    }
    if let Some((key, _)) = by_key.into_iter().next() {
        return Err(format!("tender parse published extra unit {key}"));
    }
    Ok(published)
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

    #[tokio::test]
    async fn two_image_ocr_calls_overlap() {
        let current = AtomicUsize::new(0);
        let max = AtomicUsize::new(0);
        map_concurrent(vec!["image-a", "image-b"], 4, |_| async {
            let now = current.fetch_add(1, Ordering::SeqCst) + 1;
            max.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(40)).await;
            current.fetch_sub(1, Ordering::SeqCst);
        })
        .await;
        assert_eq!(max.load(Ordering::SeqCst), 2);
    }

    fn document_unit(
        key: &str,
        kind: StructuredSourceUnitKind,
        section: u32,
        text: &str,
    ) -> StructuredSourceUnit {
        StructuredSourceUnit {
            key: key.into(),
            ordinal: 0,
            kind,
            text: text.into(),
            locator: StructuredSourceLocator::Document {
                section_ordinal: section,
                table_ordinal: None,
                row_ordinal: None,
                form_ordinal: None,
                heading_path: String::new(),
            },
            grid: None,
        }
    }

    #[test]
    fn missing_unit_fails_and_finished_images_return_to_parser_order() {
        let mut table = document_unit("table", StructuredSourceUnitKind::TableRegion, 0, "");
        if let StructuredSourceLocator::Document { table_ordinal, .. } = &mut table.locator {
            *table_ordinal = Some(0);
        }
        let units = vec![
            document_unit("section", StructuredSourceUnitKind::Section, 0, ""),
            document_unit("body", StructuredSourceUnitKind::Section, 1, "投标函"),
            table,
        ];
        let order = publication_order(&units);
        assert_eq!(order, vec!["body".to_string(), "table".to_string()]);

        let err = assert_publication_order(&order, &["body".into()]).unwrap_err();
        assert!(err.contains("table"), "{err}");

        let err = assert_publication_order(&order, &["table".into(), "body".into()]).unwrap_err();
        assert!(err.contains("order"), "{err}");

        let placed = place_in_publication_order(
            &order,
            vec![("table".into(), "表格"), ("body".into(), "正文")],
        )
        .unwrap();
        assert_eq!(placed, vec!["正文", "表格"]);
    }
}
