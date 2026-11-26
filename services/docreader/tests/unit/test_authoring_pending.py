"""Authoring pending queue projection, status dispositions, and handling metrics (W3.3)."""

from argparse import Namespace
from datetime import datetime, timezone
from types import SimpleNamespace

import pytest

from docreader.tender.authoring_pending import build_pending_queue, gap_identity, pending_queue_summary
from docreader.tender.authoring_report import authoring_report


def _outline():
    return {
        "chapters": [
            {
                "chapter_id": "ch-1",
                "title": "一、资格证明",
                "bookmark": "bm-ch-1",
                "targets": [
                    {"target_id": "t-1", "purpose": "营业执照", "bookmark": "bm-t-1"},
                    {"target_id": "t-2", "purpose": "授权委托书", "bookmark": None},
                ],
                "children": [],
            },
            {
                "chapter_id": "ch-2",
                "title": "二、技术方案",
                "bookmark": "bm-ch-2",
                "targets": [
                    {"target_id": "t-3", "purpose": "运维方案", "bookmark": "bm-t-3"},
                ],
                "children": [],
            },
        ],
    }


def test_pending_queue_sorts_by_response_location_and_disambiguates_duplicates():
    gaps = [
        {"code": "missing_global", "detail": "项目整体缺少联系人。", "chapter_id": None, "target_id": None},
        {"code": "missing_material", "detail": "缺少运维资质证明。", "chapter_id": "ch-2", "target_id": "t-3"},
        {"code": "missing_field", "detail": "签署人留空待补。", "chapter_id": "ch-1", "target_id": "t-2"},
        {"code": "missing_material", "detail": "缺少营业执照原件。", "chapter_id": "ch-1", "target_id": "t-1"},
        {"code": "missing_material", "detail": "缺少营业执照原件。", "chapter_id": "ch-1", "target_id": "t-1"},
    ]
    queue = build_pending_queue(_outline(), gaps, created_at="2026-11-25T10:00:00Z")
    assert [item["target_id"] for item in queue] == ["t-1", "t-1", "t-2", "t-3", None]
    assert queue[0]["gap_id"] != queue[1]["gap_id"]
    assert queue[0]["gap_id"] == gap_identity(gaps[3], 0)
    assert queue[1]["gap_id"] == gap_identity(gaps[4], 1)
    assert queue[0]["bookmark"] == "bm-t-1"
    # Target without own bookmark falls back to chapter bookmark.
    assert queue[2]["bookmark"] == "bm-ch-1"
    assert queue[2]["chapter_title"] == "一、资格证明"
    assert queue[2]["target_purpose"] == "授权委托书"
    assert queue[4]["chapter_title"] is None


def test_pending_queue_tracks_dispositions_and_handling_durations():
    gaps = [
        {"code": "missing_material", "detail": "缺营业执照。", "chapter_id": "ch-1", "target_id": "t-1"},
        {"code": "missing_field", "detail": "缺签署人。", "chapter_id": "ch-1", "target_id": "t-2"},
        {"code": "missing_material", "detail": "缺运维案例。", "chapter_id": "ch-2", "target_id": "t-3"},
        {"code": "missing_global", "detail": "全局待补。", "chapter_id": None, "target_id": None},
    ]
    created = datetime(2026, 11, 25, 10, 0, 0, tzinfo=timezone.utc)
    dispositions = [
        {
            "gap_id": gap_identity(gaps[0]),
            "status": "resolved",
            "note": "已线下确认",
            "handled_at": datetime(2026, 11, 25, 10, 0, 10, tzinfo=timezone.utc),
        },
        {
            "gap_id": gap_identity(gaps[1]),
            "status": "need_material",
            "note": "待补授权书",
            "handled_at": "2026-11-25T10:00:30Z",
        },
        {
            "gap_id": gap_identity(gaps[2]),
            "status": "accepted_gap",
            "note": "接受留空",
            "handled_at": "2026-11-25T10:01:00Z",
        },
        {
            "gap_id": gap_identity(gaps[3]),
            "status": "pending",
            "note": "退回重查",
            "handled_at": "2026-11-25T10:02:00Z",
        },
    ]
    queue = build_pending_queue(_outline(), gaps, dispositions, created_at=created)
    by_id = {item["gap_id"]: item for item in queue}
    assert by_id[gap_identity(gaps[0])]["handling_duration_ms"] == 10000
    assert by_id[gap_identity(gaps[1])]["handling_duration_ms"] == 30000
    assert by_id[gap_identity(gaps[2])]["handling_duration_ms"] == 60000
    # Resetting status back to pending clears handled_at and handling_duration_ms.
    assert by_id[gap_identity(gaps[3])]["handled_at"] is None
    assert by_id[gap_identity(gaps[3])]["handling_duration_ms"] is None

    summary = pending_queue_summary(gaps, dispositions, created_at=created)
    assert summary == {
        "total": 4,
        "pending": 1,
        "handled": 3,
        "by_status": {"pending": 1, "resolved": 1, "need_material": 1, "accepted_gap": 1},
        "handling_duration_ms": {"observations": 3, "avg": 33333, "max": 60000, "p95": 60000},
    }

    with pytest.raises(ValueError):
        build_pending_queue(_outline(), gaps, [{"gap_id": gap_identity(gaps[0]), "status": "bogus"}])


def test_authoring_report_includes_pending_queue_metrics_for_document_executions():
    gaps = [
        {"code": "missing_material", "detail": "缺营业执照。", "chapter_id": "ch-1", "target_id": "t-1"},
        {"code": "missing_field", "detail": "缺签署人。", "chapter_id": "ch-1", "target_id": "t-2"},
    ]
    created = "2026-11-25T10:00:00Z"
    dispositions = [
        {
            "gap_id": gap_identity(gaps[0]),
            "status": "accepted_gap",
            "note": "接受留空",
            "handled_at": "2026-11-25T10:00:45Z",
        },
    ]
    outline_report = authoring_report({"execution_id": "web-outline-1", "status": "succeeded"}, [], _outline())
    assert "pending_queue" not in outline_report

    doc_report = authoring_report(
        {"execution_id": "web-document-1", "status": "succeeded", "updated_at": created},
        [],
        _outline(),
        {"gaps": gaps},
        gap_dispositions=dispositions,
    )
    assert doc_report["pending_queue"] == {
        "total": 2,
        "pending": 1,
        "handled": 1,
        "by_status": {"pending": 1, "resolved": 0, "need_material": 0, "accepted_gap": 1},
        "handling_duration_ms": {"observations": 1, "avg": 45000, "max": 45000, "p95": 45000},
    }


@pytest.mark.asyncio
async def test_cli_authoring_report_includes_persisted_gap_dispositions(tmp_path, monkeypatch):
    from docreader.integrations import authoring_store, cli
    from docreader.integrations.artifacts import ArtifactStore

    artifacts = ArtifactStore(tmp_path)
    outline_ref = artifacts.put_json(_outline())
    gaps = [{"code": "missing_material", "detail": "缺营业执照。", "chapter_id": "ch-1", "target_id": "t-1"}]
    result_ref = artifacts.put_json({"outline_ref": outline_ref, "gaps": gaps})
    execution = {
        "execution_id": "web-document-x",
        "status": "succeeded",
        "created_at": "2026-11-25T10:00:00Z",
        "updated_at": "2026-11-25T10:00:00Z",
        "result_ref": result_ref,
        "requests": [],
        "receipts": [],
    }

    class Store:
        def __init__(self, _dsn):
            pass

        async def get_execution(self, execution_id):
            assert execution_id == "web-document-x"
            return execution

        async def list_gap_dispositions(self, execution_id):
            assert execution_id == "web-document-x"
            return [
                {
                    "gap_id": gap_identity(gaps[0]),
                    "status": "resolved",
                    "note": "已处理",
                    "handled_at": "2026-11-25T10:00:15Z",
                }
            ]

    monkeypatch.setattr(cli.Settings, "from_env", classmethod(lambda cls: SimpleNamespace(dsn="unused", artifact_root=tmp_path)))
    monkeypatch.setattr(authoring_store, "AuthoringStore", Store)
    report = await cli.run(Namespace(command="authoring-report", execution_id="web-document-x"))
    assert report["pending_queue"] == {
        "total": 1,
        "pending": 0,
        "handled": 1,
        "by_status": {"pending": 0, "resolved": 1, "need_material": 0, "accepted_gap": 0},
        "handling_duration_ms": {"observations": 1, "avg": 15000, "max": 15000, "p95": 15000},
    }
