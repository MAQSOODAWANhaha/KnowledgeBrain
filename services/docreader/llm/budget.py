"""Per-request admission estimates, separate from cumulative reported usage."""

from dataclasses import asdict, dataclass

from docreader.llm.request import ModelSemanticError


@dataclass(frozen=True)
class ContextEstimate:
    text_bytes: int
    image_count: int
    estimated_input_tokens: int
    max_context_tokens: int | None
    method: str = "utf8_bytes_plus_16384_per_image"

    def metadata(self) -> dict:
        return asdict(self)


def check_context(system_prompt: str, payload_json: str, image_count: int,
                  max_context_tokens: int | None) -> ContextEstimate:
    """Conservative text-byte and image reserve estimate, never provider usage.

    Image base64 is not text-token input. The existing internal 16,384/image
    reserve is an estimate for admission, not a claim about a model tokenizer.
    An independent VLM has no inherited Chat capacity. No output cap is sent and
    a correction uses this exact same capacity, never twice the service limit.
    """
    text_bytes = len(system_prompt.encode("utf-8")) + len(payload_json.encode("utf-8"))
    estimate = ContextEstimate(text_bytes, image_count, text_bytes + 32 + image_count * 16_384,
                               max_context_tokens)
    if max_context_tokens is not None and estimate.estimated_input_tokens >= max_context_tokens:
        raise ModelSemanticError("estimated request input does not fit the configured model context; split the source package")
    return estimate
