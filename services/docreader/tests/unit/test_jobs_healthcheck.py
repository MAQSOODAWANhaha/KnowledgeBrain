"""Role identity, bounded failures and credential-free CLI output."""
from pathlib import Path
import re
import subprocess
import sys

import psycopg
import pytest

from docreader.jobs import healthcheck


def process(root, pid, *args):
    directory = root / str(pid)
    directory.mkdir()
    (directory / "cmdline").write_bytes(b"\0".join(value.encode() for value in args) + b"\0")


def test_process_check_requires_the_real_python_role_and_rejects_probe_or_launcher(tmp_path):
    process(tmp_path, 1, "uv", "run", "dramatiq", "docreader.jobs.tasks")
    process(tmp_path, 2, "python", "-m", "docreader.jobs.healthcheck", "worker")
    with pytest.raises(RuntimeError):
        healthcheck._check_process("worker", tmp_path)
    process(tmp_path, 3, "/app/.venv/bin/python", "/app/.venv/bin/dramatiq", "docreader.jobs.tasks", "--processes", "1")
    healthcheck._check_process("worker", tmp_path)
    with pytest.raises(RuntimeError):
        healthcheck._check_process("dispatcher", tmp_path)
    process(tmp_path, 4, "/app/.venv/bin/python3.11", "-m", "docreader.jobs.dispatcher")
    healthcheck._check_process("dispatcher", tmp_path)


def test_cli_failure_reports_only_exception_class(monkeypatch, capsys):
    def fail(_role):
        raise ConnectionError("postgresql://private-user:private-password@private-host/database")
    monkeypatch.setattr(healthcheck, "probe", fail)
    assert healthcheck.main(["worker"]) == 1
    captured = capsys.readouterr()
    assert "ConnectionError" in captured.err and "healthy" in captured.err
    assert "private" not in captured.err and captured.out == ""


def test_cli_global_deadline_bounds_a_stuck_dependency():
    services = Path(__file__).resolve().parents[3]
    code = (f"import sys;sys.path.insert(0,{str(services)!r});import time;"
            "from docreader.jobs import healthcheck;healthcheck.PROBE_TIMEOUT_SECONDS=1;"
            "healthcheck.probe=lambda role:time.sleep(60);raise SystemExit(healthcheck.main(['dispatcher']))")
    result = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, timeout=4)
    assert result.returncode == 1 and "TimeoutError" in result.stderr


def test_redis_probe_is_bounded_and_requires_a_successful_ping(monkeypatch):
    class Client:
        closed = False
        def __enter__(self):
            return self
        def __exit__(self, *args):
            self.closed = True
        def ping(self):
            return False
    client, options = Client(), {}
    def connect(_url, **kwargs):
        options.update(kwargs)
        return client
    monkeypatch.setattr(healthcheck.Redis, "from_url", connect)
    with pytest.raises(RuntimeError):
        healthcheck._check_redis("redis://fixture.invalid")
    assert client.closed and 0 < options["socket_connect_timeout"] <= 2 and 0 < options["socket_timeout"] <= 2


class Database:
    read_only = False
    def __init__(self, missing=None):
        self.queries, self.missing = [], missing
    def __enter__(self):
        return self
    def __exit__(self, *args):
        pass
    def execute(self, query):
        self.queries.append(query)
        if self.missing and f"FROM {self.missing} " in query:
            raise psycopg.errors.UndefinedTable("fixture schema is incomplete")


def test_database_probe_reads_every_unified_baseline_table_in_read_only_mode(monkeypatch):
    database, options = Database(), {}
    def connect(_dsn, **kwargs):
        options.update(kwargs)
        return database
    monkeypatch.setattr(healthcheck.psycopg, "connect", connect)
    healthcheck._check_database("fixture")
    baseline = Path(__file__).resolve().parents[4] / "migrations" / "20261028000003_tender.sql"
    expected = set(re.findall(r"CREATE TABLE (\w+)", baseline.read_text()))
    queried = {re.search(r"FROM (\w+)", query)[1] for query in database.queries if query.startswith("SELECT ")}
    assert expected | {"checkpoints", "checkpoint_blobs", "checkpoint_writes"} <= queried
    assert database.read_only and 0 < options["connect_timeout"] <= 2
    assert all(query.startswith(("SELECT ", "SET LOCAL statement_timeout")) for query in database.queries)


@pytest.mark.parametrize("missing", ["tender_authoring_executions", "tender_authoring_jobs", "tender_outline_revisions", "tender_draft_versions", "checkpoints"])
def test_database_probe_rejects_partial_initialized_schema(monkeypatch, missing):
    monkeypatch.setattr(healthcheck.psycopg, "connect", lambda *args, **kwargs: Database(missing))
    with pytest.raises(psycopg.errors.UndefinedTable):
        healthcheck._check_database("fixture")


def test_checkpoint_write_probe_columns_match_the_installed_official_schema():
    from langgraph.checkpoint.postgres.base import MIGRATIONS

    definition = next(statement for statement in MIGRATIONS
                      if re.search(r"CREATE TABLE IF NOT EXISTS checkpoint_writes\b", statement))
    query = next(query for query in healthcheck._CORE_QUERIES if "FROM checkpoint_writes " in query)
    columns = query.removeprefix("SELECT ").split(" FROM ", 1)[0].split(",")
    for column in columns:
        assert re.search(rf"\b{re.escape(column)}\s+(TEXT|INTEGER|BYTEA|JSONB)\b", definition, re.IGNORECASE), column
