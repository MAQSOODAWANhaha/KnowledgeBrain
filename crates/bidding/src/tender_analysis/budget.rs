//! Frozen extraction turn/physical budgets from dispatch work units.
//! Operator JSON is a floor. Estimates never grow mid-run.
use super::FrozenInput;
use serde::{Deserialize, Serialize};

pub const TURN_FLOOR: usize = 128;
pub const TURN_CEILING: usize = 2048;
pub const TURNS_PER_PROSE_SOURCE: usize = 12;
pub const TURNS_PER_TABLE_SOURCE: usize = 16;
pub const TURNS_PER_DOCUMENT: usize = 8;
pub const TURN_OVERHEAD: usize = 48;
pub const REVIEW_TURNS_PER_SOURCE: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionBudget {
    pub prose_sources: usize,
    pub table_sources: usize,
    pub documents: usize,
    pub operator_turns: usize,
    pub estimated_turns: usize,
    pub applied_turns: usize,
    pub reviewer_reserve: usize,
    pub operator_physical: usize,
    pub estimated_physical: usize,
    pub applied_physical: usize,
    pub clipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetRefused {
    pub estimated_turns: usize,
    pub ceiling: usize,
}

pub fn is_table_source(locator: &serde_json::Value, text: &str) -> bool {
    if text.trim().is_empty() {
        return true;
    }
    !locator
        .get("table_ordinal")
        .is_none_or(|value| value.is_null())
}

pub fn classify_sources(input: &FrozenInput) -> (usize, usize) {
    let mut prose = 0;
    let mut table = 0;
    for source in &input.source_units {
        if is_table_source(&source.locator, &source.text) {
            table += 1;
        } else {
            prose += 1;
        }
    }
    (prose, table)
}

pub fn estimated_turns(prose: usize, table: usize, documents: usize) -> usize {
    TURN_OVERHEAD
        .saturating_add(prose.saturating_mul(TURNS_PER_PROSE_SOURCE))
        .saturating_add(table.saturating_mul(TURNS_PER_TABLE_SOURCE))
        .saturating_add(documents.max(1).saturating_mul(TURNS_PER_DOCUMENT))
        .max(TURN_FLOOR)
}

pub fn estimated_physical(turns: usize) -> usize {
    turns.saturating_add(turns / 8).max(64)
}

pub fn apply(
    input: &FrozenInput,
    operator_turns: usize,
    operator_physical: usize,
) -> Result<ExtractionBudget, BudgetRefused> {
    apply_with_pack(input, operator_turns, operator_physical, 1, 0)
}

pub fn apply_with_pack(
    input: &FrozenInput,
    operator_turns: usize,
    operator_physical: usize,
    pack_max_units: usize,
    pack_max_chars: usize,
) -> Result<ExtractionBudget, BudgetRefused> {
    let packed = crate::tender_analysis::pack::packs(input, pack_max_units, pack_max_chars);
    let mut prose_packs: usize = 0;
    let mut table_packs: usize = 0;
    for pack in &packed {
        if pack.len() == 1 {
            let table = input.source_units.iter().any(|source| {
                source.source_unit_revision_id == pack[0]
                    && is_table_source(&source.locator, &source.text)
            });
            if table {
                table_packs += 1;
                continue;
            }
        }
        prose_packs += 1;
    }
    let (prose, table) = classify_sources(input);
    let documents = input.documents.len().max(1);
    let estimated_turns = TURN_OVERHEAD
        .saturating_add(prose_packs.saturating_mul(3))
        .saturating_add(table_packs.saturating_mul(4))
        .saturating_add(documents.saturating_mul(TURNS_PER_DOCUMENT))
        .max(TURN_FLOOR);
    let estimated_physical = estimated_physical(estimated_turns);
    let reviewer_reserve = packed.len().saturating_mul(3);
    if estimated_turns > TURN_CEILING && operator_turns < estimated_turns {
        return Err(BudgetRefused {
            estimated_turns,
            ceiling: TURN_CEILING,
        });
    }
    let applied_turns = operator_turns.max(estimated_turns.min(TURN_CEILING));
    let applied_physical = operator_physical.max(estimated_physical);
    Ok(ExtractionBudget {
        prose_sources: prose,
        table_sources: table,
        documents,
        operator_turns,
        estimated_turns,
        applied_turns,
        reviewer_reserve,
        operator_physical,
        estimated_physical,
        applied_physical,
        clipped: estimated_turns > TURN_CEILING,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tender_analysis::Source;
    use serde_json::json;

    fn input(sources: Vec<Source>, documents: usize) -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "p".into(),
            document_set_id: "s".into(),
            documents: (0..documents).map(|i| json!({"id": i})).collect(),
            document_relations: vec![],
            decisions: vec![],
            source_units: sources,
            structured_forms: vec![],
        }
    }

    fn prose(id: &str) -> Source {
        Source {
            source_unit_revision_id: id.into(),
            document_id: "d".into(),
            text: "条款正文".into(),
            locator: json!({"table_ordinal": null}),
            ordinal: 0,
        }
    }

    fn table(id: &str) -> Source {
        Source {
            source_unit_revision_id: id.into(),
            document_id: "d".into(),
            text: String::new(),
            locator: json!({"table_ordinal": 1}),
            ordinal: 0,
        }
    }

    #[test]
    fn tables_are_not_double_counted_as_forms() {
        let mut sources: Vec<_> = (0..22).map(|i| prose(&format!("p{i}"))).collect();
        sources.extend((0..6).map(|i| table(&format!("t{i}"))));
        let budget = apply(&input(sources, 1), 96, 64).unwrap();
        assert_eq!(budget.prose_sources, 22);
        assert_eq!(budget.table_sources, 6);
        let expected = TURN_OVERHEAD + 22 * 3 + 6 * 4 + TURNS_PER_DOCUMENT;
        assert_eq!(budget.estimated_turns, expected);
        assert_eq!(budget.reviewer_reserve, 28 * 3);
        assert!(budget.applied_turns >= expected);
        assert!(!budget.clipped);
    }

    #[test]
    fn operator_floor_is_kept_when_higher_than_estimate() {
        let budget = apply(&input(vec![prose("a")], 1), 900, 800).unwrap();
        assert_eq!(budget.applied_turns, 900);
        assert_eq!(budget.applied_physical, 800);
    }

    #[test]
    fn over_ceiling_without_operator_override_is_refused() {
        let sources: Vec<_> = (0..800).map(|i| prose(&format!("s{i}"))).collect();
        let error = apply(&input(sources, 1), 384, 450).unwrap_err();
        assert!(error.estimated_turns > TURN_CEILING);
        assert_eq!(error.ceiling, TURN_CEILING);
    }

    #[test]
    fn tiny_input_uses_floor() {
        let budget = apply(&input(vec![prose("a")], 1), 32, 16).unwrap();
        assert_eq!(budget.applied_turns, TURN_FLOOR);
        assert!(budget.reviewer_reserve >= 3);
    }
}
