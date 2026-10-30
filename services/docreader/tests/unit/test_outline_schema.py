"""Authoring references identify each retained rule without copying its facts."""

import pytest
from pydantic import ValidationError

from docreader.tender.schema import EffectiveItemRef, SourceUnitRef


REF = {"sha256": "a" * 64, "relative_path": "aa/" + "a" * 64, "size": 0, "media_type": "application/json"}
ITEM = {"artifact_ref": REF, "item_id": "original-item"}


def test_whole_and_retained_references_have_distinct_identity():
    whole = EffectiveItemRef(item_ref=ITEM)
    first = EffectiveItemRef(item_ref=ITEM, decision_id="decision", rule_index=0)
    second = EffectiveItemRef(item_ref=ITEM, decision_id="decision", rule_index=1)
    assert whole.model_dump(exclude_unset=True) == {"item_ref": ITEM}
    assert first != second
    assert SourceUnitRef(source_index_ref=REF, unit_id="native-unit").source_index_ref.sha256 == REF["sha256"]


def test_unknown_decision_pair_nulls_normalize_to_the_whole_item_reference():
    payload = {"item_ref": ITEM, "decision_id": None, "rule_index": None}
    whole = EffectiveItemRef(item_ref=ITEM)
    normalized = EffectiveItemRef.model_validate(payload)
    assert normalized == whole
    assert normalized.model_dump(exclude_unset=True) == whole.model_dump(exclude_unset=True)
    assert payload == {"item_ref": ITEM, "decision_id": None, "rule_index": None}


@pytest.mark.parametrize("extra", [
    {"decision_id": "decision"}, {"rule_index": 0},
    {"decision_id": "decision", "rule_index": None}, {"decision_id": None, "rule_index": 0},
    {"decision_id": "decision", "rule_index": -1}, {"decision_id": "decision", "rule_index": True},
    {"scoring": {"points": 2}},
])
def test_incomplete_or_copied_effective_facts_are_rejected(extra):
    with pytest.raises(ValidationError):
        EffectiveItemRef.model_validate({"item_ref": ITEM, **extra})
