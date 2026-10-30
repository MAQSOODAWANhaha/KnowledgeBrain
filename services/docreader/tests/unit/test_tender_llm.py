"""Deterministic transport/configuration tests: these are not semantic LLM acceptance."""

import asyncio
import hashlib
import json

import httpx
import pytest

from docreader.llm import (
    ImageInput,
    ModelBudgetError,
    ModelConfig,
    ModelConfigurationError,
    ModelInfrastructureError,
    ModelSemanticError,
    OpenAIModel,
)
from docreader.llm.budget import RequestBudget
from docreader.llm.config import completions_url
from docreader.llm.usage import UsageLedger, reported_usage
from docreader.integrations.store import Conflict
from docreader.integrations.artifacts import ArtifactStore


def environment(**extra):
    return {
        "KNOWLEDGEBRAIN_CHAT_BASE_URL": "https://chat.invalid/v1",
        "KNOWLEDGEBRAIN_CHAT_API_KEY": "synthetic-secret",
        "KNOWLEDGEBRAIN_CHAT_MODEL": "gpt-4o",
        "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS": "32768",
        **extra,
    }


def test_aliases_preserve_secret_bytes_and_reject_conflicts_without_values():
    values = environment(KNOWLEDGEBRAIN_CHAT_API_KEY=" synthetic-secret ")
    config = ModelConfig.from_env(values)
    assert config.chat.api_key == " synthetic-secret "
    assert "synthetic-secret" not in repr(config)
    values["LLM_API_KEY"] = "other-secret"
    with pytest.raises(ModelConfigurationError) as error:
        ModelConfig.from_env(values)
    assert "KNOWLEDGEBRAIN_CHAT_API_KEY / LLM_API_KEY" in str(error.value)
    assert "synthetic-secret" not in str(error.value)
    assert "other-secret" not in str(error.value)
    aliases = {
        "LLM_BASE_URL": " https://chat.invalid/v1 ",
        "LLM_MODEL": " gpt-4o ",
        "LLM_API_KEY": "secret",
        "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS": "8192",
    }
    assert ModelConfig.from_env(aliases).chat.context_tokens == 8192


@pytest.mark.parametrize("context", ["", "0", "-1", "abc", "1.5", "4194305", "１２８"])
def test_context_requires_service_capacity(context):
    with pytest.raises(ModelConfigurationError):
        ModelConfig.from_env(
            environment(KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS=context)
        )


@pytest.mark.parametrize(
    "url",
    [
        "ftp://chat.invalid",
        "https://user:secret@chat.invalid/v1",
        "https://chat.invalid/v1?secret=x",
        "https://chat.invalid/#secret",
        "https://chat.invalid:invalid",
        "https://chat.invalid?",
    ],
)
def test_invalid_endpoint_does_not_echo_input(url):
    with pytest.raises(ModelConfigurationError) as error:
        ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_BASE_URL=url))
    assert "secret" not in str(error.value)


@pytest.mark.parametrize("chat_model", ["gpt-4o", "unlisted-chat-model"])
def test_vlm_validation_is_lazy_and_does_not_inherit_chat(chat_model):
    config = ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_MODEL=chat_model))
    assert config.chat.model == chat_model
    with pytest.raises(
        ModelConfigurationError, match="VLM base URL, API key and model"
    ):
        config.for_images()


@pytest.mark.parametrize(
    "url", ["https://vision.invalid/v1", "https://chat.invalid/v1/"]
)
def test_vlm_never_inherits_chat_credentials(url):
    values = environment(
        KNOWLEDGEBRAIN_VLM_BASE_URL=url, KNOWLEDGEBRAIN_VLM_MODEL="vision-model"
    )
    with pytest.raises(
        ModelConfigurationError, match="VLM base URL, API key and model"
    ):
        ModelConfig.from_env(values).for_images()
    values["KNOWLEDGEBRAIN_VLM_API_KEY"] = "vision-key"
    vision = ModelConfig.from_env(values).for_images()
    assert vision.api_key == "vision-key"
    assert vision.context_tokens is None


def test_explicit_vlm_accepts_unlisted_model_even_when_chat_uses_same_model():
    config = ModelConfig.from_env(
        environment(
            KNOWLEDGEBRAIN_CHAT_MODEL="unlisted-vision-model",
            KNOWLEDGEBRAIN_VLM_BASE_URL="https://chat.invalid/v1",
            KNOWLEDGEBRAIN_VLM_API_KEY=" vision-key ",
            KNOWLEDGEBRAIN_VLM_MODEL="unlisted-vision-model",
        )
    )
    assert config.for_images().model == "unlisted-vision-model"
    assert config.for_images().api_key == " vision-key "
    assert config.for_images().context_tokens == 32768


@pytest.mark.parametrize("missing", ["BASE_URL", "API_KEY", "MODEL"])
def test_partial_vlm_configuration_never_falls_back_to_chat(missing):
    values = environment(
        KNOWLEDGEBRAIN_VLM_BASE_URL="https://chat.invalid/v1",
        KNOWLEDGEBRAIN_VLM_API_KEY="vision-key",
        KNOWLEDGEBRAIN_VLM_MODEL="gpt-4o",
    )
    del values[f"KNOWLEDGEBRAIN_VLM_{missing}"]
    config = ModelConfig.from_env(values)
    assert config.chat.model == "gpt-4o"
    with pytest.raises(
        ModelConfigurationError, match="VLM base URL, API key and model"
    ):
        config.for_images()


@pytest.mark.parametrize(
    "url",
    [
        "https://chat.invalid/v1/chat/completions",
        "https://chat.invalid",
        "https://CHAT.invalid:443/v1/",
    ],
)
def test_equivalent_endpoint_inherits_context_but_requires_explicit_key(url):
    values = environment(
        KNOWLEDGEBRAIN_VLM_BASE_URL=url, KNOWLEDGEBRAIN_VLM_MODEL="gpt-4o"
    )
    with pytest.raises(ModelConfigurationError):
        ModelConfig.from_env(values).for_images()
    values["KNOWLEDGEBRAIN_VLM_API_KEY"] = "vision-key"
    vision = ModelConfig.from_env(values).for_images()
    assert vision.context_tokens == 32768
    assert vision.api_key == "vision-key"
    assert vision.sdk_base_url == "https://chat.invalid/v1/"
    assert completions_url(url) == "https://chat.invalid/v1/chat/completions"


def test_context_and_total_usage_are_independent_and_images_do_not_count_base64():
    budget = RequestBudget(context_tokens=65536, prompts={"extract": "Return JSON."})
    estimate = budget.require(
        "extract", {"source": "中文"}, images=[ImageInput(b"fixture", "image/png")]
    )
    assert estimate["image_tokens"] == 16384
    assert (
        estimate["input_tokens"]
        == estimate["text_tokens"]
        + estimate["framing_tokens"]
        + estimate["image_tokens"]
    )
    with pytest.raises(ModelBudgetError, match="context"):
        budget.require("extract", {"source": "x" * 65536})
    ledger = UsageLedger()
    ledger.record(
        "succeeded",
        {"prompt_tokens": 100000, "completion_tokens": 1000, "total_tokens": 101000},
    )
    ledger.record("uncertain", None)
    assert budget.require("extract", {"source": "small"})["context_tokens"] == 65536
    assert ledger.snapshot()["reported_tokens"]["total_tokens"] == 101000
    assert ledger.snapshot()["missing_usage_requests"] == 1
    assert reported_usage(None) is None


class Audit:
    def __init__(self, fail_sent=False):
        self.events = []
        self.fail_sent = fail_sent
        self.spec = None
        self.pin_calls = 0

    async def pin_execution_spec(self, lease, spec):
        self.pin_calls += 1
        if self.spec is not None and self.spec != spec:
            raise Conflict("execution configuration changed for this revision")
        self.spec = json.loads(json.dumps(spec))
        return self.spec

    async def begin_request(self, lease, request_id, model, kind, metadata=None):
        self.events.append(
            {
                "request_id": request_id,
                "status": "prepared",
                "model": model,
                "kind": kind,
                "metadata": metadata,
            }
        )
        return self.events[-1]

    async def finish_request(
        self,
        request_id,
        status,
        usage=None,
        error=None,
        *,
        response_ref=None,
        activity=None,
    ):
        if status == "sent" and self.fail_sent:
            raise ConnectionError("audit unavailable")
        self.events.append(
            {
                "request_id": request_id,
                "status": status,
                "usage": usage,
                "error": error,
                "response_ref": response_ref,
                "activity": activity,
            }
        )


def sse_response(envelopes):
    body = "".join("data: " + json.dumps(envelope) + "\n\n" for envelope in envelopes)
    return httpx.Response(
        200,
        content=body + "data: [DONE]\n\n",
        headers={"content-type": "text/event-stream"},
    )


def response(
    content='{"result":"ok"}', *, finish_reason="stop", refusal=None, usage=True
):
    envelopes = [
        {
            "choices": [
                {
                    "index": 0,
                    "finish_reason": None,
                    "delta": {"content": content, "refusal": refusal},
                }
            ]
        },
        {"choices": [{"index": 0, "finish_reason": finish_reason, "delta": {}}]},
    ]
    if usage:
        envelopes.append(
            {
                "choices": [],
                "usage": {
                    "prompt_tokens": 13,
                    "completion_tokens": 7,
                    "total_tokens": 20,
                    "prompt_tokens_details": {"cached_tokens": 3},
                },
            }
        )
    return sse_response(envelopes)


def model(handler, audit=None, values=None, **kwargs):
    audit = audit or Audit()
    return OpenAIModel(
        ModelConfig.from_env(values or environment()),
        audit,
        object(),
        {"extract": "Return JSON evidence.", "ocr": "Return JSON literal text."},
        transport=httpx.MockTransport(handler),
        **kwargs,
    ), audit


async def test_request_audits_before_send_no_output_caps_and_real_usage_only():
    seen = []
    audit = Audit()

    def handler(request):
        assert audit.pin_calls == 1
        assert audit.spec is not None
        assert [event["status"] for event in audit.events] == ["prepared", "sent"]
        body = json.loads(request.content)
        assert "max_tokens" not in body and "max_completion_tokens" not in body
        assert body["stream"] is True and body["stream_options"] == {
            "include_usage": True
        }
        assert audit.events[0]["metadata"]["wire_bytes"] == len(request.content)
        assert request.url == "https://chat.invalid/v1/chat/completions"
        seen.append(body)
        return response()

    client, _ = model(handler, audit)
    async with client:
        assert await client.call("extract", {"source": "private document text"}) == {
            "result": "ok"
        }
    assert len(seen) == 1
    assert audit.events[-1]["status"] == "succeeded"
    assert audit.events[-1]["usage"]["total_tokens"] == 20
    assert audit.events[-1]["usage"]["prompt_tokens_details"]["cached_tokens"] == 3
    assert "private document text" not in json.dumps(audit.events)
    assert "synthetic-secret" not in json.dumps(audit.events)
    assert client.usage.snapshot()["reported_tokens"]["total_tokens"] == 20


async def test_audit_failure_before_send_does_not_call_provider():
    calls = []
    client, _ = model(lambda request: calls.append(request), Audit(fail_sent=True))
    async with client:
        with pytest.raises(ConnectionError, match="audit unavailable"):
            await client.call("extract", {})
    assert calls == []


@pytest.mark.parametrize(
    "status,exception,recorded",
    [
        (400, ModelSemanticError, "failed"),
        (401, ModelConfigurationError, "failed"),
        (403, ModelConfigurationError, "failed"),
        (404, ModelConfigurationError, "failed"),
        (408, ModelInfrastructureError, "failed"),
        (409, ModelInfrastructureError, "failed"),
        (429, ModelInfrastructureError, "failed"),
        (500, ModelInfrastructureError, "failed"),
        (503, ModelInfrastructureError, "failed"),
    ],
)
async def test_http_errors_are_classified_without_sdk_retry_or_provider_body(
    status, exception, recorded
):
    calls = []

    def handler(request):
        calls.append(request)
        return httpx.Response(
            status, json={"error": {"message": "synthetic-secret private document"}}
        )

    client, audit = model(handler)
    async with client:
        with pytest.raises(exception) as error:
            await client.call("extract", {})
    assert len(calls) == 1
    assert audit.events[-1]["status"] == recorded
    assert "private document" not in str(error.value)
    assert "synthetic-secret" not in json.dumps(audit.events)
    assert issubclass(ModelInfrastructureError, ConnectionError)
    assert not issubclass(ModelSemanticError, ConnectionError)


async def test_no_data_before_idle_deadline_is_a_retryable_failure_with_unknown_usage():
    calls = []

    def handler(request):
        calls.append(request)
        raise httpx.ReadTimeout("private transport message", request=request)

    client, audit = model(handler)
    async with client:
        with pytest.raises(ModelInfrastructureError) as error:
            await client.call("extract", {})
    assert "private transport message" not in str(error.value)
    assert len(calls) == 1
    assert audit.events[-1]["status"] == "failed"
    assert audit.events[-1]["error"]["type"] == "first_data_idle_timeout"
    assert audit.events[-1]["usage"] is None
    assert client.usage.snapshot()["reported_tokens"] == {}


@pytest.mark.parametrize(
    "content,finish_reason,refusal",
    [
        ("not-json", "stop", None),
        ("[]", "stop", None),
        ('{"a":NaN}', "stop", None),
        ('{"a":1}', "length", None),
        ('{"a":1}', "stop", "policy refusal"),
    ],
)
async def test_semantic_failures_keep_billed_usage_and_do_not_retry(
    content, finish_reason, refusal
):
    calls = []

    def handler(request):
        calls.append(request)
        return response(content, finish_reason=finish_reason, refusal=refusal)

    client, audit = model(handler)
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call("extract", {})
    assert len(calls) == 1
    assert audit.events[-1]["status"] == "failed"
    assert audit.events[-1]["usage"]["total_tokens"] == 20


async def test_missing_provider_usage_is_explicitly_missing():
    client, audit = model(lambda _: response(usage=False))
    async with client:
        await client.call("extract", {})
    assert audit.events[-1]["usage"] is None
    assert client.usage.snapshot()["missing_usage_requests"] == 1


@pytest.mark.parametrize(
    "envelope",
    [
        None,
        {},
        [],
        {"choices": "invalid"},
        {"choices": [{"message": "bad"}]},
        {"choices": [{"message": {"content": {"a": 1}}}]},
    ],
)
async def test_malformed_provider_envelope_is_audited_as_semantic_failure(envelope):
    client, audit = model(lambda _: sse_response([envelope]))
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call("extract", {})
    assert audit.events[-1]["status"] == "failed"


async def test_redirects_cannot_send_document_or_credentials_to_a_new_endpoint():
    seen = []

    def handler(request):
        seen.append(request)
        return httpx.Response(
            307, headers={"Location": "https://other.invalid/v1/chat/completions"}
        )

    client, audit = model(handler)
    async with client:
        with pytest.raises(ModelConfigurationError):
            await client.call("extract", {"source": "private document"})
    assert len(seen) == 1
    assert seen[0].url.host == "chat.invalid"
    assert audit.events[-1]["status"] == "failed"


async def test_cancel_after_send_persists_uncertain():
    sent = asyncio.Event()

    async def handler(request):
        sent.set()
        await asyncio.Event().wait()

    client, audit = model(handler)
    async with client:
        task = asyncio.create_task(client.call("extract", {}))
        await sent.wait()
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
    assert audit.events[-1]["status"] == "uncertain"


async def test_inline_image_selects_vlm_and_rejects_remote_urls():
    seen = []

    def handler(request):
        seen.append(request)
        return response()

    values = environment(
        KNOWLEDGEBRAIN_VLM_BASE_URL="https://vision.invalid/v1",
        KNOWLEDGEBRAIN_VLM_API_KEY="vision-key",
        KNOWLEDGEBRAIN_VLM_MODEL="vision",
    )
    client, audit = model(handler, values=values)
    async with client:
        await client.call(
            "ocr",
            {"unit_id": "image-1"},
            images=[ImageInput(b"actual-fixture-bytes", "image/png")],
        )
        with pytest.raises(ModelSemanticError, match="not remote"):
            await client.call("ocr", {}, images=["https://remote.invalid/image.png"])
    assert len(seen) == 1
    assert seen[0].url.host == "vision.invalid"
    assert seen[0].headers["authorization"] == "Bearer vision-key"
    assert audit.events[0]["metadata"]["image_count"] == 1
    assert audit.events[0]["metadata"]["context_tokens"] is None


@pytest.mark.parametrize("concurrency", [None, 2, 3])
async def test_concurrency_limits_in_flight_requests_and_ids_are_unique(concurrency):
    active = 0
    maximum = 0

    async def handler(request):
        nonlocal active, maximum
        active += 1
        maximum = max(maximum, active)
        await asyncio.sleep(0.01)
        active -= 1
        return response()

    client, audit = model(
        handler, **({"concurrency": concurrency} if concurrency is not None else {})
    )
    async with client:
        await asyncio.gather(
            *(client.call("extract", {"pack": number}) for number in range(6))
        )
    assert maximum == (2 if concurrency is None else concurrency)
    prepared = [event for event in audit.events if event["status"] == "prepared"]
    assert len({event["request_id"] for event in prepared}) == 6
    assert audit.pin_calls == 6


def test_execution_spec_is_non_secret_json_with_all_actual_prompts():
    prompts = {"extract": "实际抽取提示", "ocr": "actual OCR prompt"}
    values = environment(
        KNOWLEDGEBRAIN_VLM_BASE_URL="https://vision.invalid/v1",
        KNOWLEDGEBRAIN_VLM_MODEL="vision",
        KNOWLEDGEBRAIN_VLM_API_KEY="vision-secret",
    )
    client = OpenAIModel(
        ModelConfig.from_env(values),
        Audit(),
        object(),
        prompts,
        concurrency=3,
        timeout_seconds=45,
    )
    spec = client.execution_spec()
    assert spec["chat"] == {
        "endpoint": "https://chat.invalid/v1/chat/completions",
        "model": "gpt-4o",
        "context_tokens": 32768,
    }
    assert spec["vlm"] == {
        "endpoint": "https://vision.invalid/v1/chat/completions",
        "model": "vision",
        "context_tokens": None,
    }
    assert spec["prompts"] == {
        task: hashlib.sha256(prompt.encode()).hexdigest()
        for task, prompt in prompts.items()
    }
    assert spec["timeout_seconds"] == 45
    assert spec["concurrency"] == 3
    encoded = json.dumps(spec)
    for private in (
        "synthetic-secret",
        "vision-secret",
        "api_key",
        "actual OCR prompt",
    ):
        assert private not in encoded
    # Both external mutation paths must leave the actual instance snapshot intact.
    prompts["extract"] = "changed caller mapping"
    spec["prompts"]["extract"] = "changed returned mapping"
    assert (
        client.execution_spec()["prompts"]["extract"]
        == hashlib.sha256("实际抽取提示".encode()).hexdigest()
    )


def test_default_execution_spec_snapshots_live_analysis_prompts_only(monkeypatch):
    from docreader.tender import prompts

    contents = {
        task: f"{task} original JSON prompt"
        for task in ("extract", "verify", "plan", "ocr")
    }
    monkeypatch.setattr(prompts, "get_prompt", contents.__getitem__)
    client = OpenAIModel(ModelConfig.from_env(environment()), Audit(), object())
    spec = client.execution_spec()
    assert set(spec["prompts"]) == {"extract", "verify", "ocr"}
    assert set(spec["budget"]["chat"]["prompts"]) == {"extract", "verify", "ocr"}
    contents["plan"] = "changed on disk"
    assert client.execution_spec() == spec
    unchanged = OpenAIModel(ModelConfig.from_env(environment()), Audit(), object())
    assert unchanged.execution_spec() == spec
    contents["extract"] = "changed extract prompt"
    changed = OpenAIModel(ModelConfig.from_env(environment()), Audit(), object())
    assert changed.execution_spec() != spec


async def test_key_rotation_and_equivalent_endpoint_keep_pinned_identity():
    calls = []
    audit = Audit()

    def handler(request):
        calls.append(request)
        return response()

    first, _ = model(handler, audit)
    async with first:
        await first.call("extract", {})
    spec = audit.spec
    second, _ = model(
        handler,
        audit,
        values=environment(
            KNOWLEDGEBRAIN_CHAT_API_KEY="rotated-secret",
            KNOWLEDGEBRAIN_CHAT_BASE_URL="https://CHAT.invalid:443/v1/chat/completions",
        ),
    )
    assert second.execution_spec() == spec
    async with second:
        await second.call("extract", {})
    assert len(calls) == 2
    assert calls[1].headers["authorization"] == "Bearer rotated-secret"
    assert "rotated-secret" not in json.dumps(audit.spec)


@pytest.mark.parametrize(
    "change",
    ["model", "context", "endpoint", "vlm", "prompt", "concurrency", "timeout"],
)
async def test_execution_drift_fails_before_request_audit_or_http(change):
    audit = Audit()
    calls = []

    def handler(request):
        calls.append(request)
        return response()

    first, _ = model(handler, audit)
    async with first:
        await first.call("extract", {})
    before = list(audit.events)
    values = environment()
    prompts = {"extract": "Return JSON evidence.", "ocr": "Return JSON literal text."}
    kwargs = {}
    if change == "model":
        values["KNOWLEDGEBRAIN_CHAT_MODEL"] = "gpt-4o-mini"
    elif change == "context":
        values["KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS"] = "65536"
    elif change == "endpoint":
        values["KNOWLEDGEBRAIN_CHAT_BASE_URL"] = "https://other.invalid/v1"
    elif change == "vlm":
        values.update(
            KNOWLEDGEBRAIN_VLM_BASE_URL="https://vision.invalid/v1",
            KNOWLEDGEBRAIN_VLM_API_KEY="vision-key",
            KNOWLEDGEBRAIN_VLM_MODEL="explicit-vision-model",
        )
    elif change == "prompt":
        # Even a changed task not being called now changes this revision's plan.
        prompts["ocr"] = "changed OCR prompt"
    elif change == "concurrency":
        kwargs["concurrency"] = 3
    else:
        kwargs["timeout_seconds"] = 60
    second = OpenAIModel(
        ModelConfig.from_env(values),
        audit,
        object(),
        prompts,
        transport=httpx.MockTransport(handler),
        **kwargs,
    )
    async with second:
        with pytest.raises(Conflict):
            await second.call("extract", {})
    assert len(calls) == 1
    assert audit.events == before


async def test_snapshot_prompt_is_the_prompt_actually_sent():
    from docreader.llm.request import build_request

    source_prompt = "original JSON prompt"
    prompts = {"extract": source_prompt}
    expected_prompt = build_request("gpt-4o", source_prompt, {}, task="extract")["messages"][0]["content"]
    seen = []
    audit = Audit()

    def handler(request):
        seen.append(json.loads(request.content)["messages"][0]["content"])
        assert (
            audit.spec["prompts"]["extract"]
            == hashlib.sha256(source_prompt.encode()).hexdigest()
        )
        assert seen[-1] == expected_prompt
        return response()

    client = OpenAIModel(
        ModelConfig.from_env(environment()),
        audit,
        object(),
        prompts,
        transport=httpx.MockTransport(handler),
    )
    prompts["extract"] = "caller changed this after construction"
    async with client:
        await client.call("extract", {})
    assert seen == [expected_prompt]


async def test_exact_sent_body_has_same_measurement_as_planner_and_budget_is_pinned():
    seen = []
    client, audit = model(
        lambda request: (seen.append(json.loads(request.content)), response())[1]
    )
    payload = {
        "pack_id": "p1",
        "body": [{"text": "实际正文"}],
        "supporting": [{"text": "全部适用条件"}],
    }
    budget = client.request_budget()
    expected = budget.measure("extract", payload)
    async with client:
        await client.call("extract", payload)
    assert seen == [budget.request("extract", payload)]
    assert budget.measure_request(seen[0]) == expected
    assert audit.spec["budget"]["chat"] == budget.spec()
    for key, value in expected.items():
        assert audit.events[0]["metadata"][key] == value


async def test_complete_request_budget_failure_prevents_audit_and_network():
    calls = []
    client, audit = model(lambda request: calls.append(request))
    async with client:
        with pytest.raises(ModelBudgetError, match="context"):
            await client.call(
                "extract",
                {
                    "body": [{"text": "tiny"}],
                    "supporting": [{"text": "required" * 3000}],
                },
            )
    assert calls == [] and audit.events == []


async def test_budget_estimator_drift_fails_before_network(monkeypatch):
    from docreader.llm.budget import RequestBudget

    calls = []
    audit = Audit()
    client, _ = model(lambda request: (calls.append(request), response())[1], audit)
    async with client:
        await client.call("extract", {})
        monkeypatch.setattr(RequestBudget, "IMAGE_TOKENS", 16385)
        with pytest.raises(Conflict):
            await client.call("extract", {})
    assert len(calls) == 1


class CountedArtifacts(ArtifactStore):
    def __init__(self, root):
        super().__init__(root)
        self.writes = 0

    def put_json(self, value):
        self.writes += 1
        return super().put_json(value)


@pytest.mark.parametrize(
    "content,status",
    [
        ('{"result":"private source quote"}', "succeeded"),
        ("not valid JSON private source quote", "failed"),
    ],
)
async def test_full_response_is_saved_once_and_audit_retains_only_reference(
    tmp_path, content, status
):
    artifacts = CountedArtifacts(tmp_path)
    raw = response(content).text
    client, audit = model(
        lambda _: httpx.Response(
            200, text=raw, headers={"content-type": "text/event-stream"}
        ),
        artifacts=artifacts,
    )
    async with client:
        if status == "succeeded":
            await client.call("extract", {})
        else:
            with pytest.raises(ModelSemanticError):
                await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["status"] == status and artifacts.writes == 1
    assert artifacts.read_json(terminal["response_ref"])["raw_body"] == raw
    assert terminal["usage"]["total_tokens"] == 20
    assert "private source quote" not in json.dumps(audit.events)


@pytest.mark.parametrize(
    "raw",
    [{"choices": "invalid"}, ["malformed envelope"], {"choices": [{"message": "bad"}]}],
)
async def test_invalid_envelope_also_has_one_response_artifact(tmp_path, raw):
    artifacts = CountedArtifacts(tmp_path)
    client, audit = model(lambda _: sse_response([raw]), artifacts=artifacts)
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call("extract", {})
    assert artifacts.writes == 1
    assert (
        artifacts.read_json(audit.events[-1]["response_ref"])["raw_body"]
        == sse_response([raw]).text
    )


async def test_received_http_error_is_saved_and_credentials_are_redacted(tmp_path):
    artifacts = CountedArtifacts(tmp_path)
    body = {
        "error": {
            "message": "credential synthetic-secret rejected",
            "api_key": "unknown-secret",
            "Authorization": "Bearer unknown-secret",
        }
    }
    client, audit = model(lambda _: httpx.Response(401, json=body), artifacts=artifacts)
    async with client:
        with pytest.raises(ModelConfigurationError):
            await client.call("extract", {})
    saved = artifacts.read_json(audit.events[-1]["response_ref"])
    assert artifacts.writes == 1
    assert saved["body"] == {"error": {"message": "credential [REDACTED] rejected"}}


async def test_non_json_response_preserves_body_without_headers_or_credentials(
    tmp_path,
):
    artifacts = CountedArtifacts(tmp_path)
    client, audit = model(
        lambda _: httpx.Response(
            200,
            text="invalid synthetic-secret body",
            headers={"Set-Cookie": "private-cookie"},
        ),
        artifacts=artifacts,
    )
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call("extract", {})
    assert artifacts.writes == 1
    assert (
        artifacts.read_json(audit.events[-1]["response_ref"])["raw_body"]
        == "invalid [REDACTED] body"
    )


async def test_no_response_records_empty_partial_response_once(tmp_path):
    artifacts = CountedArtifacts(tmp_path)

    def handler(request):
        raise httpx.ReadTimeout("response lost", request=request)

    client, audit = model(handler, artifacts=artifacts)
    async with client:
        with pytest.raises(ModelInfrastructureError):
            await client.call("extract", {})
    assert artifacts.writes == 1
    assert audit.events[-1]["status"] == "failed"
    assert artifacts.read_json(audit.events[-1]["response_ref"])["raw_body"] == ""


async def test_response_artifact_write_failure_is_not_a_semantic_model_failure(
    tmp_path,
):
    class BrokenArtifacts:
        def put_json(self, value):
            raise OSError("artifact storage unavailable")

    client, audit = model(lambda _: response(), artifacts=BrokenArtifacts())
    async with client:
        with pytest.raises(OSError, match="artifact storage"):
            await client.call("extract", {})
    assert audit.events[-1]["status"] == "sent"


@pytest.mark.parametrize("status", [200, 500])
async def test_reported_usage_survives_invalid_completion_or_http_error(
    tmp_path, status
):
    artifacts = CountedArtifacts(tmp_path)
    raw = {"choices": "invalid", "usage": {"prompt_tokens": 5, "total_tokens": 5}}
    client, audit = model(
        lambda _: (
            sse_response([raw]) if status == 200 else httpx.Response(status, json=raw)
        ),
        artifacts=artifacts,
    )
    async with client:
        with pytest.raises(
            ModelSemanticError if status == 200 else ModelInfrastructureError
        ):
            await client.call("extract", {})
    assert audit.events[-1]["usage"] == {"prompt_tokens": 5, "total_tokens": 5}
    saved = artifacts.read_json(audit.events[-1]["response_ref"])
    assert (
        saved["raw_body"] == sse_response([raw]).text
        if status == 200
        else saved["body"] == raw
    )
