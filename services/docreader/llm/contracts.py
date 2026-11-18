"""Typed generation contracts; tolerant response recovery stays at its own boundary."""
from functools import lru_cache
import hashlib
import json

from pydantic import Field, create_model

from docreader.llm.request import ModelConfigurationError
from docreader.tender.status import AuthoringTask
from docreader.tender.directory_compile import (
    ClassificationPlan, DirectoryPlan, MaterialPlan, NativeMaterialPlan, ParameterPlan, RequirementPlan,
)
from docreader.tender.directory_response_forms import response_form_model
from docreader.tender.directory_ref_plan import RefDirectoryPlan, RefDirectoryPlanV2
from docreader.tender.directory_source_groups import MaterialSourceGroupPlan, SOURCE_GROUP_OPERATION
from docreader.tender.directory_material_closure import MaterialClosurePlan, SOURCE_CARRIER_OPERATION
from docreader.tender.reviewed_source_facts import ReviewedClassificationPlan, REVIEWED_CLASSIFICATION_OPERATION
from docreader.tender.native_requirement_sources import (
    CLASSIFICATION_OPERATION, REQUIREMENT_OPERATION, NativeClassificationPlan, NativeRequirementPlan,
    native_requirement_model,
)
from docreader.tender.schema import (
    AnalysisIssue, AnalysisReviewResponse, AnalysisSharedFinding, AnalysisVersion,
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
    'authoring_write': WritingResponse, 'authoring_review': AuthoringContentReviewResponse,
    'authoring_structure': AuthoringStructureResponse,
}


def response_model(task, payload):
    if task == 'authoring_review' and payload.get('operation') == 'review_saved_word':
        return AuthoringReviewResponse
    if task == AuthoringTask.DIRECTORY_PLAN:
        if payload.get('operation') == 'select_directory_refs_v2':
            return RefDirectoryPlanV2
        return RefDirectoryPlan if payload.get('operation') == 'select_directory_refs_v1' else DirectoryPlan
    if task == AuthoringTask.DIRECTORY_REQUIREMENT:
        return native_requirement_model(payload) if payload.get('operation') == REQUIREMENT_OPERATION else RequirementPlan
    if task == AuthoringTask.DIRECTORY_MATERIAL:
        if payload.get('operation') == SOURCE_CARRIER_OPERATION:
            return MaterialClosurePlan
        if payload.get('operation') == SOURCE_GROUP_OPERATION:
            return MaterialSourceGroupPlan
        return NativeMaterialPlan if payload.get('operation') == 'select_response_forms_v1' else MaterialPlan
    if task == AuthoringTask.DIRECTORY_CLASSIFICATION:
        if payload.get('operation') == REVIEWED_CLASSIFICATION_OPERATION:
            return ReviewedClassificationPlan
        if payload.get('operation') == CLASSIFICATION_OPERATION and 'source_line_count' in payload:
            from docreader.tender.native_requirement_flow import native_classification_model
            return native_classification_model(payload)
        return NativeClassificationPlan if payload.get('operation') == CLASSIFICATION_OPERATION else ClassificationPlan
    if task == AuthoringTask.DIRECTORY_PARAMETER:
        return ParameterPlan
    if task == AuthoringTask.DIRECTORY_RESPONSE_FORM:
        return response_form_model(payload)
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
              Extraction, Verification, OutlineResponse, OutlineAssemblyResponse,
              AuthoringReviewResponse, AuthoringContentReviewResponse,
              DirectoryPlan, RefDirectoryPlan, RefDirectoryPlanV2, MaterialPlan, NativeMaterialPlan, RequirementPlan, ClassificationPlan, ParameterPlan,
              NativeRequirementPlan, NativeClassificationPlan, MaterialSourceGroupPlan, MaterialClosurePlan, ReviewedClassificationPlan,
              response_form_model({'atom_count': 1, 'form': {'grid': {'row_count': 1, 'column_count': 4}}}),
              response_form_model({'operation': 'select_native_response_form_v2', 'atom_count': 1,
                                   'mandate': [{}], 'form': {'grid': {'row_count': 1, 'column_count': 4}}}),
              response_form_model({'operation': 'select_native_response_form_v3', 'atom_count': 1,
                                   'mandate': [{}], 'form': {'grid': {'row_count': 1, 'column_count': 4}}})}
    return {model.__name__: hashlib.sha256(_schema_text(model).encode()).hexdigest()
            for model in sorted(models, key=lambda model: model.__name__)}


def schema_prompt(prompt, task, payload):
    return (prompt.rstrip() + '\n\n返回JSON须符合以下JSON Schema；它定义结构，前文定义业务语义。'
            '可选字段可省略；不要回抄Schema、输入清单或宿主持久身份。\n'
            + _schema_text(response_model(task, payload)))
