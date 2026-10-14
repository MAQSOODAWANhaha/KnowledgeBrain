"""Database command claims remain recoverable when Redis publish or workers fail."""
import asyncio
import logging

from docreader.integrations.store import Store
from docreader.jobs.settings import Settings

logger = logging.getLogger(__name__)


async def dispatch_once(store, send, *, redeliver_seconds=60):
    commands = await store.claim_commands(redeliver_seconds=redeliver_seconds)
    published = 0
    for cmd in commands:
        try:
            await asyncio.to_thread(send, cmd["run_id"], cmd["command_id"], cmd["version"])
        except Exception:
            # Do not log arbitrary broker URLs/passwords contained in exception text.
            await store.publish_failed(cmd["command_id"], cmd["claim_token"])
            logger.warning("Redis publish failed for command %s", cmd["command_id"])
        else:
            await store.mark_published(cmd["command_id"], cmd["claim_token"])
            published += 1
    return published


async def dispatch_forever():
    from docreader.jobs.tasks import tender_command

    settings = Settings.from_env()
    store = Store(settings.dsn)
    while True:
        await dispatch_once(store, tender_command.send, redeliver_seconds=settings.redeliver_seconds)
        await asyncio.sleep(2)


if __name__ == "__main__":
    asyncio.run(dispatch_forever())
