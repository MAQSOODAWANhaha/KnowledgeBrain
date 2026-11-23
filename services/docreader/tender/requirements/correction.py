"""Apply a complete human correction against its immutable extraction baseline."""

from docreader.tender.requirements.extract import validate_item_correction
from docreader.tender.schema import assign_issues


CORRECTION_FIELDS = ("schema_version", "items", "dispositions", "relations", "issues")


def revise_item_extraction(repository, plan, baseline, correction, *, issues=()):
    artifacts = repository.artifacts
    if (not isinstance(correction, dict) or set(correction) != {"schema_version", "packs"}
            or correction["schema_version"] != 2 or not isinstance(correction["packs"], list)):
        raise ValueError("correction must contain schema_version 2 and a complete packs list")
    if baseline.get("schema_version") != 2:
        raise ValueError("legacy extraction is read-only; correction requires analysis v2 items")
    planned = {entry["pack_id"]: entry for entry in plan["packs"]}
    expected = {entry["pack_id"] for entry in baseline["packs"]}
    submitted = {}
    for item in correction["packs"]:
        if not isinstance(item, dict) or not isinstance(item.get("pack_id"), str) or item["pack_id"] in submitted:
            raise ValueError("correction pack identities must be unique")
        if set(item) != {"pack_id", *CORRECTION_FIELDS}:
            raise ValueError("correction pack must contain exactly the extraction fields")
        submitted[item["pack_id"]] = {key: item[key] for key in CORRECTION_FIELDS}
    if set(submitted) != expected or expected != set(planned):
        raise ValueError("correction must cover exactly every frozen extraction pack")

    packs = []
    for entry in baseline["packs"]:
        pack_id = entry["pack_id"]
        ref = entry["artifact_ref"]
        old = artifacts.read_json(ref)
        if old["pack_id"] != pack_id:
            raise ValueError("correction baseline does not identify the extraction pack")
        payload = submitted[pack_id]
        if payload != {key: old[key] for key in CORRECTION_FIELDS}:
            pack = repository.load_pack(planned[pack_id]["artifact_ref"])
            source = repository.source_index(pack["source_index_ref"])
            local_issues = assign_issues([*issues, *pack.get("issues", [])], pack_id)
            value = validate_item_correction(pack, payload, issues=local_issues,
                                             baseline=old, canonical_sources=source.by_id)
            value["issues"] = assign_issues(value["issues"], pack_id)
            ref = artifacts.put_json(value)
        packs.append({"pack_id": pack_id, "artifact_ref": ref})
    return {"schema_version": 2, "packs": packs, "counts": {"packs": len(packs)}}
