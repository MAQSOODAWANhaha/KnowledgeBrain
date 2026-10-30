"""Same-mode outline recovery keeps accepted source receipts and paid accounting."""

from collections import Counter
import json
from types import SimpleNamespace
from uuid import uuid4

import pytest

from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.store import Conflict
from docreader.llm.request import ModelContractError, build_request
from docreader.tender import document_execution

from test_authoring_execution import response
from test_document_execution import analysis_runtime as analysis_runtime, execute


pytestmark = pytest.mark.integration


@pytest.fixture
def outline_pipeline(monkeypatch):
    prompts = document_execution.load_prompts()
    state = SimpleNamespace(prompts=prompts, validated=[])

    def read_prompt(name):
        matches = [task for task, filename in document_execution._PROMPT_FILES.items() if filename == name]
        stem = name[:-3] if name.endswith(".md") else name
        if stem in prompts and stem in matches:
            return prompts[stem]
        return prompts[matches[0]]

    def joinpath(name):
        def read_text(**_kwargs):
            return read_prompt(name)
        return SimpleNamespace(read_text=read_text)

    directory = SimpleNamespace(joinpath=joinpath)
    monkeypatch.setattr(document_execution, "files", lambda _package: directory)

    async def generate(
        artifacts, snapshot, budget, invoke, *, outline_only=False, **_kwargs
    ):
        for ordinal in range(1, 7):
            operation = "source" if ordinal <= 5 else "assemble"
            prompt = "outline" if operation == "source" else "outline_assemble"

            def validate(value):
                if value.get("value") != "accepted":
                    raise ModelContractError(
                        [
                            {
                                "path": ["value"],
                                "code": "bad_value",
                                "detail": "Return accepted.",
                            }
                        ]
                    )
                state.validated.append((operation, ordinal))
                return value

            await invoke(
                "outline",
                build_request(
                    budget.model,
                    budget.prompts[prompt],
                    {"operation": operation, "ordinal": ordinal},
                    task="outline",
                ),
                validate,
            )
        outline = {
            "schema_version": 2,
            "authoring_input": snapshot,
            "issues": [],
            "chapters": [],
        }
        reference = artifacts.put_json(outline)
        if outline_only:
            return reference
        return artifacts.put_json(
            {
                "schema_version": 2,
                "authoring_input": snapshot,
                "outline_ref": reference,
                "basis_ref": reference,
                "unit_refs": [],
                "review_ref": reference,
                "docx_ref": reference,
                "package_ref": reference,
                "gaps": [],
                "usage": {},
            }
        )

    monkeypatch.setattr(document_execution, "generate_document", generate)
    return state


async def test_changed_assembly_prompt_reuses_five_sources_including_corrected_receipt(
    analysis_runtime,
    outline_pipeline,
):
    sent = []

    def handler(request):
        payload = json.loads(
            json.loads(request.content)["messages"][1]["content"][0]["text"]
        )
        sent.append(payload)
        rejected = payload["ordinal"] == 3 and "contract_feedback" not in payload
        return response({"value": "invalid" if rejected else "accepted"})

    first = uuid4().hex
    await execute(analysis_runtime, first, handler, outline_only=True)
    store = AuthoringStore(analysis_runtime[2].dsn)
    previous = await store.get_execution(first)
    assert len(sent) == len(previous["requests"]) == 7
    assert Counter(entry["ordinal"] for entry in sent)[3] == 2
    source_receipts = [
        r for r in previous["receipts"] if r["stage"] == "document-model-response"
    ][:5]
    assert len(source_receipts) == 5

    outline_pipeline.prompts["outline_assemble"] += (
        "\nChanged assembly-only semantic guidance."
    )
    second = uuid4().hex
    await execute(
        analysis_runtime,
        second,
        handler,
        outline_only=True,
        previous_execution_id=first,
    )
    current = await store.get_execution(second)
    assert len(sent) == 8 and sent[-1] == {"operation": "assemble", "ordinal": 6}
    assert (
        len(current["requests"]) == 1
        and current["requests"][0]["usage"]["total_tokens"] == 170
    )
    inherited = [
        r
        for r in current["receipts"]
        if r["metadata"].get("reused_from_execution_id") == first
    ]
    assert [(r["scope"], r["artifact_ref"]) for r in inherited] == [
        (r["scope"], r["artifact_ref"]) for r in source_receipts
    ]
    assert outline_pipeline.validated[-6:] == [("source", i) for i in range(1, 6)] + [
        ("assemble", 6)
    ]
    assert await store.get_execution(first) == previous


@pytest.mark.parametrize(
    ("previous_outline", "current_outline"), [(False, True), (True, False)]
)
async def test_predecessor_operation_must_match_before_any_new_execution(
    analysis_runtime,
    outline_pipeline,
    previous_outline,
    current_outline,
):
    first = uuid4().hex
    await execute(
        analysis_runtime,
        first,
        lambda _request: response({"value": "accepted"}),
        outline_only=previous_outline,
    )
    second = uuid4().hex
    with pytest.raises(Conflict, match="predecessor"):
        await execute(
            analysis_runtime,
            second,
            lambda _request: pytest.fail("unexpected model call"),
            outline_only=current_outline,
            previous_execution_id=first,
        )
    assert (
        await AuthoringStore(analysis_runtime[2].dsn).get_execution(
            second, missing_ok=True
        )
        is None
    )


async def test_outline_predecessor_cannot_cross_analysis_runs(
    analysis_runtime, outline_pipeline
):
    first = uuid4().hex
    await execute(
        analysis_runtime,
        first,
        lambda _request: response({"value": "accepted"}),
        outline_only=True,
    )
    other = await analysis_runtime[0].submit(
        analysis_runtime[3]["manifest_ref"], uuid4().hex
    )
    other_runtime = (*analysis_runtime[:3], other, analysis_runtime[4])
    second = uuid4().hex
    with pytest.raises(Conflict, match="predecessor"):
        await execute(
            other_runtime,
            second,
            lambda _request: pytest.fail("unexpected model call"),
            outline_only=True,
            previous_execution_id=first,
        )
    assert (
        await AuthoringStore(analysis_runtime[2].dsn).get_execution(
            second, missing_ok=True
        )
        is None
    )
