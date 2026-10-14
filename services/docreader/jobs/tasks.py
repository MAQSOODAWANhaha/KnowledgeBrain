import dramatiq
import psycopg

from docreader.jobs.broker import broker
from docreader.jobs.settings import Settings
from docreader.workflow.runner import execute_command


def infrastructure_retry(retries, exception):
    return retries < 4 and isinstance(
        exception, (psycopg.OperationalError, psycopg.InterfaceError, ConnectionError, TimeoutError)
    )


@dramatiq.actor(broker=broker, queue_name="tender", max_retries=4,
                retry_when=infrastructure_retry, min_backoff=1000, max_backoff=30000,
                time_limit=float("inf"))
async def tender_command(run_id: str, command_id: str, version: int):
    from docreader.workflow.nodes import TenderNodes

    await execute_command(run_id, command_id, version, Settings.from_env(), TenderNodes)
