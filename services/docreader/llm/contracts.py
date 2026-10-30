"""Typed generation contracts; tolerant response recovery stays at its own boundary."""
from functools import lru_cache
import hashlib
import json

from pydantic import create_model

from docreader.llm.request import ModelConfigurationError
from docreader.tender.schema import (
    AnalysisIssue, AnalysisReviewResponse, AnalysisSharedFinding, AnalysisVersion,
    AssociationResult, AuthoringBasisResponse, AuthoringReviewResponse, Contract,
    Extraction, ItemDispositionOutput, LocateResult, OCRResponse, OutlineAssemblyResponse,
    OutlineResponse, Plan, Relation, TenderItem, Text, Verification, WritingResponse,
)


class ExtractionOutput(Contract):
    schema_version: AnalysisVersion
    items: list[TenderItem]
    dispositions: list[ItemDispositionOutput]
    relations: list[Relation]
    issues: list[AnalysisIssue]


def _batch_response(name, response):
    # Reuse the semantic result fields without a second per-member version or
    # the host's reviewed_* coverage. The receiver still recovers members singly.
    member = create_model(name + 'WorkUnit', __base__=Contract,
        work_unit_id=(Text, ...), **{
            key: (field.annotation, field) for key, field in response.model_fields.items()
            if key != 'schema_version'})
    return create_model(name + 'Batch', __base__=Contract,
        schema_version=(AnalysisVersion, ...), work_units=(list[member], ...),
        shared_findings=(list[AnalysisSharedFinding], ...))


_VERIFY = {'locate': LocateResult, 'associate': AssociationResult, 'review': AnalysisReviewResponse}
_BATCHES = {operation: _batch_response(response.__name__, response)
            for operation, response in _VERIFY.items() if operation != 'locate'}
_DIRECT = {
    'ocr': OCRResponse, 'plan': Plan, 'authoring_basis': AuthoringBasisResponse,
    'authoring_write': WritingResponse, 'authoring_review': AuthoringReviewResponse,
    'authoring_structure': AuthoringReviewResponse,
}


def response_model(task, payload):
    if task == 'outline':
        return OutlineAssemblyResponse if payload.get('operation') == 'assemble' else OutlineResponse
    if task == 'extract':
        return ExtractionOutput if payload.get('schema_version') == 2 else Extraction
    if task == 'verify':
        if payload.get('schema_version') != 2:
            return Verification
        operation = payload.get('operation', 'review')
        models = _BATCHES if 'work_units' in payload else _VERIFY
        if operation in models:
            return models[operation]
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


def output_contract_spec():
    models = {*_DIRECT.values(), *_VERIFY.values(), *_BATCHES.values(), ExtractionOutput,
              Extraction, Verification, OutlineResponse, OutlineAssemblyResponse}
    return {model.__name__: hashlib.sha256(_schema_text(model).encode()).hexdigest()
            for model in sorted(models, key=lambda model: model.__name__)}


def schema_prompt(prompt, task, payload):
    return (prompt.rstrip() + '\n\n返回JSON须符合以下JSON Schema；它定义结构，前文定义业务语义。'
            '可选字段可省略；不要回抄Schema、输入清单或宿主持久身份。\n'
            + _schema_text(response_model(task, payload)))
