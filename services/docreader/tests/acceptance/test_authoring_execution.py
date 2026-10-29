"""Shared synthetic HTTP responses for authoring execution tests."""

import json

import httpx

from docreader.llm.config import ModelConfig, ModelEndpoint


def config():
    return ModelConfig(ModelEndpoint("http://fixture.invalid/v1", "fixture-key", "synthetic-outline", 524288), None)


def response(value):
    events = [
        {"id": "synthetic-outline-response", "choices": [
            {"index": 0, "delta": {"content": json.dumps(value, ensure_ascii=False)}, "finish_reason": None}]},
        {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
        {"choices": [], "usage": {"prompt_tokens": 113, "completion_tokens": 57, "total_tokens": 170}},
    ]
    content = "".join("data: " + json.dumps(event) + "\n\n" for event in events) + "data: [DONE]\n\n"
    return httpx.Response(200, content=content, headers={"content-type": "text/event-stream"})
