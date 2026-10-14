"""Deterministic transport/configuration tests: these are not semantic LLM acceptance."""

import asyncio
import json

import httpx
import pytest

from docreader.llm import (
    ImageInput, ModelConfig, ModelConfigurationError, ModelInfrastructureError,
    ModelSemanticError, OpenAIModel,
)
from docreader.llm.budget import check_context
from docreader.llm.config import completions_url
from docreader.llm.usage import UsageLedger, reported_usage


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
    aliases = {"LLM_BASE_URL": " https://chat.invalid/v1 ", "LLM_MODEL": " gpt-4o ",
               "LLM_API_KEY": "secret", "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS": "8192"}
    assert ModelConfig.from_env(aliases).chat.max_context_tokens == 8192


@pytest.mark.parametrize("context", ["", "0", "-1", "abc", "1.5", "4194305", "１２８"])
def test_context_requires_service_capacity(context):
    with pytest.raises(ModelConfigurationError):
        ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS=context))


@pytest.mark.parametrize("url", ["ftp://chat.invalid", "https://user:secret@chat.invalid/v1",
                                "https://chat.invalid/v1?secret=x", "https://chat.invalid/#secret",
                                "https://chat.invalid:invalid", "https://chat.invalid?"])
def test_invalid_endpoint_does_not_echo_input(url):
    with pytest.raises(ModelConfigurationError) as error:
        ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_BASE_URL=url))
    assert "secret" not in str(error.value)


def test_vlm_validation_is_lazy_and_exact_image_capabilities_apply():
    config = ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_MODEL="gpt-4o-unverified"))
    assert config.chat.model == "gpt-4o-unverified"
    with pytest.raises(ModelConfigurationError, match="confirmed Chat image support"):
        config.for_images()
    assert ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES="true")).for_images().max_context_tokens == 32768
    config = ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES="false"))
    with pytest.raises(ModelConfigurationError):
        config.for_images()
    with pytest.raises(ModelConfigurationError):
        ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES="yes"))


def test_vlm_never_inherits_credentials_for_independent_url():
    values = environment(KNOWLEDGEBRAIN_VLM_BASE_URL="https://vision.invalid/v1",
                         KNOWLEDGEBRAIN_VLM_MODEL="vision-model")
    with pytest.raises(ModelConfigurationError, match="its own API key"):
        ModelConfig.from_env(values).for_images()
    values["KNOWLEDGEBRAIN_VLM_API_KEY"] = "vision-key"
    vision = ModelConfig.from_env(values).for_images()
    assert vision.api_key == "vision-key"
    assert vision.max_context_tokens is None
    values.update(KNOWLEDGEBRAIN_VLM_BASE_URL="https://chat.invalid/v1/", KNOWLEDGEBRAIN_VLM_API_KEY="")
    assert ModelConfig.from_env(values).for_images().api_key == "synthetic-secret"


@pytest.mark.parametrize("url", ["https://chat.invalid/v1/chat/completions", "https://chat.invalid",
                                "https://CHAT.invalid:443/v1/"])
def test_equivalent_endpoint_inherits_context_but_requires_explicit_key(url):
    values = environment(KNOWLEDGEBRAIN_VLM_BASE_URL=url, KNOWLEDGEBRAIN_VLM_MODEL="gpt-4o")
    with pytest.raises(ModelConfigurationError):
        ModelConfig.from_env(values).for_images()
    values["KNOWLEDGEBRAIN_VLM_API_KEY"] = "vision-key"
    vision = ModelConfig.from_env(values).for_images()
    assert vision.max_context_tokens == 32768
    assert vision.api_key == "vision-key"
    assert vision.sdk_base_url == "https://chat.invalid/v1/"
    assert completions_url(url) == "https://chat.invalid/v1/chat/completions"


def test_context_and_total_usage_are_independent_and_images_do_not_count_base64():
    estimate = check_context("system", "中文", 1, 32768)
    assert estimate.text_bytes == 12
    assert estimate.estimated_input_tokens == 16428
    with pytest.raises(ModelSemanticError, match="split"):
        check_context("system", "x" * 1000, 0, 1024)
    ledger = UsageLedger()
    ledger.record("succeeded", {"prompt_tokens": 50000, "completion_tokens": 1000, "total_tokens": 51000})
    ledger.record("uncertain", None)
    assert check_context("system", "small", 0, 128).max_context_tokens == 128
    assert ledger.snapshot()["reported_tokens"]["total_tokens"] == 51000
    assert ledger.snapshot()["missing_usage_requests"] == 1
    assert reported_usage(None) is None


class Audit:
    def __init__(self, fail_sent=False):
        self.events = []
        self.fail_sent = fail_sent

    async def begin_request(self, lease, request_id, model, kind, metadata=None):
        self.events.append({"request_id": request_id, "status": "prepared", "model": model,
                            "kind": kind, "metadata": metadata})

    async def finish_request(self, request_id, status, usage=None, error=None):
        if status == "sent" and self.fail_sent:
            raise ConnectionError("audit unavailable")
        self.events.append({"request_id": request_id, "status": status, "usage": usage, "error": error})


def response(content='{"result":"ok"}', *, finish_reason="stop", refusal=None, usage=True):
    body = {"id": "mock-request", "object": "chat.completion", "created": 0, "model": "gpt-4o",
            "choices": [{"index": 0, "finish_reason": finish_reason,
                         "message": {"role": "assistant", "content": content, "refusal": refusal}}]}
    if usage:
        body["usage"] = {"prompt_tokens": 13, "completion_tokens": 7, "total_tokens": 20,
                         "prompt_tokens_details": {"cached_tokens": 3}}
    return httpx.Response(200, json=body)


def model(handler, audit=None, values=None, **kwargs):
    audit = audit or Audit()
    return OpenAIModel(ModelConfig.from_env(values or environment()), audit, object(),
                       {"extract": "Return JSON evidence.", "ocr": "Return JSON literal text."},
                       transport=httpx.MockTransport(handler), **kwargs), audit


async def test_request_audits_before_send_no_output_caps_and_real_usage_only():
    seen = []
    audit = Audit()

    def handler(request):
        assert [event["status"] for event in audit.events] == ["prepared", "sent"]
        body = json.loads(request.content)
        assert "max_tokens" not in body and "max_completion_tokens" not in body
        assert request.url == "https://chat.invalid/v1/chat/completions"
        seen.append(body)
        return response()

    client, _ = model(handler, audit)
    async with client:
        assert await client.call("extract", {"source": "private document text"}) == {"result": "ok"}
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


@pytest.mark.parametrize("status,exception,recorded", [
    (400, ModelSemanticError, "failed"), (401, ModelConfigurationError, "failed"),
    (403, ModelConfigurationError, "failed"), (404, ModelConfigurationError, "failed"),
    (408, ModelInfrastructureError, "uncertain"), (409, ModelInfrastructureError, "failed"),
    (429, ModelInfrastructureError, "failed"), (500, ModelInfrastructureError, "uncertain"),
    (503, ModelInfrastructureError, "uncertain"),
])
async def test_http_errors_are_classified_without_sdk_retry_or_provider_body(status, exception, recorded):
    calls = []

    def handler(request):
        calls.append(request)
        return httpx.Response(status, json={"error": {"message": "synthetic-secret private document"}})

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


async def test_response_loss_is_uncertain_and_not_zero_usage():
    calls = []

    def handler(request):
        calls.append(request)
        raise httpx.ReadTimeout("private transport message", request=request)

    client, audit = model(handler)
    async with client:
        with pytest.raises(ModelInfrastructureError, match="billing are uncertain"):
            await client.call("extract", {})
    assert len(calls) == 1
    assert audit.events[-1]["status"] == "uncertain"
    assert audit.events[-1]["usage"] is None
    assert client.usage.snapshot()["reported_tokens"] == {}


@pytest.mark.parametrize("content,finish_reason,refusal", [
    ("not-json", "stop", None), ("[]", "stop", None), ('{"a":NaN}', "stop", None),
    ('{"a":1}', "length", None),
    ('{"a":1}', "stop", "policy refusal"),
])
async def test_semantic_failures_keep_billed_usage_and_do_not_retry(content, finish_reason, refusal):
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


@pytest.mark.parametrize("envelope", [None, {}, [], {"choices": "invalid"},
                                      {"choices": [{"message": "bad"}]},
                                      {"choices": [{"message": {"content": {"a": 1}}}]}])
async def test_malformed_provider_envelope_is_audited_as_semantic_failure(envelope):
    client, audit = model(lambda _: httpx.Response(200, json=envelope))
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call("extract", {})
    assert audit.events[-1]["status"] == "failed"


async def test_redirects_cannot_send_document_or_credentials_to_a_new_endpoint():
    seen = []

    def handler(request):
        seen.append(request)
        return httpx.Response(307, headers={"Location": "https://other.invalid/v1/chat/completions"})

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

    values = environment(KNOWLEDGEBRAIN_VLM_BASE_URL="https://vision.invalid/v1",
                         KNOWLEDGEBRAIN_VLM_API_KEY="vision-key", KNOWLEDGEBRAIN_VLM_MODEL="vision")
    client, audit = model(handler, values=values)
    async with client:
        await client.call("ocr", {"unit_id": "image-1"}, images=[ImageInput(b"actual-fixture-bytes", "image/png")])
        with pytest.raises(ModelSemanticError, match="not remote"):
            await client.call("ocr", {}, images=["https://remote.invalid/image.png"])
    assert len(seen) == 1
    assert seen[0].url.host == "vision.invalid"
    assert seen[0].headers["authorization"] == "Bearer vision-key"
    assert audit.events[0]["metadata"]["image_count"] == 1
    assert audit.events[0]["metadata"]["max_context_tokens"] is None


async def test_concurrency_limits_in_flight_requests_and_ids_are_unique():
    active = 0
    maximum = 0

    async def handler(request):
        nonlocal active, maximum
        active += 1
        maximum = max(maximum, active)
        await asyncio.sleep(0.01)
        active -= 1
        return response()

    client, audit = model(handler, concurrency=2)
    async with client:
        await asyncio.gather(*(client.call("extract", {"pack": number}) for number in range(6)))
    assert maximum == 2
    prepared = [event for event in audit.events if event["status"] == "prepared"]
    assert len({event["request_id"] for event in prepared}) == 6
