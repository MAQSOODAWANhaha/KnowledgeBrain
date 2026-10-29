"""Synthetic v2 contract responses; never evidence of model semantic accuracy."""
from copy import deepcopy
from uuid import uuid4

from docreader.llm import ModelInfrastructureError

from fixture_model import fixture_request_digest, fixture_source_texts


class AnalysisFixtureModel:
    def __init__(self, store=None, lease=None, *, artifacts=None, fail_document=None, malformed_document=None,
                 scope_ambiguity=False):
        self.store, self.lease, self.artifacts = store, lease, artifacts
        self.fail_document, self.malformed_document = fail_document, malformed_document
        self.scope_ambiguity = scope_ambiguity
        self.calls = []

    def execution_spec(self):
        return {"test_fixture": "analysis-v2"}

    def response(self, task, payload, images=()):
        assert task != "plan", "analysis must end at confirmation"
        if "work_units" in payload:
            from docreader.tender.checks.analysis_batches import member_payload

            return {"schema_version": 2, "shared_findings": [], "work_units": [
                {"work_unit_id": identity, **{key: value for key, value in self.response(
                    task, member_payload(payload, identity), images).items() if key != "schema_version"}}
                for identity in payload["target_work_unit_ids"]]}
        if task == "ocr":
            assert images
            return {"text": "The supplier must provide the illustrated maintenance schedule.",
                    "status": "complete", "issues": []}
        views = payload["source_views"]
        if task == "extract" or payload["operation"] != "locate":
            assert "catalog" not in payload, "only localization receives global navigation metadata"
        else:
            expected = {"section_id", "heading_path"}
            if payload["work_unit_id"].startswith("discovery-"):
                expected.add("range_ids")
            assert all(set(section) == expected
                       for document in payload["catalog"] for section in document["sections"])
        if task == "extract":
            items, dispositions = [], []
            for view in views:
                for source in view["body"]:
                    # This fixture's generated English documents use literal
                    # must statements. Production semantics are model-owned.
                    item_ids = []
                    for text in fixture_source_texts(source):
                        if "must" not in text.lower():
                            continue
                        item_id = f"{payload['work_unit_id']}:i{len(items) + 1}"
                        item_ids.append(item_id)
                        items.append({"item_id": item_id, "kind": "obligation", "text": text,
                                      "evidence": [{"range_id": source["range_id"], "quote": text}]})
                    dispositions.append({"range_id": source["range_id"],
                                         "status": "items" if item_ids else "non_item",
                                         "reason": "Literal fixture must statement." if item_ids else
                                                   "This generated fixture range is a heading or descriptive field.",
                                         "item_ids": item_ids})
                for template in view.get("blank_table_templates", []):
                    dispositions.extend({"range_id": range_id, "status": "non_item",
                                         "reason": "This generated fixture row is an empty response slot.", "item_ids": []}
                                        for rows in template["rows"] for range_id in rows["range_ids"])
            return {"schema_version": 2, "items": items, "dispositions": dispositions,
                    "relations": [], "issues": []}
        if payload["operation"] == "locate":
            return {"schema_version": 2, "candidates": [], "findings": []}
        if payload["operation"] == "associate":
            return {"schema_version": 2, "relations": [], "decisions": [], "findings": []}
        condition = "This form applies only to the local response field."
        reviewed_scope = any(decision["decision"] == "local_only" and decision["rationale"].strip()
                             and any(evidence["quote"] == condition for evidence in decision["evidence"])
                             for decision in payload.get("scope_decisions", []))
        findings = [{"code": "semantic_scope_unresolved", "blocking": True, "range_id": source["range_id"],
                     "detail": "This synthetic form condition requires source-backed human scope review."}
                    for view in views for source in view["body"] if self.scope_ambiguity
                    and any(condition in text for text in fixture_source_texts(source)) and not reviewed_scope]
        return {"schema_version": 2,
                "accepted_decision_ids": [item["decision_id"] for item in payload["decisions"]], "findings": findings}

    async def call(self, task, payload, images=()):
        self.calls.append((task, deepcopy(payload)))
        if self.scope_ambiguity and self.lease and self.lease.domain_revision > 1:
            decisions = payload.get("scope_decisions", [])
            assert decisions and all(decision["rationale"] and decision["evidence"] for decision in decisions)
        documents = {view["document_id"] for view in payload.get("source_views", [])}
        request_id = uuid4().hex
        if self.store:
            prior = await self.store.begin_request(self.lease, request_id, "synthetic-analysis-fixture", task, {
                "test_double": True, "external_request": False,
                "request_digest": fixture_request_digest(task, payload, images),
                "logical_request_digest": fixture_request_digest(task,
                    {key: value for key, value in payload.items() if key != "contract_feedback"}, images),
            })
            if prior["status"] == "succeeded":
                return self.artifacts.read_json(prior["response_ref"])
            await self.store.finish_request(request_id, "sent")
        if task == "extract" and self.fail_document in documents:
            if self.store:
                await self.store.finish_request(request_id, "failed", error={"type": "injected_confirmed_service_rejection"})
            raise ModelInfrastructureError("injected confirmed service rejection")
        result = ({"not_an_extraction": True} if task == "extract" and self.malformed_document in documents
                  else self.response(task, payload, images))
        if self.store:
            await self.store.finish_request(request_id, "succeeded", usage=None,
                                            response_ref=self.artifacts.put_json(result))
        return result
