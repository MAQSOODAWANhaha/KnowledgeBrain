import pytest

from docreader.llm import client


@pytest.fixture(autouse=True)
def _clear_rate_limit_cooldown():
    # The 429 cooldown is process-wide by design; tests must not inherit it.
    client._RATE_LIMITED_UNTIL.clear()
    yield
    client._RATE_LIMITED_UNTIL.clear()
