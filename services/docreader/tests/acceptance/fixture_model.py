"""Deterministic contract double. It never sends documents or reports paid usage."""
import hashlib
import json
from uuid import uuid4

from docreader.llm import ModelInfrastructureError


def fixture_request_digest(task, payload, images=()):
    image_hashes = [hashlib.sha256(image.data if hasattr(image, "data") else image.encode()).hexdigest()
                    for image in images]
    request = {"model": "deterministic-contract-fixture", "task": task,
               "payload": payload, "image_hashes": image_hashes}
    return hashlib.sha256(json.dumps(request, sort_keys=True, ensure_ascii=False,
                                     separators=(",", ":")).encode()).hexdigest()


def fixture_source_texts(source):
    # Preserve sent prose; never join separate cells into an invented quotation.
    return ([source["text"]] if "text" in source
            else [cell["text"] for cell in source.get("cells", [])])


class FixtureModel:
    def __init__(self, store=None, lease=None, *, fail_document=None, malformed_document=None):
        self.store, self.lease = store, lease
        self.fail_document, self.malformed_document = fail_document, malformed_document
        self.calls = []

    async def call(self, task, payload, images=()):
        self.calls.append((task, payload))
        request_id = uuid4().hex
        if self.store:
            await self.store.begin_request(self.lease, request_id, "deterministic-contract-fixture", task,
                                           {"test_double": True, "external_request": False,
                                            "request_digest": fixture_request_digest(task, payload, images)})
            await self.store.finish_request(request_id, "sent")
        documents = {payload["document_id"]} if payload.get("document_id") else set()
        if task == "extract" and self.fail_document in documents:
            if self.store:
                await self.store.finish_request(request_id, "failed", error={"type": "injected_confirmed_service_rejection"})
            raise ModelInfrastructureError("injected confirmed service rejection")
        if task == "extract" and self.malformed_document in documents:
            result = {"not_an_extraction": True}
        elif task == "ocr":
            assert images
            result = {"text": "The supplier must provide the illustrated maintenance schedule.",
                      "status": "complete", "issues": []}
        elif task == "extract":
            requirements, dispositions = [], []
            for source in payload["body"]:
                texts = [text for text in fixture_source_texts(source) if "must" in text.lower()]
                requirement_ids = []
                for number, text in enumerate(texts, start=1):
                    requirement_id = f"req-{source['range_id']}" + (f":{number}" if len(texts) > 1 else "")
                    requirement_ids.append(requirement_id)
                    requirements.append({
                        "requirement_id": requirement_id, "text": text, "subject": "supplier",
                        "evidence": [{"range_id": source["range_id"], "quote": text}],
                        "conditions": [], "exceptions": [], "numbers": [], "scoring": {},
                        "veto_risk": False, "cross_references": [], "applicable": True,
                        "applicability_basis": "Unconditional supplier instruction in this synthetic fixture.",
                    })
                dispositions.append({"range_id": source["range_id"],
                                     "status": "requirements" if requirement_ids else "non_requirement",
                                     "reason": "Literal must instruction." if requirement_ids else "Source is a descriptive heading or empty field without an obligation.",
                                     "requirement_ids": requirement_ids})
            for template in payload.get("blank_table_templates", []):
                for rows in template["rows"]:
                    dispositions.extend({
                        "range_id": range_id, "status": "non_requirement",
                        "reason": "Repeated empty entry slots; the supplied template preserves columns, row coordinates and merged fields.",
                        "requirement_ids": [],
                    } for range_id in rows["range_ids"])
            result = {"requirements": requirements, "dispositions": dispositions, "issues": []}
        elif task == "verify":
            template_ids = [range_id for template in payload.get("blank_table_templates", [])
                            for rows in template["rows"] for range_id in rows["range_ids"]]
            result = {"reviewed_range_ids": list(dict.fromkeys(
                [s["range_id"] for s in [*payload["body"], *payload["supporting"], *payload.get("source_aliases", [])]] + template_ids)),
                      "findings": []}
        elif task == "plan":
            result = {"items": [{"requirement_id": requirement["requirement_id"],
                                 **payload["constraint_bindings"][requirement["requirement_id"]],
                                 "target_kind": "body", "target": "Section Delivery / compliance response",
                                 "action": "Write a specific commitment and responsible person for: " + requirement["text"],
                                 "material_needs": ["Supplier's signed delivery or maintenance commitment for this obligation."]}
                                for requirement in payload["requirements"]]}
        else:
            raise AssertionError(f"unknown fixture task {task}")
        if self.store:
            await self.store.finish_request(request_id, "succeeded", usage=None)
        return result
