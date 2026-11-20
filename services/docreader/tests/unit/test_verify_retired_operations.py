"""BUG-253: verify sends only review; retired operations stay out of prompt and planning."""
import pytest

from docreader.tender.checks.analysis import _Analysis
from docreader.tender.prompts import get_prompt
from test_analysis_checks import IDENTITY, Model, budget, run, scenario


def test_verify_prompt_describes_only_review():
    prompt = get_prompt("verify")
    for retired in ("operation=locate", "operation=associate", "joint", "work_units", "candidate"):
        assert retired not in prompt


@pytest.mark.asyncio
async def test_verify_never_builds_locate_payloads(tmp_path, monkeypatch):
    data = await scenario(tmp_path, analysis_issue=True)
    built = []
    payload = _Analysis.payload

    def record(self, operation, work_unit, **fields):
        built.append(operation)
        return payload(self, operation, work_unit, **fields)

    monkeypatch.setattr(_Analysis, "payload", record)
    result = await run(data, Model())
    assert set(built) == {"review"}
    assert result["problem_routing"]


@pytest.mark.asyncio
async def test_page_siblings_without_disposition_are_context_not_review_targets(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    target = next(iter(context.dispositions))
    sibling = next(r for r in context.sources if r != target)
    context.dispositions.pop(sibling)
    key = next(iter(context.pages)) if context.pages else ("doc", 0)
    context.pages[key] = {target, sibling}
    ranges = context._expand_review_ranges([target])
    assert ranges == sorted(ranges, key=lambda r: context.source_order(context.sources[r]))
    payload = context.review_payload(ranges)
    assert [d["range_id"] for d in payload["dispositions"]] == [target]
    supplied = {s["range_id"] for v in payload["source_views"] for k in ("body", "supporting") for s in v[k]}
    assert sibling in supplied
