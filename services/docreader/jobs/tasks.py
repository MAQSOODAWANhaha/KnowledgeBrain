import dramatiq

from docreader.integrations.store import Cancelled, StaleExecution
from docreader.jobs.broker import broker
from docreader.jobs.settings import Settings
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.workflow.runner import execute_command


def execution_retry(retries, exception):
    return retries < 3 and isinstance(exception, Exception) and not isinstance(
        exception, (Cancelled, StaleExecution, ModelWorkUnitExhausted)
    )


@dramatiq.actor(broker=broker, queue_name="tender", max_retries=3,
                retry_when=execution_retry, min_backoff=1000, max_backoff=30000,
                time_limit=float("inf"))
async def tender_command(run_id: str, command_id: str, source_version: int):
    from docreader.workflow.nodes import TenderNodes

    await execute_command(run_id, command_id, source_version, Settings.from_env(), TenderNodes)


@dramatiq.actor(broker=broker, queue_name="tender", max_retries=0, time_limit=float("inf"))
async def tender_authoring(execution_id: str):
    from docreader.integrations.web import execute_authoring_job

    await execute_authoring_job(execution_id, Settings.from_env())
