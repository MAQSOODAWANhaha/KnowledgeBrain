"""Fence official PostgreSQL checkpoints in the same transaction as the run lock."""
from contextlib import asynccontextmanager
from typing import cast

from langgraph.checkpoint.postgres.aio import AsyncPostgresSaver
from psycopg import AsyncConnection
from psycopg.rows import dict_row


class FencedPostgresSaver(AsyncPostgresSaver):
    def __init__(self, conn, store, lease):
        super().__init__(conn)
        self.store = store
        self.lease = lease

    @asynccontextmanager
    async def _cursor(self, *, pipeline=False):
        # A short SQL transaction, never held across parsing or a model request.
        # The pinned official saver performs all reads/writes through this cursor.
        conn = cast(AsyncConnection, self.conn)
        async with self.lock, conn.transaction():
            await self.store.assert_fence(self.lease, conn=conn)
            async with conn.cursor(binary=True, row_factory=dict_row) as cur:
                yield cur


async def initialize_checkpoints(dsn):
    async with AsyncPostgresSaver.from_conn_string(dsn) as saver:
        await saver.setup()
