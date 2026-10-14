"""Database command claims remain recoverable when Redis publish or workers fail."""
import asyncio
import logging
import psycopg
from dramatiq.errors import ConnectionClosed
from redis.exceptions import ConnectionError as RedisConnectionError, TimeoutError as RedisTimeoutError

from docreader.integrations.store import Store
from docreader.jobs.settings import Settings

logger = logging.getLogger(__name__)


async def dispatch_once(store, send, *, redeliver_seconds=60):
    commands = await store.claim_commands(redeliver_seconds=redeliver_seconds)
    published = 0
    for cmd in commands:
        try:
            await asyncio.to_thread(send, cmd["run_id"], cmd["command_id"], cmd["source_version"])
        except Exception as error:
            # Do not log arbitrary broker URLs/passwords contained in exception text.
            retryable = isinstance(error, (ConnectionClosed, RedisConnectionError, RedisTimeoutError,
                                           ConnectionError, TimeoutError))
            await store.publish_failed(cmd["command_id"], cmd["claim_token"], type(error).__name__, retryable=retryable)
            logger.warning("Command publish failed (%s) for command %s", type(error).__name__, cmd["command_id"])
        else:
            await store.mark_published(cmd["command_id"], cmd["claim_token"])
            published += 1
    return published


async def dispatch_forever():
    from docreader.jobs.tasks import tender_command

    settings = Settings.from_env()
    store = Store(settings.dsn)
    delay = 2
    while True:
        try:
            await dispatch_once(store, tender_command.send, redeliver_seconds=settings.redeliver_seconds)
            delay = 2
        except (psycopg.OperationalError, psycopg.InterfaceError, ConnectionError, TimeoutError) as error:
            logger.warning("Dispatcher infrastructure unavailable (%s)", type(error).__name__)
            delay = min(delay * 2, 30)
        await asyncio.sleep(delay)


if __name__ == "__main__":
    asyncio.run(dispatch_forever())
