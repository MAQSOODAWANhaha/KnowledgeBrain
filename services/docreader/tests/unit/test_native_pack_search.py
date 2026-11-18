"""Native pack search preserves coverage and checks every emitted request."""
from types import SimpleNamespace

from docreader.llm.request import ModelBudgetError
from docreader.tender import native_requirement_sources as native


def test_native_pack_search_checks_every_pack_and_routes_an_oversize_record(monkeypatch):
    count = 1030
    oversized = 517
    capacities = (48, 16, 35)
    calls, admitted = [], set()

    def require(budget, view, payload, prompt, *, request):
        ordinals = tuple(view.range_ordinals)
        assert ordinals == payload['ordinals'] == request
        calls.append(ordinals)
        capacity = capacities[(ordinals[0] // 200) % len(capacities)]
        if oversized in ordinals or len(ordinals) > capacity:
            raise ModelBudgetError('budget exceeded')
        admitted.add(ordinals)

    monkeypatch.setattr(native, 'require_native_budget', require)
    ledger = SimpleNamespace(
        ranges=[{'reference': {'unit_id': str(i)}, 'base': {'range_id': str(i)}} for i in range(count)],
        binding='frozen-source', view=lambda ordinals: SimpleNamespace(range_ordinals=ordinals),
    )
    packs = native.plan_native_packs(
        ledger, None, lambda view: {'ordinals': tuple(view.range_ordinals)}, 'Select source.',
        request_factory=lambda view, payload: payload['ordinals'],
    )
    groups = [tuple(view.range_ordinals) for view in packs['views']]
    assert all(group in admitted for group in groups)
    for group in groups:
        end = group[-1] + 1
        assert (end in {oversized, count}
                or len(group) == capacities[(group[0] // 200) % len(capacities)])
    assert [ordinal for group in groups for ordinal in group] == [i for i in range(count) if i != oversized]
    pending, = packs['pending_ranges']
    assert pending['range_ordinals'] == [oversized]
    assert pending['source_reference'] == {'unit_id': str(oversized)}
    assert pending['outlet'] == 'manual_source_review'
    assert (oversized,) in calls
    assert not packs['complete']
    assert packs['model_calls'] == 0
    assert len(calls) < count // 2
