"""Recorded model-call outcomes and whether bounded recovery may retry them.

The client preserves failed versus uncertain audit status. All call failures
share the work unit's initial attempt and three retries; uncertain retries can
be billed again. Cancellation stops execution. This table does not change the
131072 default window or grant additional queue retries after unit exhaustion.
"""

import asyncio

from docreader.llm.request import (
    ModelConfigurationError, ModelInfrastructureError, ModelSemanticError, UncertainModelResponse,
)

# error_type, audit status, exception raised to recovery, recovery may retry
FAILURES = (
    ("connection_before_send", "failed", ModelInfrastructureError, True),
    ("first_data_idle_timeout", "failed", ModelInfrastructureError, True),
    ("http_408", "failed", ModelInfrastructureError, True),
    ("http_409", "failed", ModelInfrastructureError, True),
    ("http_425", "failed", ModelInfrastructureError, True),
    ("http_429", "failed", ModelInfrastructureError, True),
    ("http_500", "failed", ModelInfrastructureError, True),
    ("http_401", "failed", ModelConfigurationError, True),
    ("http_403", "failed", ModelConfigurationError, True),
    ("http_404", "failed", ModelConfigurationError, True),
    ("http_302", "failed", ModelConfigurationError, True),
    ("http_400", "failed", ModelSemanticError, True),
    ("invalid_stream_content_type", "failed", ModelSemanticError, True),
    ("invalid_json_response", "failed", ModelSemanticError, True),
    ("model_refusal", "failed", ModelSemanticError, True),
    ("tool_call_response", "failed", ModelSemanticError, True),
    ("incomplete_finish", "failed", ModelSemanticError, True),
    ("invalid_stream_envelope", "failed", ModelSemanticError, True),
    ("between_data_idle_timeout", "uncertain", UncertainModelResponse, True),
    ("transport_response_unknown", "uncertain", UncertainModelResponse, True),
    ("stream_missing_done", "uncertain", UncertainModelResponse, True),
    ("provider_stream_error", "uncertain", UncertainModelResponse, True),
    ("invalid_stream_encoding", "uncertain", UncertainModelResponse, True),
    ("cancelled_after_send", "uncertain", asyncio.CancelledError, False),
)


def http_failure(status: int):
    """Audit status is failed for every received HTTP rejection; the exception decides retry."""
    if status in {408, 409, 425, 429} or status >= 500:
        return "failed", ModelInfrastructureError
    if status in {401, 403, 404} or 300 <= status < 400:
        return "failed", ModelConfigurationError
    return "failed", ModelSemanticError
