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
    command.add_argument("--source-version", required=True, type=int)
    command.add_argument("--domain-revision", required=True, type=int)
    command.add_argument("--digest", required=True)
    command.add_argument("--request-key", required=True)
    command = commands.add_parser("retry")
    command.add_argument("run_id")
    command.add_argument("--source-version", required=True, type=int)
    command.add_argument("--domain-revision", required=True, type=int)
    command.add_argument("--failed-command-id", required=True)
    command.add_argument("--reason", required=True)
    command.add_argument("--request-key", required=True)
    command = commands.add_parser("correct")
    command.add_argument("run_id")
    command.add_argument("items", type=Path, help="Analysis v2 JSON containing the complete inline packs list")
    command.add_argument("--source-version", required=True, type=int)
    command.add_argument("--expected-revision", required=True, type=int)
    command.add_argument("--base-items-digest", required=True)
    command.add_argument("--reason", required=True)
    command.add_argument("--request-key", required=True)
    command = commands.add_parser("review-scope")
    command.add_argument("run_id")
    command.add_argument("decisions", type=Path, help="JSON with evidence-backed scope_decisions")
    command.add_argument("--source-version", required=True, type=int)
    command.add_argument("--expected-revision", required=True, type=int)
    command.add_argument("--base-plan-digest", required=True)
    command.add_argument("--reason", required=True)
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
        return {"run_id": row["run_id"], "source_version": row.get("submitted_source_version", row["source_version"]),
                "domain_revision": row.get("submitted_domain_revision", row["domain_revision"]),
                "status": row["status"], "issues": registered.get("issues", [])}
    if args.command == "status":
        from docreader.llm.usage import budget_observation

        row = await store.get_run(args.run_id)
        # Return actual domain summary and refs; never print configuration credentials.
        row["results"] = {}
        for item in row["artifacts"]:
            if not item["scope"]:
                row["results"][item["stage"]] = await asyncio.to_thread(artifacts.read_json, item["artifact_ref"])
        verification = row["results"].get("verify", {})
        if verification.get("schema_version") == 2:
            from docreader.tender.checks.analysis import current_items

            row["results"]["current_items"] = await asyncio.to_thread(current_items, artifacts, verification)
        plans = [value for stage, value in row["results"].items()
                 if stage.startswith("analysis-") and stage.endswith("-plan")]
        if source_plan := row["results"].get("pack-plan"):
            budgets = []
            for pack in source_plan["packs"]:
                descriptor = await asyncio.to_thread(artifacts.read_json, pack["artifact_ref"])
                budgets.append({"budget": descriptor["planning_details"]["budget"]})
            plans.insert(0, {"phase": "extract", "requests": budgets})
        row["budget_observation"] = budget_observation(row["usage"], plans)
        return row
    if args.command == "cancel":
        row = await store.cancel(args.run_id)
        return {"run_id": row["run_id"], "status": row["status"]}
    if args.command == "confirm":
        row = await store.confirm(
            args.run_id, args.source_version, args.digest, args.request_key, domain_revision=args.domain_revision,
        )
        return {"command_id": row["command_id"], "status": row["status"]}
    if args.command == "retry":
        row = await store.retry(
            args.run_id, args.source_version, args.domain_revision, args.failed_command_id,
            args.request_key, args.reason,
        )
        return {"command_id": row["command_id"], "status": row["status"],
                "source_version": row["source_version"], "domain_revision": row["domain_revision"]}
    if args.command == "correct":
        corrected = json.loads(args.items.read_text())
        row = await store.get_run(args.run_id)
        ref = await asyncio.to_thread(
            _correction_ref, artifacts, row, args.source_version, args.expected_revision,
            args.base_items_digest, corrected,
        )
        result = await store.correct(
            args.run_id, args.source_version, args.expected_revision, args.base_items_digest,
            ref, args.request_key, args.reason,
        )
        return {"run_id": args.run_id, "source_version": result["submitted_source_version"],
                "domain_revision": result["submitted_domain_revision"], "status": result["status"]}
    if args.command == "dispatch":
        from docreader.jobs.dispatcher import dispatch_once
        from docreader.jobs.tasks import tender_command

        count = await dispatch_once(store, tender_command.send, redeliver_seconds=settings.redeliver_seconds)
        return {"published": count}
    if args.command == "review-scope":
        row = await store.get_run(args.run_id)
        ref = await asyncio.to_thread(
            _scope_review_ref, artifacts, row, args.source_version, args.expected_revision,
            args.base_plan_digest, json.loads(args.decisions.read_text()),
        )
        result = await store.review_scope(
            args.run_id, args.source_version, args.expected_revision, args.base_plan_digest,
            ref, args.request_key, args.reason,
        )
        return {"run_id": args.run_id, "source_version": result["submitted_source_version"],
                "domain_revision": result["submitted_domain_revision"], "status": result["status"]}


def _correction_ref(artifacts, run, source_version, revision, base_digest, corrected):
    """Rebuild v2 items; all evidence identities come from frozen files."""
    from docreader.tender.requirements import validate_item_correction
    from docreader.tender.evidence import EvidenceRepository
    from docreader.tender.checks.analysis import assign_issues

    if (not isinstance(corrected, dict) or set(corrected) != {"schema_version", "packs"}
            or corrected["schema_version"] != 2 or not isinstance(corrected["packs"], list)):
        raise ValueError("correction must contain schema_version 2 and a complete packs list")
    # Include immutable history so a lost CLI response can replay its request key.
    receipts = [*run["artifacts"], *run["history"]["artifacts"]]
    base = next((item for item in receipts if item["source_version"] == source_version and item["domain_revision"] == revision
                 and item["stage"] == "extract" and not item["scope"]), None)
    frozen = next((item for item in receipts if item["source_version"] == source_version and item["domain_revision"] == revision
                   and item["stage"] == "pack-plan" and not item["scope"]), None)
    if not base or not frozen or base["artifact_ref"]["sha256"] != base_digest:
        raise ValueError("correction baseline does not identify frozen extracted items")
    index = artifacts.read_json(base["artifact_ref"])
    if index.get("schema_version") != 2:
        raise ValueError("legacy extraction is read-only; correction requires analysis v2 items")
    source_index = artifacts.read_json(frozen["artifact_ref"])
    native = artifacts.read_json(source_index["frozen_ref"]) if source_index.get("frozen_ref") else source_index
    global_issues = assign_issues(native.get("issues", []), "source")
    expected = {item["pack_id"] for item in index["packs"]}
    sources = {item["pack_id"]: item["artifact_ref"] for item in source_index["packs"]}
    by_id = {}
    for item in corrected["packs"]:
        if not isinstance(item, dict) or not isinstance(item.get("pack_id"), str) or item["pack_id"] in by_id:
            raise ValueError("correction pack identities must be unique")
        by_id[item["pack_id"]] = item
    if set(by_id) != expected or expected != set(sources):
        raise ValueError("correction must cover exactly every frozen extraction pack")
    output = []
    repository = EvidenceRepository(artifacts)
    # Baseline order makes request replay content addressing independent of JSON list order.
    for entry in index["packs"]:
        pack_id = entry["pack_id"]
        pack = repository.load_pack(sources[pack_id])
        issues = assign_issues([*global_issues, *pack.get("issues", [])], pack_id)
        result = validate_item_correction(pack, {key: value for key, value in by_id[pack_id].items() if key != "pack_id"},
                                          issues=issues)
        result["issues"] = assign_issues(result["issues"], pack_id)
        output.append({"pack_id": pack_id, "artifact_ref": artifacts.put_json(result)})
    return artifacts.put_json({"schema_version": 2, "packs": output, "counts": {"packs": len(output)}})


def _scope_review_ref(artifacts, run, source_version, domain_revision, base_plan_digest, submitted):
    """Validate only the requested scope cases, retaining prior immutable decisions."""
    from docreader.tender.evidence import EvidenceRepository
    from docreader.tender.evidence.scope import validate_scope_decisions

    if not isinstance(submitted, dict) or set(submitted) != {"scope_decisions"}:
        raise ValueError("review must contain scope_decisions")
    receipts = [*run["artifacts"], *run["history"]["artifacts"]]
    frozen = next((item for item in receipts if item["source_version"] == source_version
                   and item["domain_revision"] == 0 and item["stage"] == "freeze" and not item["scope"]), None)
    plan = next((item for item in receipts if item["source_version"] == source_version
                 and item["domain_revision"] == domain_revision and item["stage"] == "pack-plan" and not item["scope"]), None)
    if not frozen or not plan or plan["metadata"].get("plan_digest") != base_plan_digest:
        raise ValueError("scope review baseline does not identify this immutable plan")
    revision = next((item for item in [run["revision"], *run["history"]["revisions"]]
                     if item["source_version"] == source_version and item["domain_revision"] == domain_revision), None)
    if not revision:
        raise ValueError("scope review revision is absent")
    analysis = (revision.get("execution_spec") or {}).get("workflow_protocol", {}).get("schema_version") == 2
    verified = next((item for item in receipts if item["source_version"] == source_version
                     and item["domain_revision"] == domain_revision and item["stage"] == "verify" and not item["scope"]), None)
    review_issues = artifacts.read_json(verified["artifact_ref"])["issues"] if analysis and verified else []
    validated = validate_scope_decisions(
        artifacts.read_json(frozen["artifact_ref"]), artifacts.read_json(plan["artifact_ref"]),
        submitted["scope_decisions"], EvidenceRepository(artifacts), analysis=analysis, review_issues=review_issues,
    )
    existing = (artifacts.read_json(revision["scope_decisions_ref"])["scope_decisions"]
                if revision["scope_decisions_ref"] else [])
    decisions = {item["range_id"]: item for item in existing}
    for item in validated["scope_decisions"]:
        if item["range_id"] in decisions and decisions[item["range_id"]] != item:
            raise ValueError("scope review cannot silently replace a prior decision")
        decisions[item["range_id"]] = item
    return artifacts.put_json({"schema_version": 1, "scope_decisions": [decisions[key] for key in sorted(decisions)]})


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
