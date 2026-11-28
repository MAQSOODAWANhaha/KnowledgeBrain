"""Explicit isolated-test initialization from the production SQL baseline."""
import re
from typing import Any, cast
from pathlib import Path

import psycopg

_MIGRATIONS = Path(__file__).resolve().parents[3] / "migrations"


def _gap_dispositions(*, web: bool) -> str:
    """Isolated schemas have no platform users table; without the web section no projects or jobs either."""
    statement = (_MIGRATIONS / "20261125000001_tender_gap_dispositions.sql").read_text()
    statement = statement.replace(" REFERENCES platform_users(id)", "")
    if not web:
        statement = re.sub(r"ALTER TABLE tender_authoring_jobs[^;]*;", "", statement)
        statement = statement.replace(" REFERENCES tender_projects(id)", "")
        statement = re.sub(r",\s*CONSTRAINT tender_authoring_gap_dispositions_job_fk[^;]*?"
                           r"REFERENCES tender_authoring_jobs\(execution_id, project_id\)", "", statement)
    return statement


async def initialize_schema(dsn, sections=("analysis", "authoring")):
    source = (_MIGRATIONS / "20261028000003_tender.sql").read_text()
    async with await psycopg.AsyncConnection.connect(dsn) as conn:
        for section in sections:
            statement = source.split(f"-- BEGIN {section}\n", 1)[1].split(f"-- END {section}", 1)[0]
            await conn.execute(cast(Any, statement))
        exists = await (await conn.execute("SELECT to_regclass('tender_authoring_gap_dispositions')")).fetchone()
        if "web" in sections and exists is not None and exists[0] is None:
            await conn.execute(cast(Any, _gap_dispositions(web=True)))
        if "web" in sections:
            settings = (_MIGRATIONS / "20261127000001_tender_source_settings.sql").read_text()
            settings = settings.split("-- BEGIN web_source_settings\n", 1)[1].split("-- END web_source_settings", 1)[0]
            await conn.execute(cast(Any, settings))


async def initialize_gap_dispositions(dsn):
    """For runtimes without the web section whose authoring reports still read gap dispositions."""
    async with await psycopg.AsyncConnection.connect(dsn) as conn:
        await conn.execute(cast(Any, _gap_dispositions(web=False)))
