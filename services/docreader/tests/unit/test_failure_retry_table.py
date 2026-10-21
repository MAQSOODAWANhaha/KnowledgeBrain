"""Audit status preserves uncertainty while every call failure can retry."""

from pathlib import Path

from docreader.jobs.tasks import execution_retry
from docreader.llm.failures import FAILURES, http_failure


SOURCE = Path(__file__).resolve().parents[2] / "llm"
CLIENTS = (SOURCE / "client.py").read_text() + (SOURCE / "streaming.py").read_text()


def test_every_failure_row_matches_the_queue_retry_policy():
    seen = set()
    for error_type, status, exception, retries in FAILURES:
        assert error_type not in seen
        seen.add(error_type)
        assert status in {"failed", "uncertain"}
        assert execution_retry(0, exception()) is retries
        assert execution_retry(3, exception()) is False
        if not error_type.startswith("http_"):
            assert error_type in CLIENTS
        if status == "uncertain":
            assert retries is (error_type != "cancelled_after_send")


def test_received_http_status_uses_the_same_rows():
    for error_type, _status, exception, _retries in FAILURES:
        if not error_type.startswith("http_"):
            continue
        code = int(error_type.removeprefix("http_"))
        audit, failure = http_failure(code)
        assert audit == "failed" and failure is exception
