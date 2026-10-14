"""Explicit standalone operator interface; does not call or replace Rust APIs."""
import argparse
import asyncio
from datetime import date, datetime
import json
import sys
from pathlib import Path

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Store, StoreError
from docreader.jobs.settings import Settings
from docreader.workflow.persistence import initialize_checkpoints


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("init")
    for name in ("submit", "revise"):
        command = commands.add_parser(name)
        if name == "revise":
            command.add_argument("run_id")
        command.add_argument("manifest", type=Path)
        command.add_argument("--request-key", required=True)
    for name in ("status", "cancel"):
        command = commands.add_parser(name)
        command.add_argument("run_id")
    command = commands.add_parser("confirm")
    command.add_argument("run_id")
    command.add_argument("--version", required=True, type=int)
    command.add_argument("--digest", required=True)
    command.add_argument("--request-key", required=True)
    commands.add_parser("dispatch")
    return parser.parse_args()


async def run(args):
    settings = Settings.from_env()
    store = Store(settings.dsn)
    artifacts = ArtifactStore(settings.artifact_root)
    if args.command == "init":
        await store.setup()
        await initialize_checkpoints(settings.dsn)
        return {"initialized": True}
    if args.command in {"submit", "revise"}:
        from docreader.tender.evidence import register_manifest

        manifest = json.loads(args.manifest.read_text())
        # Relative original paths are relative to the manifest, not worker cwd.
        for doc in manifest.get("documents", []):
            if doc.get("path") and not Path(doc["path"]).is_absolute():
                doc["path"] = str(args.manifest.resolve().parent / doc["path"])
        registered = await asyncio.to_thread(register_manifest, manifest, artifacts)
        ref = await asyncio.to_thread(artifacts.put_json, registered)
        if args.command == "submit":
            row = await store.submit(ref, args.request_key)
        else:
            row = await store.revise(args.run_id, ref, args.request_key)
        return {"run_id": row["run_id"], "version": row["current_version"],
                "status": row["status"], "issues": registered.get("issues", [])}
    if args.command == "status":
        row = await store.get_run(args.run_id)
        # Return actual domain summary and refs; never print configuration credentials.
        row["results"] = {}
        for item in row["artifacts"]:
            if not item["scope"]:
                row["results"][item["stage"]] = await asyncio.to_thread(artifacts.read_json, item["artifact_ref"])
        return row
    if args.command == "cancel":
        row = await store.cancel(args.run_id)
        return {"run_id": row["run_id"], "status": row["status"]}
    if args.command == "confirm":
        row = await store.confirm(args.run_id, args.version, args.digest, args.request_key)
        return {"command_id": row["command_id"], "status": row["status"]}
    if args.command == "dispatch":
        from docreader.jobs.dispatcher import dispatch_once
        from docreader.jobs.tasks import tender_command

        count = await dispatch_once(store, tender_command.send, redeliver_seconds=settings.redeliver_seconds)
        return {"published": count}


def _json(value):
    if isinstance(value, (datetime, date)):
        return value.isoformat()
    raise TypeError(type(value).__name__)


def main():
    try:
        result = asyncio.run(run(parse_args()))
        print(json.dumps(result, ensure_ascii=False, default=_json))
    except (StoreError, ValueError, OSError) as error:
        # Exception bodies from clients may contain DSNs or request data.
        print(json.dumps({"error": type(error).__name__}), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
