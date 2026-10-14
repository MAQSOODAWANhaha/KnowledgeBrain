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

    async def call(self, task, payload, images=()):
        if self.model is None:
            from docreader.llm import ModelConfig, OpenAIModel

            self.model = OpenAIModel(ModelConfig.from_env(), self.store, self.lease)
        return await self.model.call(task, payload, images=images)

    async def close(self):
        if self.model is not None and hasattr(self.model, "close"):
            await self.model.close()

    async def _read(self, ref):
        return await asyncio.to_thread(self.artifacts.read_json, ref)

    async def _save(self, stage, operation, *, scope="", is_ref=False):
        previous = await self.store.receipt(self.lease, stage, scope)
        if previous:
            return previous["artifact_ref"]
        start = time.monotonic()
        value = await operation()
        ref = value if is_ref else await asyncio.to_thread(self.artifacts.put_json, value)
        metadata = {"elapsed_seconds": time.monotonic() - start}
        if isinstance(value, dict):
            metadata.update({key: value[key] for key in ("counts", "stats", "blocking") if key in value})
        await self.store.commit_artifact(self.lease, stage, ref, scope=scope, metadata=metadata)
        return ref

    async def register(self, state):
        async def produce():
            manifest = await self._read(await self.store.get_manifest(self.lease.run_id, self.lease.version))
            return await asyncio.to_thread(register_manifest, manifest, self.artifacts)

        return {"registered": await self._save("register", produce)}

    async def freeze(self, state):
        async def ocr(image_ref, source):
            import base64

            async def produce():
                data = await asyncio.to_thread(self.artifacts.read_bytes, image_ref)
                mime = image_ref.get("media_type", "image/png")
                image = f"data:{mime};base64,{base64.b64encode(data).decode('ascii')}"
                return await self.call("ocr", source, images=[image])

            ref = await self._save("ocr", produce, scope=f"{source['document_id']}:{source['unit_id']}")
            return await self._read(ref)

        async def produce():
            registered = await self._read(state["registered"])

            async def document(source):
                async with self.semaphore:
                    async def freeze_document():
                        return await freeze_sources({"documents": [source], "issues": []}, self.artifacts, ocr=ocr)
                    ref = await self._save("freeze-document", freeze_document, scope=str(source["ordinal"]), is_ref=True)
                    return await self._read(ref)

            results = await asyncio.gather(*(document(source) for source in registered["documents"]),
                                           return_exceptions=True)
            errors = [result for result in results if isinstance(result, BaseException)]
            if errors:
                raise errors[0]
            merged = {"schema_version": 1, "documents": [], "packs": [], "issues": list(registered["issues"]),
                      "range_count": 0, "stats": {}}
            for result in results:
                for key in ("documents", "packs", "issues"):
                    merged[key].extend(result[key])
                merged["range_count"] += result["range_count"]
                for key, value in result["stats"].items():
                    merged["stats"][key] = merged["stats"].get(key, 0) + value
            return merged

        return {"frozen": await self._save("freeze", produce)}

    async def _packs(self, index, stage, function):
        async def worker(item):
            async with self.semaphore:
                async def produce():
                    pack = await asyncio.to_thread(self.repo.load_pack, item["ref"])
                    return await function(pack)

                ref = await self._save(stage, produce, scope=item["pack_id"])
                return {"pack_id": item["pack_id"], "ref": ref}

        # Infrastructure failure of one pack never rolls back independent successful receipts.
        results = await asyncio.gather(*(worker(item) for item in index["packs"]), return_exceptions=True)
        failures = [result for result in results if isinstance(result, BaseException)]
        if failures:
            raise failures[0]
        return results

    async def extract(self, state):
        async def produce():
            index = await self._read(state["frozen"])
            packs = await self._packs(index, "extract-pack", lambda pack: extract_pack(pack, self))
            return {"packs": packs, "counts": {"packs": len(packs)}}

        return {"requirements": await self._save("extract", produce)}

    async def verify(self, state):
        async def produce():
            index = await self._read(state["frozen"])
            extraction_index = await self._read(state["requirements"])
            # Index once, not one full source scan per request.
            extractions = {
                item["pack_id"]: await self._read(item["ref"]) for item in extraction_index["packs"]
            }
            async def check(pack):
                return await verify_pack(pack, extractions[pack["pack_id"]], self)

            results = await self._packs(index, "verify-pack", check)
            verifications = [await self._read(item["ref"]) for item in results]
            summary = verify_all(index, list(extractions.values()), verifications)
            summary["pack_results"] = results
            summary["requirement_packs"] = {
                requirement["requirement_id"]: pack_id
                for pack_id, extraction in extractions.items() for requirement in extraction["requirements"]
            }
            return summary

        ref = await self._save("verify", produce)
        checked = await self._read(ref)
        digest = hashlib.sha256(json.dumps({
            "version": self.lease.version, "frozen": state["frozen"]["sha256"],
            "requirements": state["requirements"]["sha256"], "verified": ref["sha256"],
        }, sort_keys=True).encode()).hexdigest()
        return {"checked": ref, "checklist_digest": digest,
                "status": "blocked" if checked["blocking"] else "awaiting-confirmation"}

    async def confirm(self, state):
        approved = interrupt({"run_id": self.lease.run_id, "version": self.lease.version,
                              "checklist_digest": state["checklist_digest"], "checked": state["checked"]})
        if (approved.get("version") != self.lease.version
                or approved.get("checklist_digest") != state["checklist_digest"]):
            raise InvalidConfirmation("confirmation does not match this checklist")
        row = await self.store.get_run(self.lease.run_id)
        if not any(item["checklist_digest"] == state["checklist_digest"] for item in row["confirmations"]):
            raise InvalidConfirmation("confirmation was not persisted")

        async def produce():
            return approved

        await self._save("confirm", produce)
        return {}

    async def plan(self, state):
        async def produce():
            checked = await self._read(state["checked"])
            groups = {}
            for requirement in checked["requirements"]:
                if requirement["applicable"]:
                    groups.setdefault(checked["requirement_packs"][requirement["requirement_id"]], []).append(requirement)

            async def group(pack_id, requirements):
                async with self.semaphore:
                    async def build():
                        return await build_plan(requirements, self)
                    ref = await self._save("plan-pack", build, scope=pack_id)
                    return await self._read(ref)

            results = await asyncio.gather(*(group(key, value) for key, value in groups.items()), return_exceptions=True)
            errors = [result for result in results if isinstance(result, BaseException)]
            if errors:
                raise errors[0]
            return {"items": [item for result in results for item in result["items"]],
                    "issues": [issue for result in results for issue in result["issues"]],
                    "blocking": any(result["blocking"] for result in results),
                    "checklist_digest": state["checklist_digest"], "counts": {"packs": len(groups)}}

        ref = await self._save("plan", produce)
        plan = await self._read(ref)
        return {"plan": ref, "status": "blocked" if plan["blocking"] else "response-plan-ready"}
