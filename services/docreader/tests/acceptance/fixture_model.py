"""Deterministic contract double. It never sends documents or reports paid usage."""
from uuid import uuid4

from docreader.llm import ModelInfrastructureError


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
                                           {"test_double": True, "external_request": False})
            await self.store.finish_request(request_id, "sent")
        documents = {source["document_id"] for source in payload.get("body", [])}
        if task == "extract" and self.fail_document in documents:
            if self.store:
                await self.store.finish_request(request_id, "uncertain", error={"type": "injected_transport_failure"})
            raise ModelInfrastructureError("injected transport failure")
        if task == "extract" and self.malformed_document in documents:
            result = {"not_an_extraction": True}
        elif task == "ocr":
            assert images
            result = {"text": "The supplier must provide the illustrated maintenance schedule.",
                      "status": "complete", "issues": []}
        elif task == "extract":
            requirements, dispositions = [], []
            for source in payload["body"]:
                text = source["text"]
                required = "must" in text.lower()
                requirement_id = f"req-{source['range_id']}"
                if required:
                    requirements.append({
                        "requirement_id": requirement_id, "text": text, "subject": "supplier",
                        "evidence": [{"range_id": source["range_id"], "quote": text}],
                        "conditions": [], "exceptions": [], "numbers": [], "scoring": {},
                        "veto_risk": False, "cross_references": [], "applicable": True,
                        "applicability_basis": "Unconditional supplier instruction in this synthetic fixture.",
                    })
                dispositions.append({"range_id": source["range_id"],
                                     "status": "requirements" if required else "non_requirement",
                                     "reason": "Literal must instruction." if required else "Source is a descriptive heading or empty field without an obligation.",
                                     "requirement_ids": [requirement_id] if required else []})
            result = {"requirements": requirements, "dispositions": dispositions, "issues": []}
        elif task == "verify":
            result = {"reviewed_range_ids": list(dict.fromkeys(s["range_id"] for s in [*payload["body"], *payload["supporting"]])),
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
