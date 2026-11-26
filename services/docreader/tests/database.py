"""Explicit isolated-test initialization from the production SQL baseline."""
from typing import Any, cast
from pathlib import Path

import psycopg


async def initialize_schema(dsn, sections=("analysis", "authoring")):
    baseline = Path(__file__).resolve().parents[3] / "migrations" / "20261028000003_tender.sql"
    source = baseline.read_text()
    async with await psycopg.AsyncConnection.connect(dsn) as conn:
        for section in sections:
            statement = source.split(f"-- BEGIN {section}\n", 1)[1].split(f"-- END {section}", 1)[0]
            await conn.execute(cast(Any, statement))
        if "web" in sections:
            # Isolated schemas have no platform users table; keep every other constraint of the migration.
            later = baseline.with_name("20261125000001_tender_gap_dispositions.sql").read_text()
            await conn.execute(cast(Any, later.replace(" REFERENCES platform_users(id)", "")))
