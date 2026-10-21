"""Tender-specific graph nodes; only file references leave these functions."""
import asyncio
import hashlib
import json
import time
from typing import get_args

from langgraph.types import interrupt
from docreader.integrations.store import Cancelled, InvalidConfirmation, StaleExecution
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender.evidence import EvidenceRepository, freeze_sources, register_manifest
from docreader.tender.schema import OCRStatus, validate_issue_resolutions
from docreader.workflow.state import WORKFLOW_PROTOCOL


def _raise_failures(results):
    failures = [result for result in results if isinstance(result, BaseException)]
    if failures:
        # Successful siblings have already committed. Cancellation and a lost
        # execution fence stop the worker; exhausted units cannot gain queue retries.
        stop = (Cancelled, StaleExecution)
        fallback = next((failure for failure in failures if isinstance(failure, ModelWorkUnitExhausted)), failures[0])
        raise next((failure for failure in failures if isinstance(failure, stop)), fallback)


def _ocr_response(value):
    if not isinstance(value, dict):
        raise ModelContractError([{'path': [], 'code': 'object_type', 'detail': 'Expected an OCR object'}])
    checks = (
        ('text', isinstance(value.get('text'), str), 'Expected literal transcription text'),
        ('status', value.get('status') in get_args(OCRStatus), 'Expected complete, partial or unresolved'),
        ('issues', isinstance(value.get('issues', []), list), 'Expected an array of OCR issues'),
    )
    errors = [{'path': [key], 'code': 'ocr_response_invalid', 'detail': detail}
              for key, valid, detail in checks if not valid]
    if errors:
        raise ModelContractError(errors)
    return value


class TenderNodes:
    def __init__(self, store, artifacts, lease, model=None):
        self.store, self.artifacts, self.lease = store, artifacts, lease
        self.model = model
        self.repo = EvidenceRepository(artifacts)
        self.semaphore = asyncio.Semaphore(2)
        self._ocr_locks = {}
        self._budget = None

    def _ensure_model(self):
        if self.model is None:
            from docreader.llm import ModelConfig, OpenAIModel

            self.model = OpenAIModel(ModelConfig.from_env(), self.store, self.lease, artifacts=self.artifacts)
        return self.model

    def _request_budget(self):
        if self._budget is not None:
            return self._budget
        model = self._ensure_model()
        if hasattr(model, "request_budget"):
            self._budget = model.request_budget()
        else:
            from docreader.llm.budget import RequestBudget

            # Deterministic injected models use the same ordinary context contract.
            self._budget = RequestBudget(context_tokens=131072, model="gpt-6-luna")
        return self._budget

    async def _execution_spec(self):
        model = self._ensure_model()
        spec = (model.execution_spec() if hasattr(model, "execution_spec") else
                {"injected_model": f"{type(model).__module__}.{type(model).__qualname__}"})
        spec = {**spec, "workflow_protocol": WORKFLOW_PROTOCOL}
        # Also check before returning cached model-derived artifacts after a restart.
        await self.store.pin_execution_spec(self.lease, spec)
        return spec

    async def _pin_source_spec(self):
        from docreader.workflow.source_spec import source_spec

        spec = source_spec()
        async def produce():
            return spec
        await self._save("source-spec", produce, inputs=spec)

    async def prepare(self):
        # A completed checkpoint can skip every node, so recovery guards belong here too.
        from docreader.integrations.store import Conflict

        revision = await self.store.get_revision(self.lease)
        row = await self.store.get_run(self.lease.run_id)
        spec = revision["execution_spec"]
        inherited_analysis = any(
            item["source_version"] == self.lease.source_version
            and (item.get("execution_spec") or {}).get("workflow_protocol") == WORKFLOW_PROTOCOL
            for item in row["history"]["revisions"]
        )
        if ((spec is not None and spec.get("workflow_protocol") != WORKFLOW_PROTOCOL)
                or (spec is None and row["artifacts"] and not inherited_analysis)):
            raise Conflict("execution protocol changed; prior revisions are read-only, submit a new analysis run")
        await self._execution_spec()
        await self._pin_source_spec()
        # Plan capacity is revision-owned, while native parser identity is shared.
        # Check it even if a completed checkpoint skips all graph nodes.
        plan_receipt = next((item for item in row["artifacts"]
                             if item["stage"] == "pack-plan" and not item["scope"]), None)
        if plan_receipt:
            from docreader.integrations.store import Conflict

            digest = hashlib.sha256(json.dumps(self._request_budget().spec(), sort_keys=True,
                                              separators=(",", ":")).encode()).hexdigest()
            if plan_receipt["metadata"]["budget_digest"] != digest:
                raise Conflict("request budget changed within an immutable domain revision")

    async def call(self, task, payload, images=()):
        from docreader.llm.recovery import call_validated

        validate = _ocr_response if task == "ocr" else lambda response: response
        return await call_validated(self._ensure_model(), self.store, self.artifacts,
                                    self.lease, task, payload, validate, images=images)

    async def call_validated(self, task, payload, validate):
        from docreader.llm.recovery import call_validated

        return await call_validated(self._ensure_model(), self.store, self.artifacts,
                                    self.lease, task, payload, validate)

    async def close(self):
        if self.model is not None and hasattr(self.model, "close"):
            await self.model.close()

    async def _read(self, ref):
        return await asyncio.to_thread(self.artifacts.read_json, ref)

    async def _save(self, stage, operation, *, scope="", is_ref=False, inputs=None, model=False):
        identity = {"upstream": inputs or {}}
        if model:
            identity["execution_spec"] = await self._execution_spec()
        input_digest = hashlib.sha256(json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        previous = await self.store.receipt(self.lease, stage, scope, input_digest=input_digest)
        if previous:
            return previous["artifact_ref"]
        start = time.monotonic()
        value = await operation()
        ref = value if is_ref else await asyncio.to_thread(self.artifacts.put_json, value)
        metadata = {"elapsed_seconds": time.monotonic() - start}
        if isinstance(value, dict):
            metadata.update({key: value[key] for key in ("counts", "stats", "blocking", "plan_digest", "budget_digest") if key in value})
        await self.store.commit_artifact(self.lease, stage, ref, scope=scope, metadata=metadata,
                                         input_digest=input_digest)
        return ref

    async def register(self, state):
        manifest_ref = await self.store.get_manifest(self.lease.run_id, self.lease.source_version)
        async def produce():
            manifest = await self._read(manifest_ref)
            return await asyncio.to_thread(register_manifest, manifest, self.artifacts)

        return {"registered_ref": await self._save("register", produce, inputs=manifest_ref)}

    async def freeze(self, state):
        await self._pin_source_spec()
        async def ocr(image_ref, source):
            # Occurrence coordinates stay in raw evidence. Literal OCR sees only
            # pixels, so repeated logos/stamps never need one request per page.
            import base64

            async def produce():
                data = await asyncio.to_thread(self.artifacts.read_bytes, image_ref)
                mime = image_ref.get("media_type", "image/png")
                image = f"data:{mime};base64,{base64.b64encode(data).decode('ascii')}"
                return await self.call("ocr", {"image_sha256": image_ref["sha256"]}, images=[image])

            image_identity = image_ref["sha256"]
            lock = self._ocr_locks.setdefault(image_identity, asyncio.Lock())
            async with lock:
                ref = await self._save("ocr", produce, scope=image_identity,
                                       inputs=image_ref, model=True)
                return await self._read(ref)

        async def produce():
            registered = await self._read(state["registered_ref"])

            async def document(source):
                async with self.semaphore:
                    async def freeze_document():
                        return await freeze_sources({"documents": [source], "issues": [],
                                                     "image_dispositions": registered.get("image_dispositions", [])}, self.artifacts,
                                                    ocr=ocr, budget=self._request_budget(), plan=False)
                    ref = await self._save("freeze-document", freeze_document, scope=str(source["ordinal"]),
                                           is_ref=True, inputs={"source": source,
                                                               "image_dispositions": registered.get("image_dispositions", [])})
                    return await self._read(ref)

            results = await asyncio.gather(*(document(source) for source in registered["documents"]),
                                           return_exceptions=True)
            _raise_failures(results)
            merged = {"schema_version": 2, "documents": [], "issues": list(registered["issues"]),
                      "native_range_count": 0, "stats": {}}
            for result in results:
                for key in ("documents", "issues"):
                    merged[key].extend(result[key])
                merged["native_range_count"] += result["native_range_count"]
                for key, value in result["stats"].items():
                    merged["stats"][key] = merged["stats"].get(key, 0) + value
            return merged

        frozen_ref = await self._save("freeze", produce, inputs=state["registered_ref"])
        revision = await self.store.get_revision(self.lease)
        scope_decisions_ref = revision.get("scope_decisions_ref")

        async def plan_sources():
            from docreader.tender.evidence import plan_sources as build_source_plan
            from docreader.tender.checks.analysis import assign_issues
            from docreader.tender.requirements import item_payload

            frozen = await self._read(frozen_ref)
            roles = {document["document_id"]: document["role"] for document in frozen["documents"]}
            decisions = await self._read(scope_decisions_ref) if scope_decisions_ref else None
            identity = {"run_id": self.lease.run_id, "source_version": self.lease.source_version,
                        "domain_revision": self.lease.domain_revision}
            source_issues = assign_issues(frozen["issues"], "source")
            result = await build_source_plan(
                frozen, self.artifacts, budget=self._request_budget(),
                analysis=True,
                scope_decisions=decisions["scope_decisions"] if decisions else [],
                run_identity=identity,
                payload_builder=lambda pack: item_payload(pack, identity=identity, role=roles[pack["document_id"]],
                    issues=assign_issues([*source_issues, *pack.get("issues", [])], pack["pack_id"]),
                    scope_decisions=decisions["scope_decisions"] if decisions else ()),
            )
            return await self._read(result)

        pack_plan_ref = await self._save("pack-plan", plan_sources, inputs={
            "frozen_ref": frozen_ref, "scope_decisions_ref": scope_decisions_ref,
            "request_budget": self._request_budget().spec(),
        })
        return {"frozen_ref": frozen_ref, "pack_plan_ref": pack_plan_ref}

    async def _packs(self, index, stage, function, upstream=None):
        async def worker(item):
            async with self.semaphore:
                async def produce():
                    pack = await asyncio.to_thread(self.repo.load_pack, item["artifact_ref"])
                    return await function(pack)

                scope = (f"{index['plan_digest']}:{item['pack_id']}" if index.get("task") == "verify" else item["pack_id"])
                ref = await self._save(stage, produce, scope=scope,
                                       inputs={"source": item["artifact_ref"], "upstream": (upstream or {}).get(item["pack_id"])},
                                       model=True)
                return {"pack_id": item["pack_id"], "artifact_ref": ref}

        # Infrastructure failure of one pack never rolls back independent successful receipts.
        results = await asyncio.gather(*(worker(item) for item in index["packs"]), return_exceptions=True)
        _raise_failures(results)
        return results

    async def extract(self, state):
        from docreader.tender.checks.analysis import assign_issues
        from docreader.tender.requirements import extract_items, validate_item_correction

        revision = await self.store.get_revision(self.lease)
        correction = revision["corrected_items_ref"]
        index = await self._read(state["pack_plan_ref"])
        frozen = await self._read(state["frozen_ref"])
        roles = {document["document_id"]: document["role"] for document in frozen["documents"]}
        identity = {"run_id": self.lease.run_id, "source_version": self.lease.source_version,
                    "domain_revision": self.lease.domain_revision}
        global_issues = assign_issues(frozen["issues"], "source")
        decisions_ref = revision.get("scope_decisions_ref")
        decisions = await self._read(decisions_ref) if decisions_ref else None
        scope_decisions = decisions["scope_decisions"] if decisions else ()

        async def produce():
            if correction is not None:
                corrected = await self._read(correction)
                if corrected.get("schema_version") != 2:
                    raise ValueError("correction must use the analysis v2 protocol")
                expected = {entry["pack_id"]: entry for entry in index["packs"]}
                entries = corrected.get("packs", [])
                if (len(entries) != len(expected) or
                        {entry["pack_id"] for entry in entries} != set(expected)):
                    raise ValueError("correction must cover every current extraction pack")
                packs = []
                for entry in entries:
                    pack = await asyncio.to_thread(self.repo.load_pack, expected[entry["pack_id"]]["artifact_ref"])
                    value = await self._read(entry["artifact_ref"])
                    # Revalidate even callers that bypassed the CLI. Host metadata is
                    # rebuilt by the validator; it cannot authorize altered source IDs.
                    payload = {key: value[key] for key in
                               ("schema_version", "items", "dispositions", "relations", "issues")}
                    issues = assign_issues([*global_issues, *pack.get("issues", [])], pack["pack_id"])
                    validated = validate_item_correction(pack, payload, issues=issues)
                    validated["issues"] = assign_issues(validated["issues"], pack["pack_id"])
                    ref = await asyncio.to_thread(self.artifacts.put_json, validated)
                    packs.append({"pack_id": pack["pack_id"], "artifact_ref": ref})
                return {"schema_version": 2, "packs": packs, "counts": {"packs": len(packs)}}

            async def extract(pack):
                issues = assign_issues([*global_issues, *pack.get("issues", [])], pack["pack_id"])
                result = await extract_items(pack, self, identity=identity, issues=issues,
                                             scope_decisions=scope_decisions, role=roles[pack["document_id"]])
                result["issues"] = assign_issues(result.get("issues", []), pack["pack_id"])
                return result

            packs = await self._packs(index, "extract-pack", extract)
            return {"schema_version": 2, "packs": packs, "counts": {"packs": len(packs)}}

        return {"items_ref": await self._save("extract", produce,
                                              inputs={"pack_plan_ref": state["pack_plan_ref"],
                                                      "corrected_items_ref": correction},
                                              model=correction is None)}

    async def verify(self, state):
        from docreader.tender.checks.analysis import verify_analysis

        async def produce():
            source_plan = await self._read(state["pack_plan_ref"])
            items_index = await self._read(state["items_ref"])
            if items_index.get("schema_version") != 2:
                raise ValueError("analysis verification requires v2 items")
            summary = await verify_analysis(
                source_plan, items_index, self.artifacts, self, self._request_budget(),
                identity={"run_id": self.lease.run_id, "source_version": self.lease.source_version,
                          "domain_revision": self.lease.domain_revision},
                save=self._save,
            )
            return {**summary, "items_ref": state["items_ref"]}

        ref = await self._save("verify", produce, inputs={"pack_plan_ref": state["pack_plan_ref"],
                                                        "items_ref": state["items_ref"]}, model=True)
        checked = await self._read(ref)
        digest = hashlib.sha256(json.dumps({
            "schema_version": 2,
            "source_version": self.lease.source_version, "domain_revision": self.lease.domain_revision,
            "frozen_ref": state["frozen_ref"], "pack_plan_ref": state["pack_plan_ref"],
            "items_ref": state["items_ref"], "verification_ref": ref,
        }, sort_keys=True).encode()).hexdigest()
        return {"verification_ref": ref, "checklist_digest": digest,
                "status": "blocked" if checked["hard_blocking"] else "waiting-confirmation"}

    async def confirm(self, state):
        approved = interrupt({"run_id": self.lease.run_id, "source_version": self.lease.source_version,
                              "domain_revision": self.lease.domain_revision,
                              "checklist_digest": state["checklist_digest"],
                              "verification_ref": state["verification_ref"]})
        if (approved.get("source_version") != self.lease.source_version
                or approved.get("domain_revision") != self.lease.domain_revision
                or approved.get("checklist_digest") != state["checklist_digest"]):
            raise InvalidConfirmation("confirmation does not match this checklist")
        row = await self.store.get_run(self.lease.run_id)
        if not any(item["checklist_digest"] == state["checklist_digest"]
                   and item["domain_revision"] == self.lease.domain_revision for item in row["confirmations"]):
            raise InvalidConfirmation("confirmation was not persisted")
        checked = await self._read(state["verification_ref"])
        if checked["hard_blocking"] or checked.get("items_ref") != state["items_ref"]:
            raise InvalidConfirmation("analysis confirmation requires the verified current items")
        try:
            resolutions = validate_issue_resolutions(checked["human_issue_ids"], approved.get("issue_resolutions"))
        except ValueError:
            raise InvalidConfirmation("confirmation must resolve exactly the pending semantic issues") from None
        if resolutions != approved.get("issue_resolutions"):
            raise InvalidConfirmation("confirmation resolutions were not persisted in canonical form")

        async def produce():
            return approved

        await self._save("confirm", produce, inputs={"checklist_digest": state["checklist_digest"]})
        return {"status": "analysis-ready"}
