"""The authoring stage report is a read-only projection of requests and results."""
from docreader.llm.usage import budget_observation
from docreader.tender.authoring_report import authoring_report


def request(task, digest, status="succeeded", completion=10):
    return {"task": task, "status": status, "sent_at": "2026-11-19T00:00:00Z",
            "metadata": {"logical_request_digest": digest, "request_digest": digest + status},
            "usage": {"prompt_tokens": 100, "completion_tokens": completion, "total_tokens": 100 + completion}}


def outline():
    return {"issues": [{"code": "outline_directory_classification_rejected"},
                       {"code": "outline_rule_unassigned"}, {"code": "outline_rule_unassigned"}],
            "chapters": [{"targets": [{"target_id": "t1", "rule_refs": [{"rule": 1}]},
                                      {"target_id": "t1:answer", "rule_refs": []}],
                          "children": [{"targets": [{"target_id": "t2", "rule_refs": []}], "children": []}]}]}


def test_stages_count_logical_and_physical_requests_with_workflow_names():
    requests = [request("directory_plan", "a1"), request("directory_plan", "a2"),
                request("item_placement", "b1", "failed"), request("item_placement", "b1")]
    report = authoring_report({"execution_id": "web-outline-x", "status": "succeeded"}, requests, outline())
    assert report["kind"] == "outline" and report["requests"] == 4
    assert report["stages"]["directory_plan"]["stage"] == "格式章节选择"
    assert report["stages"]["directory_plan"]["logical_requests"] == 2
    b = report["stages"]["item_placement"]
    assert b["stage"] == "事项归属" and b["logical_requests"] == 1 and b["requests"] == 2
    assert b["statuses"] == {"failed": 1, "succeeded": 1}
    assert report["targets"] == {"total": 3, "knowledge_answers": 1, "with_rule_refs": 1}
    assert report["outline_issues"] == {"outline_directory_classification_rejected": 1, "outline_rule_unassigned": 2}
    assert "document_gaps" not in report


def test_document_report_counts_gaps():
    document = {"gaps": [{"code": "knowledge_unavailable"}, {"code": "knowledge_unavailable"}]}
    report = authoring_report({"execution_id": "web-document-x", "status": "succeeded"},
                              [request("authoring_write", "w1")], outline(), document)
    assert report["kind"] == "document"
    assert report["stages"]["authoring_write"]["stage"] == "W 写作 / 改稿"
    assert report["document_gaps"] == {"knowledge_unavailable": 2}


def test_failures_tie_rejections_to_the_sent_request_and_rank_by_tokens():
    from docreader.llm.request import request_digest
    from docreader.tender.authoring_execution import sent_request
    from docreader.tender.authoring_report import failure_table

    endpoint = "https://fixture.invalid/v1/chat/completions"
    base = {"messages": [{"role": "system", "content": "s"},
                         {"role": "user", "content": [{"type": "text", "text": '{"units":[]}'}]}]}
    corrected = {"request": base, "contract_feedback": {"errors": [{"code": "list_type"}], "rejection_count": 1}}
    rejected = request("authoring_review", "r1", completion=40)
    rejected["metadata"]["request_digest"] = request_digest(endpoint, sent_request(corrected))
    # A failed resend of the same complete request shares its digest; only the
    # send that succeeded at transport can have produced the rejected response.
    failed = {**request("authoring_review", "r1", "failed"), "usage": None, "error": {"type": "http_429"}}
    failed["metadata"] = {**failed["metadata"], "request_digest": rejected["metadata"]["request_digest"]}
    uncertain = {**request("outline", "o1", "uncertain", completion=5), "error": {"type": "between_data_idle_timeout"}}
    rejections = [{"task": "authoring_review", "errors": [{"code": "list_type"}, {"code": "other"}], "payload": corrected},
                  {"task": "authoring_review", "errors": [{"code": "list_type"}], "payload": {"request": base}}]
    table = failure_table([rejected, failed, uncertain, request("outline", "o1")], rejections, endpoint)
    assert [(row["task"], row["kind"], row["code"], row["count"], row["reported_tokens"], row["unknown_usage"])
            for row in table] == [
        ("authoring_review", "contract", "list_type", 2, 140, 1),
        ("outline", "send", "between_data_idle_timeout", 1, 105, 0),
        ("authoring_review", "send", "http_429", 1, 0, 1)]
    assert table[0]["stage"] == "正文复核 / 再核"
    assert sum(row["count"] for row in table if row["kind"] == "contract") == len(rejections)


def test_analysis_planning_does_not_wait_for_retired_phases():
    plans = [{"phase": phase, "requests": [{"budget": {"input_tokens": 1, "output_reserve": 1, "fits": True}}]}
             for phase in ("extract", "review")]
    planning = budget_observation([], plans)["planning"]
    assert planning["unplanned_phases"] == [] and planning["total_planned_requests"] == 2


async def test_cli_reads_document_and_its_outline(tmp_path, monkeypatch):
    from argparse import Namespace
    from types import SimpleNamespace

    from docreader.integrations import authoring_store, cli
    from docreader.integrations.artifacts import ArtifactStore

    artifacts = ArtifactStore(tmp_path)
    outline_ref = artifacts.put_json(outline())
    result_ref = artifacts.put_json({"outline_ref": outline_ref, "gaps": [{"code": "knowledge_unavailable"}]})
    execution = {"execution_id": "web-document-x", "status": "succeeded", "result_ref": result_ref,
                 "requests": [request("authoring_write", "w1")], "receipts": []}

    class Store:
        def __init__(self, dsn):
            pass

        async def get_execution(self, execution_id):
            assert execution_id == "web-document-x"
            return execution

    monkeypatch.setattr(cli.Settings, "from_env", classmethod(
        lambda cls: SimpleNamespace(dsn="unused", artifact_root=tmp_path)))
    monkeypatch.setattr(authoring_store, "AuthoringStore", Store)
    report = await cli.run(Namespace(command="authoring-report", execution_id="web-document-x"))
    assert isinstance(report, dict)
    assert report["kind"] == "document" and report["document_gaps"] == {"knowledge_unavailable": 1}
    targets = report["targets"]
    assert isinstance(targets, dict) and targets["total"] == 3
