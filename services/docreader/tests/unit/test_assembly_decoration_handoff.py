"""Accepted source-decoration exclusions survive later explicit template spans."""

from copy import deepcopy
from io import BytesIO
import json

from docx import Document
import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_templates import load_template_part
from test_authoring_templates import create_context, create_source
from test_complete_format_pipeline import assembled, source_form, source_result


DECORATION = "源文件固定页首装饰"
BODY = "投标函完整承诺正文。"
SIGNATURE = "法定代表人签字（盖章）：____"
CONDITION = "本表仅适用于代理商参与投标的情形。"


def targets(chapters):
    return [target for chapter in pipeline._flatten(chapters) for target in chapter["targets"]]


async def organize(tmp_path, selection, *, shared_candidate=False, ambiguous_subject=False):
    artifacts, index_ref, native = create_source(tmp_path, [
        {"text": text} for text in [DECORATION, BODY, SIGNATURE, CONDITION]])
    context = create_context(artifacts, index_ref, native)
    before = deepcopy((context.payload, native.raw, artifacts.read_json(index_ref)))
    original_sources = deepcopy(context.sources)
    calls, source_parts, reviews = [], [], []
    subjects = ["投标人", "投标人" if ambiguous_subject else "产品原厂"]
    identities = ["t1", "t1" if shared_candidate else "t2"]

    async def execute(stage, request, validate):
        value = json.loads(request["messages"][1]["content"][0]["text"])
        calls.append(stage)
        if stage == "authoring_structure":
            reviews.append(deepcopy(value))
            if len(reviews) > 1:
                return validate({"findings": []})
            first, _ = targets(value["chapters"])
            return validate({
                "findings": [{"code": "retain_instances", "detail": "保留实际填写实例及完整正文。"}],
                "source_decoration_exclusions": [{
                    "target_id": first["target_id"],
                    "support_ids": first["template_support_ids"][:1],
                    "reason": "该精确独立部件仅为源文件固定页首装饰。"}]})
        if value.get("operation") == "assemble":
            if "previous_structure" in value:
                first, second = targets(value["previous_structure"])
                assert first["candidate_target_ids"] == ["t1"]
                assert first["template_part_ids"] == source_parts[1:]
                assert second["candidate_target_ids"] == [identities[1]]
                assert second["template_part_ids"] == source_parts
            result = []
            for identity, subject in zip(identities, subjects):
                target = {"kind": "form", "purpose": "保留正文、适用条件及签署位置。",
                          "target_ids": [identity], "subject": subject}
                if "previous_structure" in value:
                    if selection == "parts":
                        target["template_part_ids"] = source_parts
                    else:
                        target["template_spans"] = [{
                            "start_part_id": source_parts[0], "end_part_id": source_parts[-1]}]
                result.append(target)
            return validate(assembled(result))
        source_parts.extend(part["part_id"] for part in value["template_parts"])
        source_targets = ([source_form(source_parts)] if shared_candidate else [
            source_form(source_parts, subject=subject) for subject in subjects])
        return validate(source_result(source_targets))

    outline = await pipeline._organize(artifacts, context,
        RequestBudget(model="fixture", context_tokens=131072), execute, [], structure_results={})
    assert (context.payload, native.raw, artifacts.read_json(index_ref)) == before
    assert all(context.sources[identity] == refs for identity, refs in original_sources.items())
    assert calls[:4] == ["outline", "outline", "authoring_structure", "outline"]
    assert calls[4:] in [[], ["authoring_structure"]]
    return artifacts, outline


@pytest.mark.parametrize("selection", ["parts", "spans"])
async def test_explicit_revision_does_not_revive_confirmed_decoration_or_expand_its_scope(
        tmp_path, selection):
    artifacts, outline = await organize(tmp_path, selection)
    first, second = targets(outline["chapters"])

    def selected_text(target):
        return [load_template_part(artifacts, ref)["text"] for ref in target["template_refs"]]

    assert selected_text(first) == [BODY, SIGNATURE, CONDITION]
    # The same part belongs to a different candidate too. Its exclusion was not
    # accepted for this instance and must not become a global text/part filter.
    assert selected_text(second) == [DECORATION, BODY, SIGNATURE, CONDITION]
    exported = export_authoring_document(artifacts, outline, [])
    document = Document(BytesIO(artifacts.read_bytes(exported["docx_ref"])))
    visible = [paragraph.text for paragraph in document.paragraphs]
    assert visible.count(DECORATION) == 1
    assert all(visible.count(text) == 2 for text in [BODY, SIGNATURE, CONDITION])


async def test_shared_candidate_keeps_f0_decoration_exclusion_on_its_authorized_instance(tmp_path):
    artifacts, outline = await organize(tmp_path, "spans", shared_candidate=True)
    first, second = targets(outline["chapters"])
    assert [first["subject"], second["subject"]] == ["投标人", "产品原厂"]
    first_text = [load_template_part(artifacts, ref)["text"] for ref in first["template_refs"]]
    second_text = [load_template_part(artifacts, ref)["text"] for ref in second["template_refs"]]
    assert first_text == [BODY, SIGNATURE, CONDITION]
    assert second_text == [DECORATION, BODY, SIGNATURE, CONDITION]


async def test_ambiguous_instances_do_not_inherit_another_instances_decoration_exclusion(tmp_path):
    artifacts, outline = await organize(
        tmp_path, "spans", shared_candidate=True, ambiguous_subject=True)
    instances = targets(outline["chapters"])
    assert len(instances) == 2
    assert all([load_template_part(artifacts, ref)["text"] for ref in target["template_refs"]]
               == [DECORATION, BODY, SIGNATURE, CONDITION] for target in instances)
