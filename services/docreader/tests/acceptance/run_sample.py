"""Save reviewable synthetic phase-one outputs, never call an external model."""
import argparse
import asyncio
import json
import os
from pathlib import Path
from uuid import uuid4

from fixture_model import FixtureModel
from test_phase1 import mixed_manifest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Store
from docreader.jobs.settings import Settings
from docreader.tender.evidence import register_manifest
from docreader.workflow.nodes import TenderNodes
from docreader.workflow.persistence import initialize_checkpoints
from docreader.workflow.runner import execute_command


async def sample(root, evidence):
    root.mkdir(parents=True, exist_ok=True)
    evidence.mkdir(parents=True, exist_ok=True)
    settings = Settings(os.environ["TENDER_TEST_DATABASE_URL"], "redis://127.0.0.1:16379/1", root / "artifacts")
    store, artifacts = Store(settings.dsn), ArtifactStore(settings.artifact_root)
    await store.setup()
    await initialize_checkpoints(settings.dsn)
    manifest = await asyncio.to_thread(register_manifest, mixed_manifest(root), artifacts)
    run = await store.submit(artifacts.put_json(manifest), uuid4().hex)

    def factory(s, a, lease):
        return TenderNodes(s, a, lease, FixtureModel(s, lease))

    async def execute():
        cmd = (await store.get_run(run["run_id"]))["commands"][-1]
        await execute_command(run["run_id"], cmd["command_id"], 1, settings, factory)
        return await store.get_run(run["run_id"])

    paused = await execute()
    if paused["status"] != "waiting-confirmation":
        raise RuntimeError("synthetic sample did not pass its verification gate")
    await store.confirm(run["run_id"], 1, paused["checklist_digest"], "synthetic-review")
    completed = await execute()
    assert completed["status"] == "response-plan-ready"
    results = {}
    for receipt in completed["artifacts"]:
        if not receipt["scope"]:
            results[receipt["stage"]] = artifacts.read_json(receipt["artifact_ref"])
    report = {
        "evidence_kind": "synthetic documents + real native parsers/PostgreSQL + deterministic test model",
        "external_model_requests": 0, "paid_model_usage": None,
        "run_id": run["run_id"], "status": completed["status"], "version": completed["current_version"],
        "checklist_digest": completed["checklist_digest"],
        "source_stats": results["freeze"]["stats"], "verification_counts": results["verify"]["counts"],
        "request_counts": {task: sum(request["kind"] == task for request in completed["usage"])
                           for task in {request["kind"] for request in completed["usage"]}},
        "stage_receipts": completed["artifacts"], "commands": completed["commands"],
        "remaining": ["real original documents + real model semantic acceptance", "future Rust integration"],
    }
    for name, data in {"sample-run": report, "sample-plan": results["plan"],
                       "sample-verification": results["verify"], "sample-sources": results["freeze"]}.items():
        (evidence / f"{name}.json").write_text(json.dumps(data, ensure_ascii=False, indent=2, default=str) + "\n")
    print(json.dumps({"run_id": run["run_id"], "status": completed["status"],
                      "counts": report["verification_counts"], "requests": report["request_counts"]}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    asyncio.run(sample(args.root.resolve(), args.evidence.resolve()))
