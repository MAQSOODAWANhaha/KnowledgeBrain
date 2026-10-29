"""A user-selected response must retain the predecessor's actual outline."""
from psycopg.types.json import Jsonb
import pytest

from docreader.integrations.web import WebError
from docreader.tests.workflow.test_web_bridge import (
    completed_job, configured, payload, runtime as runtime, upload,
)


async def predecessor(runtime):
    await upload(runtime)
    execution = await completed_job(runtime)
    result = await runtime.bridge.result(runtime.project, execution)
    outline = runtime.bridge.artifacts.read_json(result["outline_ref"])
    outline["chapters"] = [{"chapter_id": "original", "title": "原稿实施方案", "children": [], "targets": [{
        "target_id": "original:target", "kind": "narrative", "purpose": "实际原要求", "rule_refs": [], "source_refs": []}]}]
    result["outline_ref"] = runtime.bridge.artifacts.put_json(outline)
    reference = runtime.bridge.artifacts.put_json(result)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_authoring_executions SET result_ref=%s WHERE execution_id=%s",
                           (Jsonb(reference), execution))
    return execution, result, outline


async def test_targeted_revision_pins_original_outline_and_snapshot(runtime, monkeypatch):
    configured(monkeypatch)
    execution, result, outline = await predecessor(runtime)
    response = await runtime.bridge.start_authoring(payload(runtime, request_key="targeted",
        previous_execution_id=execution, review_request={"target_ids": ["original:target"], "reason": "补充实施步骤"}))
    job = await runtime.bridge.job(runtime.project, response["authoring"]["execution_id"])
    assert job["payload"]["outline_ref"] == result["outline_ref"]
    assert job["payload"]["identity"] == {key: outline["authoring_input"][key]
                                           for key in ("run_id", "source_version", "domain_revision")}
    assert (await runtime.bridge.start_authoring(payload(runtime, request_key="targeted",
        previous_execution_id=execution, review_request={"target_ids": ["original:target"], "reason": "补充实施步骤"}))) == response


async def test_targeted_revision_rejects_foreign_target_before_queueing(runtime, monkeypatch):
    configured(monkeypatch)
    execution, _, _ = await predecessor(runtime)
    with pytest.raises(WebError, match="响应位置"):
        await runtime.bridge.start_authoring(payload(runtime, request_key="invalid-target",
            previous_execution_id=execution, review_request={"target_ids": ["foreign:target"], "reason": "核对"}))


async def test_targeted_revision_requires_a_successful_predecessor(runtime, monkeypatch):
    configured(monkeypatch)
    await upload(runtime)
    pending = await runtime.bridge.start_authoring(payload(runtime, request_key="unfinished"))
    with pytest.raises(WebError, match="完整编制产物"):
        await runtime.bridge.start_authoring(payload(runtime, request_key="bad-base",
            previous_execution_id=pending["authoring"]["execution_id"],
            review_request={"target_ids": ["original:target"], "reason": "核对"}))
