"""Typed generation contracts; tolerant response recovery stays at its own boundary."""
from functools import lru_cache
import hashlib
import json

from pydantic import Field

from docreader.llm.request import ModelConfigurationError
from docreader.tender.bid_format_outline import BidFormatOutline
from docreader.tender.item_placement import ItemPlacementResponse
from docreader.tender.proposal_outline import (
    PROPOSAL_TREE_OPERATION, ProposalOutlineResponse, ProposalTreeOnlyResponse,
)
from docreader.tender.status import AuthoringTask
from docreader.tender.schema import (
    AnalysisIssue, AnalysisReviewResponse, AnalysisVersion,
    AssociationResult, AuthoringBasisResponse, AuthoringContentReviewResponse, AuthoringReviewResponse,
    AuthoringStructureResponse, Contract,
    Evidence, Extraction, ItemDispositionOutput, LocateResult, OCRResponse, OutlineAssemblyResponse,
    OutlineResponse, Plan, TenderItem, Text, Verification, WritingResponse,
)


class ExtractionRelationOutput(Contract):
    from_item_id: Text
    to_item_id: Text
    relation: Text
    evidence: list[Evidence] = Field(min_length=1)


class ExtractionOutput(Contract):
    schema_version: AnalysisVersion
    items: list[TenderItem]
    dispositions: list[ItemDispositionOutput]
    relations: list[ExtractionRelationOutput]
    issues: list[AnalysisIssue]


_VERIFY = {'locate': LocateResult, 'associate': AssociationResult, 'review': AnalysisReviewResponse}
_DIRECT = {
    'ocr': OCRResponse, 'plan': Plan, 'authoring_basis': AuthoringBasisResponse,
    'authoring_write': WritingResponse, 'authoring_review': AuthoringContentReviewResponse,
    'authoring_structure': AuthoringStructureResponse,
}


def response_model(task, payload):
    if task == 'authoring_review' and payload.get('operation') == 'review_saved_word':
        return AuthoringReviewResponse
    if task == AuthoringTask.DIRECTORY_PLAN and payload.get('operation') == 'select_bid_format_outline_v1':
        return BidFormatOutline
    if task == AuthoringTask.ITEM_PLACEMENT:
        return ItemPlacementResponse
    if task == AuthoringTask.PROPOSAL_OUTLINE:
        return ProposalTreeOnlyResponse if payload.get('operation') == PROPOSAL_TREE_OPERATION else ProposalOutlineResponse
    if task == 'outline':
        return OutlineAssemblyResponse if payload.get('operation') == 'assemble' else OutlineResponse
    if task == 'extract':
        return ExtractionOutput if payload.get('schema_version') == 2 else Extraction
    if task == 'verify':
        if payload.get('schema_version') != 2:
            return Verification
        operation = payload.get('operation', 'review')
        if operation in _VERIFY:
            return _VERIFY[operation]
    elif task in _DIRECT:
        return _DIRECT[task]
    raise ModelConfigurationError('model task or operation has no generation contract')


@lru_cache(maxsize=None)
def _schema_text(model):
    return json.dumps(model.model_json_schema(mode='validation'), ensure_ascii=False,
                      sort_keys=True, separators=(',', ':'), allow_nan=False)


def output_schema(task, payload):
    """Return an independent schema so callers cannot mutate the cached contract."""
    return json.loads(_schema_text(response_model(task, payload)))


def _contracts_by_prompt():
    outline = {OutlineResponse, OutlineAssemblyResponse}
    return {
        'extract': {ExtractionOutput, Extraction}, 'verify': {*_VERIFY.values(), Verification},
        'ocr': {OCRResponse}, 'plan': {Plan},
        'outline': outline, 'outline_assemble': outline,
        'bid_format_outline': {BidFormatOutline},
        'item_placement': {ItemPlacementResponse},
        'proposal_outline': {ProposalOutlineResponse, ProposalTreeOnlyResponse},
        'authoring_basis': {AuthoringBasisResponse}, 'authoring_write': {WritingResponse},
        'authoring_review': {AuthoringContentReviewResponse, AuthoringReviewResponse},
        'authoring_structure': {AuthoringStructureResponse},
    }


def output_contract_spec(prompt_names):
    """BUG-259: hash only the contracts the named prompts can request."""
    table = _contracts_by_prompt()
    unknown = sorted(set(prompt_names) - table.keys())
    if unknown:
        raise ModelConfigurationError(f"prompts without a registered output contract: {unknown}")
    models = {model for name in prompt_names for model in table[name]}
    return {model.__name__: hashlib.sha256(_schema_text(model).encode()).hexdigest()
            for model in sorted(models, key=lambda model: model.__name__)}


def schema_prompt(prompt, task, payload):
    return (prompt.rstrip() + '\n\n返回JSON须符合以下JSON Schema；它定义结构，前文定义业务语义。'
            '可选字段可省略；不要回抄Schema、输入清单或宿主持久身份。\n'
            + _schema_text(response_model(task, payload)))
