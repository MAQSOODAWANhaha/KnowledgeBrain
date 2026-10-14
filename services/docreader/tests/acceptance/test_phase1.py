"""Real parsers + real PostgreSQL + deterministic model, separate from LLM acceptance."""
import asyncio
import os
from pathlib import Path
import sys
from uuid import uuid4

from docx import Document
from openpyxl import Workbook
from openpyxl.drawing.image import Image as SheetImage
from PIL import Image, ImageDraw
from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import InvalidConfirmation, Store
from docreader.jobs.settings import Settings
from docreader.llm import ModelInfrastructureError
from docreader.tender.evidence import register_manifest
from docreader.workflow.nodes import TenderNodes
from docreader.workflow.persistence import initialize_checkpoints
from docreader.workflow.runner import execute_command

sys.path.insert(0, str(Path(__file__).parent))
from fixture_model import FixtureModel

pytestmark = pytest.mark.integration


def mixed_manifest(root):
    document = Document()
    document.add_heading("Delivery obligations", level=1)
    document.add_paragraph("The supplier must provide a signed delivery commitment.")
    document.save(root / "main.docx")
    image = Image.new("RGB", (240, 80), "white")
    ImageDraw.Draw(image).text((10, 20), "Maintenance schedule", fill="black")
    image.save(root / "schedule.png")
    book = Workbook()
    first = book.active
    first.title = "Delivery"
    first.append(["Requirement", "Response"])
    first.append(["The supplier must name a delivery coordinator.", None])
    first.append([None, None])
    first.append(["Descriptive note", None])
    first.add_image(SheetImage(str(root / "schedule.png")), "D2")
    second = book.create_sheet("Service")
    second.append(["Requirement", "Response"])
    second.append(["The supplier must provide a service contact.", None])
    book.save(root / "annex.xlsx")
    writer = PdfWriter()
    page = writer.add_blank_page(width=600, height=800)
    font = DictionaryObject({NameObject("/Type"): NameObject("/Font"),
                             NameObject("/Subtype"): NameObject("/Type1"),
                             NameObject("/BaseFont"): NameObject("/Helvetica")})
    page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({NameObject("/F1"): font})})
    stream = DecodedStreamObject()
    stream.set_data(b"BT /F1 11 Tf 40 740 Td (The supplier must provide a written quality assurance statement.) Tj ET")
    page[NameObject("/Contents")] = writer._add_object(stream)
    writer.write(root / "quality.pdf")
    return {"documents": [{"document_id": document_id, "role": role, "version": "one", "path": str(root / name)}
                          for document_id, role, name in [("main", "main", "main.docx"),
                                                         ("annex", "attachment", "annex.xlsx"),
                                                         ("quality", "attachment", "quality.pdf"),
                                                         ("image", "attachment", "schedule.png")]],
            "expected_document_ids": ["main", "annex", "quality", "image"]}


@pytest.fixture
async def runtime(tmp_path):
    dsn = os.environ.get("TENDER_TEST_DATABASE_URL")
    if not dsn:
        pytest.skip("set TENDER_TEST_DATABASE_URL for real PostgreSQL acceptance")
    settings = Settings(dsn, os.environ.get("TENDER_TEST_REDIS_URL", "redis://127.0.0.1:16379/1"), tmp_path / "artifacts")
    store, artifacts = Store(dsn), ArtifactStore(settings.artifact_root)
    await store.setup()
    await initialize_checkpoints(dsn)
    registered = await asyncio.to_thread(register_manifest, mixed_manifest(tmp_path), artifacts)
    run = await store.submit(artifacts.put_json(registered), uuid4().hex)
    return store, artifacts, settings, run


async def execute(store, settings, run, **model_options):
    cmd = (await store.get_run(run["run_id"]))["commands"][-1]
    def factory(s, a, lease):
        return TenderNodes(s, a, lease, FixtureModel(s, lease, **model_options))
    await execute_command(run["run_id"], cmd["command_id"], cmd["version"], settings, factory)
    return await store.get_run(run["run_id"])


def output(artifacts, run, stage):
    return artifacts.read_json(next(item["artifact_ref"] for item in run["artifacts"]
                                    if item["stage"] == stage and not item["scope"]))


async def test_mixed_formats_confirm_to_concrete_response_plan(runtime):
    store, artifacts, settings, run = runtime
    row = await execute(store, settings, run)
    checked = output(artifacts, row, "verify")
    assert row["status"] == "waiting-confirmation", checked["issues"]
    frozen = output(artifacts, row, "freeze")
    assert frozen["stats"]["document_count"] == 4
    assert frozen["stats"]["image_count"] == 2  # Excel embedded + standalone; no text PDF render.
    assert frozen["range_count"] == checked["counts"]["ranges"]
    assert checked["counts"]["requirements"] >= 5
    assert all(row["usage"][i]["metadata"]["test_double"] for i in range(len(row["usage"])))
    assert all(request["usage"] is None for request in row["usage"])  # Never fabricate paid usage.
    await store.confirm(run["run_id"], 1, row["checklist_digest"], "first-confirm")
    completed = await execute(store, settings, run)
    assert completed["status"] == "response-plan-ready"
    plan = output(artifacts, completed, "plan")
    assert len(plan["items"]) == checked["counts"]["applicable_requirements"]
    assert all(item["action"] and item["material_needs"] for item in plan["items"])
    assert plan["checklist_digest"] == completed["checklist_digest"]


async def test_partial_pack_infrastructure_failure_reuses_success_and_frozen_sources(runtime):
    store, artifacts, settings, run = runtime
    with pytest.raises(ModelInfrastructureError):
        await execute(store, settings, run, fail_document="annex")
    failed = await store.get_run(run["run_id"])
    assert failed["status"] == "pending"
    saved = {(a["stage"], a["scope"]): a for a in failed["artifacts"]}
    assert any(stage == "extract-pack" for stage, _ in saved)
    assert ("freeze", "") in saved
    recovered = await execute(store, settings, run)
    assert recovered["status"] == "waiting-confirmation"
    for item in recovered["artifacts"]:
        key = (item["stage"], item["scope"])
        if key in saved:
            assert item["epoch"] == saved[key]["epoch"]
            assert item["artifact_ref"] == saved[key]["artifact_ref"]
    successful_initial = [r for r in failed["usage"] if r["status"] == "succeeded"]
    assert len([r for r in recovered["usage"] if r["kind"] == "ocr"]) == 2
    assert successful_initial


async def test_semantic_failure_is_unresolved_not_global_success_and_revision_invalidates(runtime):
    store, artifacts, settings, run = runtime
    row = await execute(store, settings, run, malformed_document="annex")
    assert row["status"] == "blocked"
    checked = output(artifacts, row, "verify")
    assert checked["requirements"]  # independent successful documents retained
    assert checked["counts"]["blocking_issues"] > 0
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run["run_id"], 1, row["checklist_digest"], "cannot-force")
    revised = await store.revise(run["run_id"], row["manifest_ref"], "reviewed-source-version")
    assert revised["current_version"] == 2
    recovered = await execute(store, settings, run)
    assert recovered["status"] == "waiting-confirmation"
    assert recovered["checklist_digest"] != row["checklist_digest"]
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run["run_id"], 1, row["checklist_digest"], "expired-version")
