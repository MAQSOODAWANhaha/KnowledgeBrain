"""BUG-259: an execution identity names only the prompts and contracts it can call."""

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelConfigurationError
from docreader.tender import document_execution
from docreader.tender.prompts import ANALYSIS_PROMPT_TASKS, execution_prompt, get_prompt

ANALYSIS_CONTRACTS = {"ExtractionOutput", "Extraction", "LocateResult", "AssociationResult",
                      "AnalysisReviewResponse", "Verification", "OCRResponse"}
BODY = {"authoring_basis", "authoring_write", "authoring_review", "authoring_structure"}


def analysis_spec(prompts=None):
    prompts = prompts or {task: get_prompt(task) for task in ANALYSIS_PROMPT_TASKS}
    return RequestBudget(prompts=prompts).spec()


def authoring_spec(names):
    prompts = document_execution.load_prompts()
    return RequestBudget(prompts={name: prompts[name] for name in names}).spec()


def test_analysis_identity_names_only_analysis_contracts():
    assert set(analysis_spec()["output_contracts"]) == ANALYSIS_CONTRACTS


def test_authoring_identity_never_names_analysis_contracts():
    outline = document_execution.execution_prompt_names(outline_only=True, fixed_outline=False)
    for names in (outline, BODY):
        assert not set(authoring_spec(names)["output_contracts"]) & ANALYSIS_CONTRACTS


def test_body_execution_identity_ignores_outline_prompts_and_contracts():
    body = document_execution.execution_prompt_names(outline_only=False, fixed_outline=True)
    assert set(body) == BODY
    spec = authoring_spec(body)
    assert set(spec["prompts"]) == BODY
    assert set(spec["output_contracts"]) == {"AuthoringBasisResponse", "WritingResponse",
                                             "AuthoringContentReviewResponse", "AuthoringReviewResponse",
                                             "AuthoringStructureResponse"}


def test_outline_and_whole_executions_keep_their_full_task_sets():
    every = set(document_execution.load_prompts())
    outline = set(document_execution.execution_prompt_names(outline_only=True, fixed_outline=False))
    assert outline == every - BODY
    assert set(document_execution.execution_prompt_names(outline_only=False, fixed_outline=False)) == every


def test_changing_a_verify_prompt_leaves_authoring_identity_unchanged():
    before = authoring_spec(BODY)
    changed = analysis_spec({**{task: get_prompt(task) for task in ANALYSIS_PROMPT_TASKS}, "verify": "changed"})
    assert changed != analysis_spec()
    assert authoring_spec(BODY) == before


def test_unknown_prompt_cannot_enter_an_identity():
    with pytest.raises(ModelConfigurationError):
        RequestBudget(prompts={"not_a_task": "x"}).spec()


def test_scoped_execution_rejects_a_prompt_outside_its_identity():
    prompts = {name: "p" for name in BODY}
    assert execution_prompt(prompts, "authoring_write") == "p"
    with pytest.raises(ModelConfigurationError):
        execution_prompt(prompts, "directory_plan")


def test_unscoped_test_budget_still_reads_the_packaged_prompt():
    analysis_only = {task: get_prompt(task) for task in ANALYSIS_PROMPT_TASKS}
    assert execution_prompt(analysis_only, "authoring_write") == document_execution.load_prompts()["authoring_write"]


def test_every_packaged_prompt_belongs_to_an_identity():
    from importlib.resources import files

    packaged = {path.name[:-3] for path in files("docreader.tender.prompts").iterdir() if path.name.endswith(".md")}
    registered = set(document_execution.load_prompts()) | set(ANALYSIS_PROMPT_TASKS) | {"plan"}
    assert packaged == registered


def test_outline_execution_can_send_the_bid_format_prompt():
    outline = document_execution.execution_prompt_names(outline_only=True, fixed_outline=False)
    prompts = {name: document_execution.load_prompts()[name] for name in outline}
    assert execution_prompt(prompts, "bid_format_outline")
    assert "BidFormatOutline" in authoring_spec(outline)["output_contracts"]
