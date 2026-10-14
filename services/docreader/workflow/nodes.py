"""Tender-specific graph nodes; only file references leave these functions."""
import asyncio
import hashlib
import json
import time

from langgraph.types import interrupt

from docreader.integrations.store import InvalidConfirmation
from docreader.tender.checks import verify_all, verify_pack
from docreader.tender.evidence import EvidenceRepository, freeze_sources, register_manifest
from docreader.tender.planning import build_plan
from docreader.tender.requirements import extract_pack


class TenderNodes:
    def __init__(self, store, artifacts, lease, model=None):
        self.store, self.artifacts, self.lease = store, artifacts, lease
        self.model = model
        self.repo = EvidenceRepository(artifacts)
        self.semaphore = asyncio.Semaphore(4)
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
        await self._pin_source_spec()
        revision = await self.store.get_revision(self.lease)
        if revision["execution_spec"] is not None:
            await self._execution_spec()
        # Plan capacity is revision-owned, while native parser identity is shared.
        # Check it even if a completed checkpoint skips all graph nodes.
        row = await self.store.get_run(self.lease.run_id)
        plan_receipt = next((item for item in row["artifacts"]
                             if item["stage"] == "pack-plan" and not item["scope"]), None)
        if plan_receipt:
            from docreader.integrations.store import Conflict

            digest = hashlib.sha256(json.dumps(self._request_budget().spec(), sort_keys=True,
                                              separators=(",", ":")).encode()).hexdigest()
            if plan_receipt["metadata"]["budget_digest"] != digest:
                raise Conflict("request budget changed within an immutable domain revision")

    async def call(self, task, payload, images=()):
        return await self._ensure_model().call(task, payload, images=images)

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
            errors = [result for result in results if isinstance(result, BaseException)]
            if errors:
                raise errors[0]
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

            frozen = await self._read(frozen_ref)
            decisions = await self._read(scope_decisions_ref) if scope_decisions_ref else None
            result = await build_source_plan(
                frozen, self.artifacts, budget=self._request_budget(),
                scope_decisions=decisions["scope_decisions"] if decisions else [],
                run_identity={"run_id": self.lease.run_id, "source_version": self.lease.source_version,
                              "domain_revision": self.lease.domain_revision},
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

                ref = await self._save(stage, produce, scope=item["pack_id"],
                                       inputs={"source": item["artifact_ref"], "upstream": (upstream or {}).get(item["pack_id"])},
                                       model=True)
                return {"pack_id": item["pack_id"], "artifact_ref": ref}

        # Infrastructure failure of one pack never rolls back independent successful receipts.
        results = await asyncio.gather(*(worker(item) for item in index["packs"]), return_exceptions=True)
        failures = [result for result in results if isinstance(result, BaseException)]
        if failures:
            raise failures[0]
        return results

    async def extract(self, state):
        revision = await self.store.get_revision(self.lease)
        correction = revision["corrected_requirements_ref"]
        async def produce():
            if correction is not None:
                return await self._read(correction)
            index = await self._read(state["pack_plan_ref"])
            packs = await self._packs(index, "extract-pack", lambda pack: extract_pack(pack, self))
            return {"packs": packs, "counts": {"packs": len(packs)}}

        return {"requirements_ref": await self._save("extract", produce,
                                                  inputs={"pack_plan_ref": state["pack_plan_ref"], "corrected_requirements_ref": correction},
                                                  model=correction is None)}

    async def verify(self, state):
        async def produce():
            index = await self._read(state["pack_plan_ref"])
            extraction_index = await self._read(state["requirements_ref"])
            # Index once, not one full source scan per request.
            extractions = {
                item["pack_id"]: await self._read(item["artifact_ref"]) for item in extraction_index["packs"]
            }
            async def check(pack):
                return await verify_pack(pack, extractions[pack["pack_id"]], self)

            results = await self._packs(index, "verify-pack", check,
                                        {item["pack_id"]: item["artifact_ref"] for item in extraction_index["packs"]})
            verifications = [await self._read(item["artifact_ref"]) for item in results]
            summary = verify_all(index, list(extractions.values()), verifications)
            summary["pack_results"] = results
            summary["requirements_ref"] = state["requirements_ref"]
            return summary

        ref = await self._save("verify", produce, inputs={"pack_plan_ref": state["pack_plan_ref"],
                                                        "requirements_ref": state["requirements_ref"]}, model=True)
        checked = await self._read(ref)
        digest = hashlib.sha256(json.dumps({
            "source_version": self.lease.source_version, "domain_revision": self.lease.domain_revision,
            "frozen_ref": state["frozen_ref"], "pack_plan_ref": state["pack_plan_ref"],
            "requirements_ref": state["requirements_ref"], "verification_ref": ref,
        }, sort_keys=True).encode()).hexdigest()
        return {"verification_ref": ref, "checklist_digest": digest,
                "status": "blocked" if checked["blocking"] else "waiting-confirmation"}

    async def confirm(self, state):
        approved = interrupt({"run_id": self.lease.run_id, "source_version": self.lease.source_version, "domain_revision": self.lease.domain_revision,
                              "checklist_digest": state["checklist_digest"], "verification_ref": state["verification_ref"]})
        if (approved.get("source_version") != self.lease.source_version
                or approved.get("domain_revision") != self.lease.domain_revision
                or approved.get("checklist_digest") != state["checklist_digest"]):
            raise InvalidConfirmation("confirmation does not match this checklist")
        row = await self.store.get_run(self.lease.run_id)
        if not any(item["checklist_digest"] == state["checklist_digest"]
                   and item["domain_revision"] == self.lease.domain_revision for item in row["confirmations"]):
            raise InvalidConfirmation("confirmation was not persisted")

        async def produce():
            return approved

        await self._save("confirm", produce, inputs={"checklist_digest": state["checklist_digest"]})
        return {}

    async def plan(self, state):
        async def produce():
            from docreader.tender.checks import apply_applicability_decisions

            checked = await self._read(state["verification_ref"])
            extraction_index = await self._read(checked["requirements_ref"])

            async def group(entry):
                async with self.semaphore:
                    extraction = await self._read(entry["artifact_ref"])
                    requirements = [item for item in apply_applicability_decisions(
                        extraction["requirements"], checked["applicability_decisions"],
                    ) if item["applicable"]]
                    if not requirements:
                        return None
                    references = {item["requirement_id"]: {
                        "artifact_ref": entry["artifact_ref"], "requirement_id": item["requirement_id"],
                    } for item in requirements}
                    async def build():
                        return await build_plan(requirements, self, requirement_refs=references)
                    ref = await self._save("plan-pack", build, scope=entry["pack_id"],
                                           inputs={"requirements_ref": entry["artifact_ref"],
                                                   "verification_ref": state["verification_ref"],
                                                   "checklist_digest": state["checklist_digest"]},
                                           model=True)
                    return {"pack_id": entry["pack_id"], "artifact_ref": ref, "result": await self._read(ref)}

            results = await asyncio.gather(*(group(entry) for entry in extraction_index["packs"]), return_exceptions=True)
            errors = [result for result in results if isinstance(result, BaseException)]
            if errors:
                raise errors[0]
            results = [result for result in results if result is not None]
            return {"packs": [{"pack_id": result["pack_id"], "artifact_ref": result["artifact_ref"]} for result in results],
                    "blocking": any(result["result"]["blocking"] for result in results),
                    "checklist_digest": state["checklist_digest"],
                    "counts": {"packs": len(results),
                               "items": sum(len(result["result"]["items"]) for result in results),
                               "issues": sum(len(result["result"]["issues"]) for result in results)}}

        ref = await self._save("plan", produce, inputs={"verification_ref": state["verification_ref"],
                                                              "checklist_digest": state["checklist_digest"]}, model=True)
        plan = await self._read(ref)
        return {"response_plan_ref": ref, "status": "blocked" if plan["blocking"] else "response-plan-ready"}
