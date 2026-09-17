//! 按冻结库存打分析窗口。测试默认一源一包；产品把能装进窗口的整份文档打成一包。
use super::{FrozenInput, Source};

pub fn pack_containing(
    input: &FrozenInput,
    max_units: usize,
    max_chars: usize,
    source_id: &str,
) -> Vec<String> {
    packs(input, max_units, max_chars)
        .into_iter()
        .find(|pack| pack.iter().any(|id| id == source_id))
        .unwrap_or_else(|| vec![source_id.to_string()])
}

pub fn packs(input: &FrozenInput, max_units: usize, max_chars: usize) -> Vec<Vec<String>> {
    if max_units <= 1 || input.source_units.is_empty() {
        return input
            .source_units
            .iter()
            .map(|source| vec![source.source_unit_revision_id.clone()])
            .collect();
    }
    let char_limit = if max_chars == 0 {
        usize::MAX
    } else {
        max_chars
    };
    let mut order = Vec::new();
    for source in &input.source_units {
        if !order.contains(&source.document_id) {
            order.push(source.document_id.clone());
        }
    }
    let mut packs = Vec::new();
    for document in order {
        let mut units: Vec<&Source> = input
            .source_units
            .iter()
            .filter(|source| source.document_id == document)
            .collect();
        units.sort_by_key(|source| source.ordinal);
        let total: usize = units.iter().map(|source| source.text.len()).sum();
        if total <= char_limit {
            packs.push(
                units
                    .iter()
                    .map(|source| source.source_unit_revision_id.clone())
                    .collect(),
            );
            continue;
        }
        let mut current = Vec::new();
        let mut chars: usize = 0;
        for source in units {
            let extra = source.text.len();
            // 空表不占字数，跟在当前窗口后面，避免单独开会话。
            if !current.is_empty()
                && (current.len() >= max_units
                    || (extra > 0 && chars.saturating_add(extra) > char_limit))
            {
                packs.push(std::mem::take(&mut current));
                chars = 0;
            }
            current.push(source.source_unit_revision_id.clone());
            chars = chars.saturating_add(extra);
        }
        if !current.is_empty() {
            packs.push(current);
        }
    }
    packs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tender_analysis::Source;
    use serde_json::json;

    fn source(
        id: &str,
        document: &str,
        ordinal: usize,
        heading: &str,
        text: &str,
        table: bool,
    ) -> Source {
        Source {
            source_unit_revision_id: id.into(),
            document_id: document.into(),
            text: text.into(),
            locator: json!({
                "heading_path": heading,
                "table_ordinal": if table { serde_json::Value::from(0) } else { serde_json::Value::Null }
            }),
            ordinal,
        }
    }

    fn input(units: Vec<Source>) -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "p".into(),
            document_set_id: "s".into(),
            documents: vec![json!({"id": "d"})],
            document_relations: vec![],
            source_units: units,
            structured_forms: vec![],
            decisions: vec![],
        }
    }

    #[test]
    fn default_is_one_source_per_pack() {
        let packed = packs(
            &input(vec![
                source("a", "d", 0, "一", "aaa", false),
                source("b", "d", 1, "一", "bbb", false),
            ]),
            1,
            1200,
        );
        assert_eq!(packed, vec![vec!["a".to_string()], vec!["b".to_string()]]);
    }

    #[test]
    fn small_document_including_tables_is_one_pack() {
        let packed = packs(
            &input(vec![
                source("p1", "d", 0, "一", "条款甲", false),
                source("p2", "d", 1, "一", "条款乙", false),
                source("t1", "d", 2, "一", "", true),
                source("p3", "d", 3, "二", "条款丙", false),
            ]),
            8,
            1200,
        );
        assert_eq!(
            packed,
            vec![vec![
                "p1".to_string(),
                "p2".to_string(),
                "t1".to_string(),
                "p3".to_string()
            ]]
        );
    }

    #[test]
    fn twenty_eight_short_units_fit_in_one_window() {
        let mut units: Vec<_> = (0..22)
            .map(|i| {
                source(
                    &format!("p{i}"),
                    "d",
                    i,
                    "节",
                    "约四十字的短条款正文。",
                    false,
                )
            })
            .collect();
        units.extend((22..28).map(|i| source(&format!("t{i}"), "d", i, "表", "", true)));
        let packed = packs(&input(units), 32, 8000);
        assert_eq!(packed.len(), 1);
        assert_eq!(packed[0].len(), 28);
    }

    #[test]
    fn pack_containing_returns_the_merged_siblings() {
        let packed = pack_containing(
            &input(vec![
                source("p1", "d", 0, "一", "条款甲", false),
                source("p2", "d", 1, "一", "条款乙", false),
            ]),
            8,
            1200,
            "p2",
        );
        assert_eq!(packed, vec!["p1".to_string(), "p2".to_string()]);
    }

    #[test]
    fn oversize_document_splits_by_chars_and_keeps_tables_with_neighbors() {
        let long = "字".repeat(500);
        let packed = packs(
            &input(vec![
                source("p1", "d", 0, "一", &long, false),
                source("t1", "d", 1, "一", "", true),
                source("p2", "d", 2, "二", &long, false),
                source("p3", "d", 3, "二", &long, false),
            ]),
            8,
            1200,
        );
        assert_eq!(
            packed,
            vec![
                vec!["p1".to_string(), "t1".to_string()],
                vec!["p2".to_string()],
                vec!["p3".to_string()],
            ]
        );
    }
}
