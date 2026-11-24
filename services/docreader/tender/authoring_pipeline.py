"""Bounded document authoring: semantics in model calls, references in the host.

The executor owns paid retries and receipts. Business gaps become located output;
cancellation, lease loss and storage errors retain their infrastructure semantics.
"""
from __future__ import annotations

import asyncio
from copy import deepcopy
import hashlib
import json

from pydantic import ValidationError

from docreader.llm.request import ModelBudgetError, ModelConfigurationError, ModelContractError, ModelWorkUnitExhausted, build_request
from docreader.llm.wire import WirePayload
from docreader.tender.authoring import (
    _OutlineContext, _check_reference_ids, load_outline_source, project_source_sections, validate_outline_references,
)
from docreader.tender.authoring_templates import (
    prepare_template_parts, load_template_part, template_part_key, ordered_template_part_ids, template_part_runs, expand_template_spans,
)
from docreader.tender.authoring_excerpts import (
    load_source_excerpt, source_excerpt_key, whole_source_excerpts, response_row_id, response_table_excerpts,
)
from docreader.tender.status import AuthoringTask
from docreader.tender.directory_compile import (
    ClassificationPlan, DirectoryPlan, ReadingDirectoryPlan, DirectoryPlanRejected, MaterialPlan, NativeMaterialPlan, ParameterPlan, RequirementPlan,
    PARAMETER_SOURCE_CONTRACT,
    apply_classification, apply_materials, apply_parameters, apply_requirements, apply_table_headers, compile_directory, compile_plans,
    documents_from_frozen, note_missing_requirements, outline_from_compiled, parameter_view, reading_payload,
    requirement_blocks, soften_directory_plan, sparse_page_issues, table_header_catalog, unplaced_tables,
    validate_requirement_windows, validate_table_headers,
)
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.directory_adjudication import (
    requirement_cases, validate_requirement_adjudications, apply_requirement_adjudications,
)
from docreader.tender.directory_choices import (
    directory_cases, apply_directory_adjudications, reference_exclusions,
    validate_reference_exclusions, validate_requirement_exclusions,
)
from docreader.tender.directory_titles import (
    title_catalog, validate_chapter_titles, apply_chapter_titles, accept_parameter_titles,
)
from docreader.tender.directory_ref_plan import (
    DIRECTORY_REF_CONTRACT, LEGACY_DIRECTORY_REF_CONTRACT, RefDirectoryPlan, RefDirectoryPlanV2, decode_plan,
)
from docreader.tender.directory_sources import SourceRefs
from docreader.tender.directory_source_groups import (
    SOURCE_GROUP_CONTRACT, SOURCE_GROUP_OPERATION, MaterialSourceGroupPlan,
    apply_source_groups, source_group_candidates, validate_source_groups,
)
from docreader.tender.directory_material_closure import (
    SOURCE_CARRIER_CONTRACT, SOURCE_CARRIER_OPERATION, MaterialClosurePlan,
    apply_carrier_closure, candidate_carriers, native_parts, source_carrier_catalog,
)
from docreader.tender.directory_response_forms import (
    RESPONSE_FORM_CONTRACTS, ResponseFormView, response_form_catalog, require_response_form_budget,
    validate_response_form_selections, pending_response_form,
)
from docreader.tender.reviewed_source_facts import (
    REVIEWED_CLASSIFICATION_OPERATION, REVIEWED_BODY_ENTRY_CONTRACT,
    load_reviewed_facts, reviewed_material_summary, apply_reviewed_material_facts,
    reviewed_body_summary, validate_reviewed_body_dispositions, apply_reviewed_body_dispositions, ReviewedClassificationPlan,
    bind_reviewed_outline_facts, bind_reviewed_credential_slots,
    bind_reviewed_carrier_results,
    reviewed_classification_view, apply_reviewed_classification,
)
from docreader.integrations.store import Conflict
from docreader.tender.prompts import execution_prompt
from docreader.tender.authoring_coverage import (
    accept_assessments, attach_w2_proposal_answers, batch_coverage_view, build_ledger, coverage_scope_index, fair_select, plan_queries,
    rule_ref_key, settle_coverage, task_coverage_refs, unit_fingerprint,
)
from docreader.tender.schema import (
    AUTHORING_GROUNDING_POLICY, AUTHORING_UNIT_BINDING_CONTRACT,
    AuthoringBasis, AuthoringBasisContent, AuthoringBlock, AuthoringBasisResponse, AuthoringCoverage, AuthoringDocument, AuthoringDocumentPlan,
    AuthoringInput, AuthoringOutline, AuthoringQueryPlan, AuthoringReviewRecord, AuthoringReviewResponse, AuthoringUsage,
    KnowledgeMaterial, OutlineAssemblyResponse, OutlineResponse, SourceUnitRef, WritingResponse,
    WritingTask, WritingUnit,
)


_LOCAL_FAILURES = (ModelWorkUnitExhausted, ModelBudgetError)
_OUTLINE_CORRECTION_TOKENS = 2048


def _json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def _accept_directory_plan(raw, documents=None, *, model=DirectoryPlan):
    softened, skipped = soften_directory_plan(raw)
    plan = _parse(model, softened)
    if skipped:
        plan['skipped_item_issues'] = skipped
    if documents is not None:
        try:
            compile_directory(plan, documents)
        except DirectoryPlanRejected as error:
            raise ModelContractError([{'path': [], 'code': 'directory_source_invalid',
                                       'detail': error.detail}]) from None
    return plan


def _accept_ref_directory_plan(raw, refs, documents, *, model=RefDirectoryPlan):
    plan = decode_plan(raw, refs, model=model)
    try:
        return _accept_directory_plan(plan, documents,
            model=ReadingDirectoryPlan if model is RefDirectoryPlanV2 else DirectoryPlan)
    except ModelContractError as error:
        errors = deepcopy(error.errors)
        for entry in errors:
            for node, selected in zip(plan['nodes'], raw.get('nodes', [])):
                entry['detail'] = entry['detail'].replace(node['id'], f'heading_ref={selected["heading_ref"]}')
        raise ModelContractError(errors) from None


def _accept_parameter_plan(raw, compiled, documents, chapter_id):
    plan = _parse(ParameterPlan, raw)
    parents = [chapter['chapter_id'] for chapter in plan['chapters']]
    if len(parents) != len(set(parents)) or any(identity != chapter_id for identity in parents):
        raise ModelContractError([{'path': ['chapters'], 'code': 'directory_parameter_scope_invalid',
                                   'detail': 'parameter plan must only select the requested parent once'}])
    checked = deepcopy(compiled)
    before = len(checked['issues'])
    apply_parameters(checked, documents, plan['chapters'])
    errors = [issue for issue in checked['issues'][before:]
              if issue['code'] in {'outline_directory_parameter_rejected', 'outline_directory_parameter_short'}]
    if errors:
        raise ModelContractError([{'path': ['chapters'], 'code': 'directory_parameter_source_invalid',
                                   'detail': issue['detail']} for issue in errors])
    try:
        accept_parameter_titles(plan)
    except DirectoryPlanRejected as error:
        raise ModelContractError([{'path': ['chapters'], 'code': 'directory_parameter_title_invalid',
                                   'detail': error.detail}]) from None
    return plan


def _accept_requirement_plan(raw, documents, block, compiled=None):
    plan = _parse(RequirementPlan, raw)
    try:
        validate_requirement_windows(documents, plan['assignments'], block)
        if compiled is not None:
            validate_requirement_exclusions(compiled, plan['assignments'])
    except DirectoryPlanRejected as error:
        raise ModelContractError([{'path': ['assignments'], 'code': 'directory_requirement_source_invalid',
                                   'detail': error.detail}]) from None
    return plan


def _accept_material_plan(raw, catalog, compiled=None, documents=None, *, response_forms_catalog=None,
                          source_group_contract=None, source_carrier_contract=None, reviewed_source_facts=()):
    closed = source_carrier_contract == SOURCE_CARRIER_CONTRACT
    grouped = closed or source_group_contract == SOURCE_GROUP_CONTRACT
    model = (MaterialClosurePlan if closed else MaterialSourceGroupPlan if grouped
             else NativeMaterialPlan if response_forms_catalog is not None else MaterialPlan)
    plan = _parse(model, raw)
    try:
        validate_table_headers(catalog, plan['table_headers'])
    except DirectoryPlanRejected as error:
        raise ModelContractError([{'path': ['table_headers'], 'code': 'directory_table_header_invalid',
                                   'detail': error.detail}]) from None
    if compiled is not None:
        try:
            chosen = deepcopy(compiled)
            apply_directory_adjudications(chosen, documents, plan['directory_adjudications'])
        except DirectoryPlanRejected as error:
            raise ModelContractError([{'path': ['directory_adjudications'],
                'code': 'directory_adjudication_invalid', 'detail': error.detail}]) from None
        try:
            validate_chapter_titles(chosen, documents, plan['chapter_titles'], title_catalog(compiled, documents))
        except DirectoryPlanRejected as error:
            raise ModelContractError([{'path': ['chapter_titles'],
                'code': 'directory_title_invalid', 'detail': error.detail}]) from None
        before = len(chosen['issues'])
        apply_materials(chosen, documents, plan['assignments'],
                        **({'source_owner': True} if grouped else {}))
        errors = [issue for issue in chosen['issues'][before:]
                  if issue['code'] == 'outline_directory_material_rejected']
        if errors:
            raise ModelContractError([{'path': ['assignments'], 'code': 'directory_material_source_invalid',
                                       'detail': issue['detail']} for issue in errors])
        try:
            validate_reference_exclusions(chosen)
        except DirectoryPlanRejected as error:
            raise ModelContractError([{'path': ['assignments'], 'code': 'directory_reference_exclusion_invalid',
                                       'detail': error.detail}]) from None
        if response_forms_catalog is not None:
            validate_response_form_selections(chosen, documents, plan['response_forms'], supplied=response_forms_catalog)
        if grouped:
            if response_forms_catalog is None and plan['response_forms']:
                raise ModelContractError([{'path': ['response_forms'], 'code': 'directory_response_form_invalid',
                    'detail': 'Response forms must be selected only from a supplied native form catalog.'}])
            try:
                if closed:
                    apply_source_groups(chosen, documents, plan['source_groups'])
                else:
                    validate_source_groups(chosen, documents, plan['source_groups'])
            except DirectoryPlanRejected as error:
                raise ModelContractError([{'path': ['source_groups'], 'code': 'directory_source_group_invalid',
                    'detail': error.detail}]) from None
        if reviewed_source_facts:
            try:
                apply_reviewed_material_facts(chosen, documents, reviewed_source_facts)
            except DirectoryPlanRejected as error:
                raise ModelContractError([{'path': [], 'code': 'reviewed_material_fact_invalid', 'detail': error.detail}]) from None
        if closed:
            apply_carrier_closure(chosen, documents, plan['carrier_dispositions'], candidate_carriers(compiled, documents))
            if reviewed_source_facts:
                bind_reviewed_carrier_results(chosen, reviewed_source_facts)
    return plan


def _accept_classification_plan(raw, compiled, documents, *, reviewed_source_facts=None):
    plan = _parse(ReviewedClassificationPlan if reviewed_source_facts else ClassificationPlan, raw)
    legacy = reviewed_classification_view(compiled, reviewed_source_facts)[0] if reviewed_source_facts else compiled
    try:
        validate_requirement_adjudications(legacy, documents, plan['requirement_adjudications'])
    except DirectoryPlanRejected as error:
        raise ModelContractError([{'path': ['requirement_adjudications'],
            'code': 'directory_requirement_adjudication_invalid', 'detail': error.detail}]) from None
    if reviewed_source_facts:
        try:
            validate_reviewed_body_dispositions(compiled, documents, plan['reviewed_body_dispositions'], reviewed_source_facts)
        except DirectoryPlanRejected as error:
            raise ModelContractError([{'path': ['reviewed_body_dispositions'], 'code': 'reviewed_body_fact_invalid', 'detail': error.detail}]) from None
    checked = deepcopy(compiled)
    before = len(checked['issues'])
    apply_classification(checked, documents, plan['items'])
    errors = [issue for issue in checked['issues'][before:]
              if issue['code'] == 'outline_directory_classification_rejected']
    if errors:
        raise ModelContractError([{'path': ['items'], 'code': 'directory_classification_source_invalid',
                                   'detail': issue['detail']} for issue in errors])
    return plan


def _parse(schema, value):
    try:
        return schema.model_validate(value).model_dump()
    except ValidationError as error:
        raise ModelContractError([{'path': list(e['loc']), 'code': e['type'], 'detail': e['msg']}
                                  for e in error.errors(include_input=False)]) from None


def _prompt(budget, task):
    return execution_prompt(budget.prompts, task)


def _request(budget, task, payload):
    prompt_name = 'outline_assemble' if task == 'outline' and payload.get('operation') == 'assemble' else task
    if task == AuthoringTask.DIRECTORY_PLAN and payload.get('operation') == 'select_bid_format_outline_v1':
        prompt_name = 'bid_format_outline'
    elif task == AuthoringTask.DIRECTORY_PLAN and payload.get('operation') in {'select_directory_refs_v1', 'select_directory_refs_v2'}:
        prompt_name = 'directory_ref_plan'
    prompt = _prompt(budget, prompt_name)
    if task == AuthoringTask.DIRECTORY_PLAN and payload.get('operation') == 'select_directory_refs_v2':
        prompt += '\n\n' + _prompt(budget, 'directory_ref_plan_v2')
    if task == AuthoringTask.DIRECTORY_MATERIAL and payload.get('operation') == 'select_response_forms_v1':
        prompt += '\n\n' + _prompt(budget, 'directory_material_response_forms')
    if task == AuthoringTask.DIRECTORY_MATERIAL and payload.get('operation') == SOURCE_GROUP_OPERATION:
        prompt += '\n\n' + _prompt(budget, 'directory_material_source_groups')
    if task == AuthoringTask.DIRECTORY_MATERIAL and payload.get('operation') == SOURCE_CARRIER_OPERATION:
        prompt += '\n\n' + _prompt(budget, 'directory_material_source_groups')
        prompt += '\n\n' + _prompt(budget, 'directory_material_closure')
    if task == AuthoringTask.DIRECTORY_MATERIAL and payload.get('identity_format') == 'material_short_ids_v3':
        prompt += ('\n目录采用column_blocks_v1：blocks按原顺序展开，constant_values为每行共有字段，'
                   'rows按columns逐值对应；全部来源和值均保留。输出仍按原Schema返回对象。')
    if task == AuthoringTask.DIRECTORY_RESPONSE_FORM and payload.get('operation') in {'select_native_response_form_v2', 'select_native_response_form_v3'}:
        prompt += '\n\n' + _prompt(budget, 'directory_response_form_v2')
    if task == AuthoringTask.DIRECTORY_RESPONSE_FORM and payload.get('operation') == 'select_native_response_form_v3':
        prompt += '\n\n' + _prompt(budget, 'directory_response_form_v3')
    if task == AuthoringTask.DIRECTORY_REQUIREMENT and payload.get('operation') == 'select_native_requirement_sources_v1':
        prompt += '\n\n' + _prompt(budget, 'directory_requirement_native')
        if payload.get('reviewed_body_refs') or payload.get('reviewed_partial_refs') or any(
                line.get('source_role') == 'reviewed_body' for page in payload.get('block', ()) for line in page['lines']):
            prompt += ('\n核定正文由分类阶段独立裁定。source_role=reviewed_body 的原行只供阅读，'
                       '不能进入assignments。reviewed_body_refs必须标为already_carried，'
                       'reviewed_partial_refs必须标为reference（宿主保留整片待核）。'
                       '两类引用均不得进入native_assignments，仍须逐个处置。')
    if task == AuthoringTask.DIRECTORY_CLASSIFICATION and payload.get('operation') == 'classify_with_native_requirement_cases_v1':
        prompt += '\n\n' + _prompt(budget, 'directory_classification_native')
        if payload.get('classification_scope') == 'cases_v1':
            prompt += ('\n本次只裁定当前 requirement_cases，每个短 ID 恰出现一次。items 必须为空。'
                       '所有章仍可选。依据只能引用本次显示的原文行，包含范围内每一行。'
                       '需要合并范围之外的续段或缺少依据时，保留该事项独立未归属。')
        elif payload.get('classification_scope') == 'document_v1':
            prompt += ('\n本次只处理文件级 items 和本次提供的已核正文事实。'
                       'requirement_adjudications 和 basis_groups 必须为空。')
        if payload.get('reviewed_source_facts'):
            if payload.get('classification_scope') != 'document_v1':
                prompt += '\n同时裁定 reviewed_source_facts，保留原生 items、basis_groups 和 requirement_adjudications。'
            prompt += ('reviewed_body_dispositions 每条事实恰出现一次，'
                       '只返回 fact_id（短 fN）、scope 和 chapter_id（短 cN 或 null）。'
                       'scope=chapter 时选一个真实职责、主体和提交实例相容的已有章节。'
                       '无法确认时 scope=unassigned 且 chapter_id=null，保留具名待核。'
                       'body 是核定正文权威，context 与原件坐标保留出处。'
                       '独有续段不得删除；共享片段或相同 entry 的不同事实不得合并。'
                       'manual_only 的案件须独立留待核，不可作为响应正文。不要誊写原文。')
    if task == AuthoringTask.DIRECTORY_CLASSIFICATION and payload.get('operation') == REVIEWED_CLASSIFICATION_OPERATION:
        prompt += '\n\n' + _prompt(budget, 'directory_classification_reviewed_facts')
    return build_request(budget.model, prompt, payload, task=task,
                         reasoning_effort=budget.reasoning_effort)


def _fits(budget, task, payload, *, headroom_tokens=0):
    try:
        measured = budget.require_request(_request(budget, task, payload), task)
    except ModelBudgetError:
        return False
    return measured['input_tokens'] + headroom_tokens <= measured['effective_input_budget_tokens']


def _directory_chapters(compiled):
    return [{'id': chapter['id'], 'title': chapter.get('display_title', chapter['title']), 'parent_id': chapter.get('parent_id'),
                 'response_kind': chapter.get('response_kind', 'form'), 'subjects': chapter.get('subjects', [])}
                for chapter in compiled['chapters'] if not chapter.get('unplaced')]


def _directory_applicability(compiled):
    value = {'excluded': [deepcopy(issue) for issue in compiled['issues'] if issue['code'] == 'outline_directory_excluded'],
                     'conditions': [{'chapter_id': chapter['id'], 'text': chapter['condition']}
                                    for chapter in compiled['chapters'] if chapter.get('condition')]}
    exclusions = reference_exclusions(compiled)
    if exclusions:
        value['source_reference_exclusions'] = exclusions
    return value


def _full_source_reading(reading):
    value = deepcopy(reading)
    value['source_line_format'] = 'numbered_line_pairs_v1'
    for document in value['documents']:
        for page in document['pages']:
            page['lines'] = [[entry['line'], entry['text']] for entry in page['lines']]
    return value


async def _request_directory_details(compiled, documents, budget, execute, identity=None, *, artifacts=None,
                                     response_form_contract=None, parameter_source_contract=None, source_group_contract=None,
                                     native_requirement_contract=None, source_carrier_contract=None,
                                     reviewed_source_facts=(), on_source_closure=None, reviewed_body_entry_contract=None,
                                     native_requirement_concurrency=2):
    from docreader.tender.native_requirement_sources import CONTRACT as NATIVE_CONTRACT
    if reviewed_body_entry_contract not in (None, REVIEWED_BODY_ENTRY_CONTRACT):
        raise ModelConfigurationError('unsupported reviewed body entry contract')
    entry_options = {'entry_contract': reviewed_body_entry_contract} if reviewed_body_entry_contract is not None else {}
    if native_requirement_contract not in (None, NATIVE_CONTRACT):
        raise ModelConfigurationError('unsupported native requirement source contract')
    if source_group_contract not in (None, SOURCE_GROUP_CONTRACT):
        raise ModelConfigurationError('unsupported directory source group contract')
    if source_carrier_contract not in (None, SOURCE_CARRIER_CONTRACT):
        raise ModelConfigurationError('unsupported directory source carrier contract')
    if reviewed_source_facts and source_carrier_contract != SOURCE_CARRIER_CONTRACT:
        raise ModelConfigurationError('reviewed source facts require source_carriers_closed_v1')
    if reviewed_source_facts:
        compiled['_reviewed_required_native_table_ids'] = sorted(f['_table_id'] for f in reviewed_source_facts if f['kind'] == 'required_native')
    reviewed_body_facts = tuple(f for f in reviewed_source_facts if f['kind'] == 'independent_body_response')
    closed = source_carrier_contract == SOURCE_CARRIER_CONTRACT
    grouped = closed or source_group_contract == SOURCE_GROUP_CONTRACT
    if response_form_contract not in (None, *RESPONSE_FORM_CONTRACTS):
        raise ModelConfigurationError('unsupported response form source contract')
    if parameter_source_contract not in (None, PARAMETER_SOURCE_CONTRACT):
        raise ModelConfigurationError('unsupported parameter source contract')
    if parameter_source_contract == PARAMETER_SOURCE_CONTRACT:
        compiled['_parameter_source_contract'] = parameter_source_contract
    forms_catalog = None
    if response_form_contract in RESPONSE_FORM_CONTRACTS:
        compiled['_response_form_contract'] = response_form_contract
        forms_catalog = response_form_catalog(compiled, documents)
    compiled['issues'].extend(sparse_page_issues(documents))
    chapters = _directory_chapters(compiled)
    applicability = _directory_applicability(compiled)
    reading = reading_payload(documents, identity)
    catalog = table_header_catalog(compiled, documents)
    choices = directory_cases(compiled, documents)
    titles = title_catalog(compiled, documents)
    material_options = ({'operation': 'select_response_forms_v1', 'response_form_catalog': forms_catalog}
                        if forms_catalog is not None else {})
    if grouped:
        material_options.update(operation=SOURCE_GROUP_OPERATION, source_group_candidates=source_group_candidates(compiled))
    carriers = candidate_carriers(compiled, documents) if closed else []
    if closed:
        material_options.update(operation=SOURCE_CARRIER_OPERATION, source_carrier_catalog=source_carrier_catalog(
            carriers, required_native_ids=compiled.get('_reviewed_required_native_table_ids', [])))
    material_facts = reviewed_material_summary(reviewed_source_facts)
    if material_facts:
        material_options['reviewed_source_facts'] = material_facts
    materials = None
    try:
        materials = await _ask(execute, budget, AuthoringTask.DIRECTORY_MATERIAL,
                               {**_full_source_reading(reading), 'chapters': chapters, 'applicability': applicability,
                                'unplaced_tables': unplaced_tables(compiled, documents), 'table_header_catalog': catalog,
                                'chapter_title_catalog': titles,
                                **({'directory_cases': choices} if choices else {}),
                                **material_options},
                               lambda raw: _accept_material_plan(raw, catalog, compiled, documents,
                                    response_forms_catalog=forms_catalog, source_group_contract=source_group_contract,
                                    source_carrier_contract=source_carrier_contract, reviewed_source_facts=reviewed_source_facts))
    except _LOCAL_FAILURES:
        compiled['issues'].append({'code': 'outline_directory_material_rejected', 'detail': '章内材料未完成，其余继续'})
        if reviewed_source_facts:
            apply_reviewed_material_facts(compiled, documents, reviewed_source_facts)
        if closed:
            present = {identity for chapter in compiled['chapters'] if not chapter.get('unplaced')
                       for identity in native_parts(chapter)}
            apply_carrier_closure(compiled, documents,
                [{'table_id': item['table_id'], 'action': 'unresolved'} for item in carriers
                 if item['table_id'] not in present], carriers)
    else:
        apply_directory_adjudications(compiled, documents, materials['directory_adjudications'])
        apply_chapter_titles(compiled, documents, materials['chapter_titles'], titles)
        apply_materials(compiled, documents, materials['assignments'],
                        **({'source_owner': True} if grouped else {}))
        if grouped:
            apply_source_groups(compiled, documents, materials['source_groups'])
        if reviewed_source_facts:
            apply_reviewed_material_facts(compiled, documents, reviewed_source_facts)
        if closed:
            apply_carrier_closure(compiled, documents, materials['carrier_dispositions'], carriers)
    if reviewed_source_facts and closed:
        bind_reviewed_carrier_results(compiled, reviewed_source_facts)
    if on_source_closure is not None and closed:
        await on_source_closure(deepcopy(compiled.get('_source_carrier_closure', [])))
    apply_table_headers(compiled, documents, catalog, materials.get('table_headers', []) if materials else [],
                        failure='材料请求未完成，请对照原件核定' if materials is None else None)
    chapters = _directory_chapters(compiled)
    applicability = _directory_applicability(compiled)

    async def requirements():
        requirement_chapters = deepcopy(chapters)
        originals = {chapter['id']: chapter for chapter in compiled['chapters']}
        for chapter in requirement_chapters:
            original = originals[chapter['id']]
            if chapter['response_kind'] in {'form', 'attachment'} and original.get('text', '').strip():
                chapter['source_body'] = original['text']
            if original.get('obligations', '').strip():
                chapter['source_obligations'] = original['obligations']
        blocks = list(enumerate(requirement_blocks(documents), start=1))
        semaphore = asyncio.Semaphore(2)

        async def one(block):
            async with semaphore:
                try:
                    return await _ask(execute, budget, AuthoringTask.DIRECTORY_REQUIREMENT,
                                      {'chapters': requirement_chapters, 'block': block, 'applicability': applicability, **(
                                          {'source_version': identity.get('source_version'),
                                           'domain_revision': identity.get('domain_revision')} if identity else {})},
                                      lambda raw: _accept_requirement_plan(raw, documents, block, compiled))
                except _LOCAL_FAILURES:
                    return None

        results = await asyncio.gather(*(one(block) for _, block in blocks))
        for (index, _), plan in zip(blocks, results):
            if plan is None:
                compiled['issues'].append({'code': 'outline_directory_requirement_rejected',
                                           'detail': f'要求分块{index}未完成，其余继续'})
            else:
                apply_requirements(compiled, documents, plan['assignments'])

    if native_requirement_contract == NATIVE_CONTRACT:
        await _request_native_requirements(compiled, documents, budget, execute, identity, artifacts, chapters, applicability,
            reviewed_body_facts=reviewed_body_facts, reviewed_body_entry_contract=reviewed_body_entry_contract,
            concurrency=native_requirement_concurrency)
    else:
        await requirements()
    classification_view = reviewed_classification_view(compiled, reviewed_body_facts)[0] if reviewed_body_facts else compiled
    cases = requirement_cases(classification_view, documents)
    if native_requirement_contract != NATIVE_CONTRACT:
        classification_payload = {**_full_source_reading(reading), **({'requirement_cases': cases, 'chapters': chapters,
                                                 'applicability': applicability} if cases else {})}
        if reviewed_body_facts:
            classification_payload.update(operation=REVIEWED_CLASSIFICATION_OPERATION,
                chapters=chapters, applicability=applicability,
                reviewed_source_facts=reviewed_body_summary(reviewed_body_facts))
        try:
            classification_plan = await _ask(execute, budget, AuthoringTask.DIRECTORY_CLASSIFICATION,
                classification_payload, lambda raw: _accept_classification_plan(raw, compiled, documents, reviewed_source_facts=reviewed_body_facts))
        except _LOCAL_FAILURES:
            classification_plan = None
        if classification_plan is None:
            compiled['issues'].append({'code': 'outline_directory_classification_rejected', 'detail': '分类未完成，其余继续'})
        else:
            if reviewed_body_facts:
                apply_reviewed_classification(compiled, documents, classification_plan, reviewed_body_facts, **entry_options)
            else:
                apply_classification(compiled, documents, classification_plan['items'])
                apply_requirement_adjudications(compiled, documents, classification_plan['requirement_adjudications'])
        if reviewed_body_facts:
            if classification_plan is None:
                if reviewed_body_entry_contract == REVIEWED_BODY_ENTRY_CONTRACT:
                    apply_reviewed_classification(compiled, documents, None, reviewed_body_facts, **entry_options)
                else:
                    apply_reviewed_body_dispositions(compiled, documents, [
                        {'fact_id': f['fact_id'], 'scope': 'unassigned', 'chapter_id': None}
                        for f in reviewed_body_facts], reviewed_body_facts, **entry_options)
    long_chapters = [chapter for chapter in compiled['chapters']
                   if not chapter.get('unplaced')
                   and chapter.get('response_kind', 'form') == 'narrative'
                   and len([line for line in chapter.get('text', '').splitlines() if line.strip()]) >= 25]
    for chapter in long_chapters:
        try:
            plan = await _ask(execute, budget, AuthoringTask.DIRECTORY_PARAMETER,
                              {'chapter': {'id': chapter['id'], 'title': chapter.get('display_title', chapter['title']),
                                           'lines': parameter_view(chapter, documents)}},
                              lambda raw: _accept_parameter_plan(raw, compiled, documents, chapter['id']))
        except _LOCAL_FAILURES:
            compiled['issues'].append({'code': 'outline_directory_parameter_rejected',
                                       'detail': f'“{chapter["title"]}”的参数展开未完成，原章保留'})
            continue
        apply_parameters(compiled, documents, plan['chapters'])
    for selection in (materials or {}).get('response_forms', []):
        try:
            if artifacts is None or identity is None:
                raise ValueError('native response form sources are unavailable')
            pending = pending_response_form(artifacts, compiled, documents, selection, identity)
            compiled.setdefault('_response_form_pending', []).append(pending)
            view = ResponseFormView(artifacts, compiled, documents, selection, identity, contract=response_form_contract)
        except (ModelContractError, OSError, ValueError, KeyError):
            compiled['issues'].append({'code': 'outline_directory_response_form_unconfirmed',
                'detail': '逐条响应原式的完整来源范围暂不能核定，原表及已选来源保留，请核定后展开。'})
            continue
        try:
            form_plan = await _ask(execute, budget, AuthoringTask.DIRECTORY_RESPONSE_FORM, view.payload, view.accept)
        except _LOCAL_FAILURES:
            compiled['issues'].append({'code': 'outline_directory_response_form_unconfirmed',
                'detail': '逐条响应原式尚未完成有界来源选择，原表保留；请按其完整来源范围人工列条款或修订后重试。'})
            continue
        compiled.setdefault('_response_form_plans', []).append(form_plan)
        if not form_plan['complete']:
            pending['pending']['reason'] = 'selection_incomplete'
            compiled['issues'].append({'code': 'outline_directory_response_form_unconfirmed',
                'detail': '逐条响应原式仍有未决来源或布局，未把部分条款写成完整表；原表保留，须核定完整范围后展开。'})
    return note_missing_requirements(compiled)


async def _request_native_requirements(compiled, documents, budget, execute, identity, artifacts, chapters, applicability,
                                       *, reviewed_body_facts=(), reviewed_body_entry_contract=None, concurrency=2):
    if concurrency < 1:
        raise ValueError('native requirement concurrency must be positive')
    from docreader.tender.native_requirement_sources import SourceLedger, plan_native_packs, require_native_budget
    from docreader.tender.native_requirement_flow import JointRequirements, JointClassification
    requirement_chapters = deepcopy(chapters)
    originals = {chapter['id']: chapter for chapter in compiled['chapters']}
    for chapter in requirement_chapters:
        original = originals[chapter['id']]
        if chapter['response_kind'] in {'form', 'attachment'} and original.get('text', '').strip():
            chapter['source_body'] = original['text']
        if original.get('obligations', '').strip():
            chapter['source_obligations'] = original['obligations']
    try:
        if artifacts is None or not identity or not identity.get('frozen_ref'):
            raise ValueError('Frozen native requirement sources are unavailable.')

        def build_joint():
            frozen = artifacts.read_json(identity['frozen_ref'])
            ledger = SourceLedger(artifacts, frozen['documents'], chapters=requirement_chapters)
            from docreader.tender.directory_compile import _used_lines
            joint = JointRequirements(ledger, documents, applicability, identity, carried_points=_used_lines(compiled),
                                      reviewed_body_facts=reviewed_body_facts)
            return ledger, joint

        ledger, joint = await asyncio.to_thread(build_joint)
    except (OSError, ValueError, KeyError, ModelContractError) as error:
        compiled['issues'].append({'code': 'outline_directory_native_source_unavailable',
            'detail': '完整原生要求来源暂不可核定；原件保留，请在Word对照完整原件人工核定正文及归属。' + type(error).__name__})
        if reviewed_body_facts:
            apply_reviewed_classification(compiled, documents, None, reviewed_body_facts,
                                          entry_contract=reviewed_body_entry_contract)
        return
    task = AuthoringTask.DIRECTORY_REQUIREMENT
    packs = await asyncio.to_thread(plan_native_packs, ledger, budget, joint.payload,
        _prompt(budget, task) + '\n\n' + _prompt(budget, 'directory_requirement_native'),
        request_factory=lambda view, payload: _request(budget, task, payload))
    for pending in packs['pending_ranges']:
        joint.pending(compiled, pending['range_ordinals'], pending['reason'])
    async def request_native_pack(view):
        payload = joint.payload(view)
        try:
            require_native_budget(budget, view, payload, '', request=_request(budget, task, payload))
            return await _ask(execute, budget, task, payload, lambda raw: joint.accept(view, raw, compiled))
        except _LOCAL_FAILURES:
            return None

    views = packs['views']
    for offset in range(0, len(views), concurrency):
        batch = views[offset:offset + concurrency]
        tasks = [asyncio.create_task(request_native_pack(view)) for view in batch]
        try:
            results = await asyncio.gather(*tasks)
        except BaseException:
            for pending_task in tasks:
                pending_task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            raise
        # Validation only reads the directory guards. Apply in inventory order,
        # because assigning a range can change which later ranges are already used.
        for view, prepared in zip(batch, results, strict=True):
            if prepared is None:
                joint.pending(compiled, view.range_ordinals, 'joint_B_unfinished')
            else:
                joint.apply(compiled, prepared)

    def build_classification():
        joint.unknown_geometry(compiled)
        joint.reviewed_partial_geometry(compiled)
        return JointClassification(ledger, compiled, documents, proof=joint.proof,
            reviewed_body_facts=reviewed_body_facts, reviewed_body_entry_contract=reviewed_body_entry_contract)

    classification = await asyncio.to_thread(build_classification)
    await _request_joint_classification(compiled, classification, budget, execute,
                                        {'applicability': applicability, **joint.identity})


async def _request_joint_classification(compiled, classification, budget, execute, context):
    from docreader.tender.native_requirement_flow import require_classification_budget

    task = AuthoringTask.DIRECTORY_CLASSIFICATION

    def admit(payload):
        payload = {**payload, **context}
        measured = require_classification_budget(budget, _request(budget, task, payload), payload)
        return payload, {key: value for key, value in measured.items() if key != 'request'}

    try:
        payload, measured = await asyncio.to_thread(admit, classification.payload)
    except ModelBudgetError:
        pass
    else:
        compiled['_native_classification_budget'] = measured
        try:
            prepared = await _ask(execute, budget, task, payload, classification.accept)
        except _LOCAL_FAILURES:
            compiled['issues'].append({'code': 'outline_directory_classification_rejected',
                'detail': '完整联合分类未完成或超出完整输入/输出预算；全部精确来源保留，请核定具名未决来源后修订。'})
            await asyncio.to_thread(classification.fallback, compiled)
        else:
            await asyncio.to_thread(classification.apply, compiled, prepared)
        return

    scopes, budgets = [], []
    held = 0
    document_completed = False

    async def request_scope(payload, measured):
        budgets.append(measured)
        try:
            prepared = await _ask(execute, budget, task, payload,
                                  lambda raw: classification.accept_scope(payload, raw))
        except _LOCAL_FAILURES:
            return False
        scopes.append(prepared)
        return True

    try:
        document = await asyncio.to_thread(lambda: admit(classification.scope_payload()))
    except ModelBudgetError:
        pass
    else:
        document_completed = await request_scope(*document)

    def admit_cases(groups):
        aliases = [alias for group in groups for alias in group]
        try:
            return admit(classification.scope_payload(aliases))
        except ModelBudgetError:
            return None

    groups = await asyncio.to_thread(classification.scoped_groups)
    start = 0
    while start < len(groups):
        def next_scope(offset):
            admitted = admit_cases(groups[offset:offset + 1])
            if admitted is None:
                return None, 1
            size, rejected = 1, None
            remaining = len(groups) - offset
            while size < remaining:
                probe = min(size * 2, remaining)
                candidate = admit_cases(groups[offset:offset + probe])
                if candidate is None:
                    rejected = probe
                    break
                size, admitted = probe, candidate
            while rejected is not None and rejected - size > 1:
                probe = (size + rejected) // 2
                candidate = admit_cases(groups[offset:offset + probe])
                if candidate is None:
                    rejected = probe
                else:
                    size, admitted = probe, candidate
            return admitted, size

        admitted, size = await asyncio.to_thread(next_scope, start)
        if admitted is None:
            held += len(groups[start])
            start += 1
            continue
        if not await request_scope(*admitted):
            held += len(admitted[0]['requirement_cases'])
        start += size

    compiled['_native_classification_budget'] = {'mode': 'scoped_v1', 'scopes': budgets,
        'document_completed': document_completed, 'held_case_count': held}
    if scopes:
        try:
            prepared = await asyncio.to_thread(classification.merge_scopes, scopes)
        except ModelContractError:
            scopes.clear()
        else:
            await asyncio.to_thread(classification.apply, compiled, prepared)
    if not scopes:
        await asyncio.to_thread(classification.fallback, compiled)
    if not scopes or held or not document_completed:
        compiled['issues'].append({'code': 'outline_directory_classification_rejected',
            'detail': '部分来源分类待核。请根据具名未决事项核对完整原文、适用性和所属章节。'})


async def _request_directory_plans(documents, budget, execute, identity=None, *, contract=None):
    if contract not in (None, 'source_closure_business_titles_v10', LEGACY_DIRECTORY_REF_CONTRACT, DIRECTORY_REF_CONTRACT):
        raise ModelConfigurationError('unsupported directory planning contract')
    refs = None
    if contract in {LEGACY_DIRECTORY_REF_CONTRACT, DIRECTORY_REF_CONTRACT}:
        refs = SourceRefs(documents, identity)
        payload = {**refs.reading(), 'operation': ('select_directory_refs_v2' if contract == DIRECTORY_REF_CONTRACT
                                                 else 'select_directory_refs_v1')}
    else:
        payload = _full_source_reading(reading_payload(documents, identity))

    def accept(raw):
        return (_accept_ref_directory_plan(raw, refs, documents,
                    model=RefDirectoryPlanV2 if contract == DIRECTORY_REF_CONTRACT else RefDirectoryPlan) if refs is not None
                else _accept_directory_plan(raw, documents))

    plans, failures = [], []
    for slot in (1, 2):
        try:
            if contract == DIRECTORY_REF_CONTRACT:
                # A valid v1 response is a strict subset of the new reading-ref
                # contract. Reuse only its exact original request and revalidate;
                # rejected or merely similar old replies never become successes.
                legacy = {**payload, 'operation': 'select_directory_refs_v1', 'plan_slot': slot}
                hit, value = await _replay(execute, AuthoringTask.DIRECTORY_PLAN,
                    _request(budget, AuthoringTask.DIRECTORY_PLAN, legacy),
                    lambda raw: _accept_ref_directory_plan(raw, refs, documents))
                if hit:
                    plans.append(value)
                    continue
            plans.append(await _ask(execute, budget, AuthoringTask.DIRECTORY_PLAN,
                                    {**payload, 'plan_slot': slot}, accept))
        except _LOCAL_FAILURES as error:
            failures.append(f'{type(error).__name__}: {error}')
    return plans, failures


async def _ask(execute, budget, task, payload, validate):
    request = _request(budget, task, payload)
    hit, value = await _replay(execute, task, request, validate)
    if hit:
        return value
    if (task == AuthoringTask.DIRECTORY_MATERIAL or
            (task == AuthoringTask.DIRECTORY_CLASSIFICATION and payload.get('operation') == REVIEWED_CLASSIFICATION_OPERATION)):
        from docreader.tender.directory_material_ids import MATERIAL_IDENTITY_CONTRACT, MaterialIds

        source_validate = validate
        identities = MaterialIds(payload)
        request = _request(budget, task, identities.payload)
        validate = identities.accept(validate)
        hit, value = await _replay(execute, task, request, validate)
        if hit:
            return value
        if (task == AuthoringTask.DIRECTORY_MATERIAL
                and (getattr(execute, 'spec', None) or {}).get('directory_material_identity_contract') == MATERIAL_IDENTITY_CONTRACT
                and not await asyncio.to_thread(_fits, budget, task, identities.payload)):
            identities = MaterialIds(payload, contract=MATERIAL_IDENTITY_CONTRACT)
            request = _request(budget, task, identities.payload)
            validate = identities.accept(source_validate)
            hit, value = await _replay(execute, task, request, validate)
            if hit:
                return value
    if (task == 'outline' and payload.get('operation') == 'assemble'
            and payload.get('assembly_view') != 'compact_v1'):
        if not await asyncio.to_thread(_fits, budget, task, payload):
            from docreader.tender.authoring_view import compact_assembly_payload
            compact = await asyncio.to_thread(compact_assembly_payload, payload)
            request = _request(budget, task, compact)
            hit, value = await _replay(execute, task, request, validate)
            if hit:
                return value
    if task == AuthoringTask.DIRECTORY_RESPONSE_FORM:
        require_response_form_budget(budget, request, payload)
    budget.admit_request(request, task)
    return await execute(task, request, validate)


async def _replay(execute, task, request, validate):
    replay = getattr(execute, 'replay', None)
    return await replay(task, request, validate) if replay is not None else (False, None)


def _batches(records, budget, task, payload, *, headroom_tokens=0):
    """Greedy packing at record boundaries; never truncate an indivisible record."""
    batch = []
    for record in records:
        if batch and not _fits(budget, task, payload([*batch, record]), headroom_tokens=headroom_tokens):
            yield batch
            batch = []
        batch.append(record)
    if batch:
        yield batch


def _note_unassigned_rules(outline, context):
    assigned = {rule_ref_key(entry['rule_ref']) for entry in build_ledger(outline)
                if entry['rule_ref'] is not None and entry['resolution'] != 'pending'}
    existing, issues = set(), []
    for issue in outline.get('issues', []):
        if issue.get('code') != 'outline_rule_unassigned' or not issue.get('rule_refs'):
            issues.append(issue)
            continue
        remaining = [ref for ref in issue['rule_refs'] if rule_ref_key(ref) not in assigned]
        if remaining:
            issues.append({**issue, 'rule_refs': remaining})
            existing.update(rule_ref_key(ref) for ref in remaining)
    for ref in context.rules.values():
        identity = rule_ref_key(ref)
        if identity in assigned or identity in existing:
            continue
        issues.append({
            'code': 'outline_rule_unassigned', 'detail': '该有效规则尚未安排响应位置，需补充大纲映射。',
            'rule_refs': [ref]})
    outline['issues'] = issues


def _gap(code, detail, task=None):
    result = {'code': code, 'detail': detail}
    if task:
        result.update(task_id=task['task_id'], chapter_id=task['chapter_id'])
        if len(task['targets']) == 1:
            result['target_id'] = task['targets'][0]['target_id']
    return result


def prepare_materials(artifacts, materials):
    result, issues, seen = [], [], {}
    for raw in materials:
        material = KnowledgeMaterial.model_validate(raw).model_dump()
        if material['material_id'] in seen:
            if material != seen[material['material_id']]:
                raise ValueError('material_id identifies different material content')
            continue
        seen[material['material_id']] = deepcopy(material)
        # Text snapshots and original files have deliberately different roles.
        material['content_ref'] = artifacts.put_bytes(material['text'].encode(), 'text/plain')
        if material['original_ref'] is not None:
            try:
                artifacts.path(material['original_ref'])
            except (OSError, ValueError):
                material['original_ref'] = None
                issues.append(_gap('material_original_unavailable', f"资料原件不可读：{material['title']}。"))
        if material['attachment'] and material['original_ref'] is None:
            material['attachment'] = False
            issues.append(_gap('material_original_missing', f"资料仅有文字片段，不能作为证明原件：{material['title']}。"))
        if material['attachment'] and not material['text'].strip():
            issues.append(_gap('material_evidence_unreadable', f"证明没有可判断的文字依据，保留待核位置：{material['title']}。"))
        result.append(material)
    return result, issues


def _material_views(materials, targets=()):
    result = []
    for ordinal, material in enumerate(materials, 1):
        view = {key: material[key] for key in ('title', 'text', 'usage', 'attachment')}
        view.update(support_id=f'k{ordinal}', origin='knowledge')
        view['locator'] = {key: material['locator'][key] for key in (
            'product_id', 'version_id', 'version_label', 'document_id', 'is_current') if key in material['locator']}
        if targets:
            view['target_ids'] = [f't{i + 1}' for i, target in enumerate(targets)
                                  if _material_applies(material, target)]
        result.append(view)
    return result


def _material_applies(material, target):
    scope = material.get('locator', {}).get('target_ids')
    return (not target.get('manual_fill', False)
            and 'knowledge' in target.get('support_origins', ['knowledge'])
            and (not scope or target['target_id'] in scope)
            and bool(material.get('text', '').strip()))


def _excerpt_texts(artifacts, outline):
    texts = {}
    for target in _walk_targets(outline):
        table = target.get('response_table') or {}
        for row in [target, {'source_excerpts': list(response_table_excerpts(table))}]:
            for ref in row.get('source_excerpts', []):
                key = _json(ref)
                if key in texts:
                    continue
                try:
                    loaded = load_source_excerpt(artifacts, ref)
                except (OSError, ValueError, KeyError):
                    texts[key] = ''
                    continue
                text = loaded.get('text') if isinstance(loaded, dict) else ''
                if not text and isinstance(loaded, dict):
                    text = (loaded.get('unit') or {}).get('text') or ''
                texts[key] = text or ''
    return texts


def _walk_targets(outline):
    def walk(chapters):
        for chapter in chapters or []:
            yield from chapter.get('targets', [])
            yield from walk(chapter.get('children', []))
    return list(walk(outline.get('chapters', [])))


def _knowledge_queries(outline, tasks, gaps, requirements=None, excerpt_texts=None, scope=None):
    entries = build_ledger(outline, requirements)
    planned = plan_queries(outline, tasks, entries, requirements, excerpt_texts, scope)
    gaps.extend(planned['gaps'])
    return planned['search']


def _retrieval_groups(materials, task):
    groups = {}
    for material in _task_materials(materials, task):
        retrievals = material.get('locator', {}).get('retrievals') or []
        if not retrievals:
            return {}
        for item in retrievals:
            if not isinstance(item, dict) or type(item.get('rank')) not in {int, float}:
                return {}
            groups.setdefault(item.get('query_id') or 'query', []).append((item['rank'], material['material_id']))
    return {key: [identity for _, identity in sorted(values)] for key, values in groups.items()}


def _task_materials(materials, task):
    selected = task.get('_selected_material_ids')
    selected = set(selected) if selected is not None else None
    return [m for m in materials if (selected is None or m['material_id'] in selected)
            and any(_material_applies(m, t) for t in task['targets'])]


def _support_bindings(sources, materials):
    return {**{identity: {'origin': 'tender', 'source_ref': ref.get('source_ref', ref),
                         **({'source_excerpt': ref} if 'selectors' in ref else {})}
               for identity, ref in sources.items()},
            **{f'k{i}': {'origin': 'knowledge', 'material_id': m['material_id']}
               for i, m in enumerate(materials, 1)}}


def _writing_input_digest(task, payload, sources, materials, budget):
    return hashlib.sha256(_json({'request': _request(budget, 'authoring_write', payload),
        'task': task, 'bindings': _support_bindings(sources, materials),
        'contract': AUTHORING_UNIT_BINDING_CONTRACT}).encode()).hexdigest()


def _plan_input_digest(snapshot, outline_ref, materials_ref, basis_ref, budget):
    return hashlib.sha256(_json({'snapshot': snapshot, 'outline_ref': outline_ref,
        'materials_ref': materials_ref, 'basis_ref': basis_ref, 'budget': budget.spec(),
        'write_prompt': _prompt(budget, 'authoring_write'),
        'contract': AUTHORING_UNIT_BINDING_CONTRACT}).encode()).hexdigest()


def _has_allowed_support(payload):
    targets = {t['target_id']: t for t in payload['targets'] if not t.get('manual_fill', False)}
    response_targets = {row['target_id'] for row in payload.get('response_rows', [])}
    return any((s.get('text', '').strip() or s.get('grid')) and
               any(identity in targets and s['origin'] in targets[identity].get('support_origins', ['knowledge'])
                   and (identity not in response_targets or s['origin'] == 'knowledge')
                   for identity in s.get('target_ids', [])) for s in payload['supports'])


def _source_records(artifacts, context):
    """Read every frozen range once for organization, including non-item background."""
    records, included_rules = [], set()
    repository = getattr(context, 'repository', None) or EvidenceRepository(artifacts)
    rule_views = context.payload['rules']
    index_by_range = {}
    for rule in rule_views:
        semantic = rule.get('item', rule.get('effective_rule', {}))
        for evidence in semantic.get('evidence', []):
            for ref in context.sources.get(evidence['range_id'], []):
                index_by_range.setdefault((ref['source_index_ref']['sha256'], ref['unit_id']), []).append(rule)
    if context.input['frozen_ref']:
        try:
            frozen = artifacts.read_json(context.input['frozen_ref'])
        except (OSError, ValueError):
            frozen = {}
        for document in frozen.get('documents', []):
            try:
                index = repository.source_index(document['source_index_ref'])
            except (OSError, ValueError):
                continue  # The input/context already records this unavailable source.
            for source in index.ranges:
                related = []
                for ref in context.sources.get(source['range_id'], []):
                    related.extend(index_by_range.get((ref['source_index_ref']['sha256'], ref['unit_id']), []))
                related = list({r['rule_id']: r for r in related if r['rule_id'] not in included_rules}.values())
                included_rules.update(r['rule_id'] for r in related)
                records.append({'source': {'range_id': source['range_id'], 'text': source['text'],
                    'heading_path': source.get('heading_path', ''), 'kind': source['kind'],
                    'role': document['role']}, 'rules': related})
    records.extend({'rules': [r]} for r in rule_views if r['rule_id'] not in included_rules)
    return records


def _analysis_context(context, rules, source_ids=(), explicit_issue_ids=()):
    """Route existing semantic observations by identity, without reinterpreting them."""
    item_ids, ranges = set(), set(source_ids)
    for rule in rules:
        semantic = rule.get('item', rule.get('effective_rule', {}))
        identity = rule.get('item_id') or semantic.get('item_id')
        if identity:
            item_ids.add(identity)
        ranges.update(e['range_id'] for e in semantic.get('evidence', []))
    # A table unit may contain many independent source rows. Only a fragment's
    # canonical range, not every row in its native table, shares its issue scope.
    roots = getattr(context, 'range_roots', {})
    original_ranges = set(ranges)
    ranges.update(roots.get(identity, identity) for identity in original_ranges)
    ranges.update(identity for identity, root in roots.items() if root in original_ranges)
    relations = [r for r in context.payload.get('relations', [])
                 if r.get('from_item_id') in item_ids or r.get('to_item_id') in item_ids
                 or any(e['range_id'] in ranges for e in r.get('evidence', []))]
    issues = [i for i in context.payload.get('issues', [])
              if i.get('range_id') in ranges or set(i.get('item_ids') or []) & item_ids
              or i.get('issue_id') in explicit_issue_ids]
    issue_ids = {i.get('issue_id') for i in issues}
    resolutions = context.payload.get('issue_resolutions') or {}
    resolutions = resolutions.get('resolutions', []) if isinstance(resolutions, dict) else resolutions
    related_ids = {r[key] for r in relations for key in ('from_item_id', 'to_item_id')} - item_ids
    related_rules = [{k: v for k, v in r.items() if k != 'rule_id'} for r in context.payload['rules']
                     if (r.get('item_id') or r.get('item', {}).get('item_id')) in related_ids]
    return {'relations': relations, 'related_rules': related_rules, 'analysis_issues': issues,
            'unverified_relation_ids': [r['relation_id'] for r in relations
                                       if r['relation_id'] in context.payload.get('unverified_relation_ids', [])],
            'issue_resolutions': [r for r in resolutions if r['issue_id'] in issue_ids]}


def _record_sources(record):
    return record.get('sources', [record['source']] if 'source' in record else [])


def _outline_payload(context, records, template_parts=()):
    rules = list({r['rule_id']: r for entry in records for r in entry.get('rules', [])}.values())
    sources = [source for entry in records for source in _record_sources(entry)]
    visible = {source['range_id'] for source in sources}
    sections, projected = project_source_sections(sources)
    return {'schema_version': 2, 'grounding_policy': AUTHORING_GROUNDING_POLICY, 'rules': rules,
            'sources': projected, 'sections': sections,
            'template_parts': [part for part in template_parts if set(part['range_ids']) <= visible],
            'rule_origin': context.payload.get('rule_origin', 'extraction'),
            'input_issues': context.input['issues'],
            'scope_decisions': context.payload.get('scope_decisions', []),
            **_analysis_context(context, rules, [s['range_id'] for s in sources])}


def _constraint_context(context, refs, submission_refs=(), *, source_bindings=None, excerpt_bindings=None):
    """Only model-selected submission rules and their real source evidence."""
    views = {rule['rule_id']: rule for rule in context.payload['rules']}
    by_ref = {rule_ref_key(ref): views[identity] for identity, ref in context.rules.items()}
    rules, sources, used = [], [], {}
    def source(text, identity, references=(), excerpts=()):
        if not text.strip():
            return None
        key = (identity, text)
        if key not in used:
            used[key] = f's{len(sources) + 1}'
            sources.append({'source_id': used[key], 'text': text})
            if source_bindings is not None:
                if references:
                    source_bindings[used[key]] = list({_json(ref): deepcopy(ref) for ref in references}.values())
                else:
                    sources[-1]['bindable'] = False
            if excerpt_bindings is not None:
                excerpt_bindings[used[key]] = list(excerpts)
        return used[key]
    for ref in refs:
        rule = by_ref.get(rule_ref_key(ref))
        if rule is None:
            continue
        semantic = rule.get('item', rule.get('effective_rule', {}))
        supports = [source(e.get('quote', ''), e.get('range_id'), context.sources.get(e.get('range_id'), []),
                           getattr(context, 'excerpts', {}).get(e.get('range_id'), []))
                    for e in semantic.get('evidence', [])]
        rules.append({'rule_id': f'e{len(rules) + 1}',
                      **{field: semantic[field] for field in ('text', 'kind', 'subject', 'scope',
                          'conditions', 'exceptions') if field in semantic},
                      'source_ids': [identity for identity in supports if identity]})
    artifacts = getattr(context, 'artifacts', None)
    repository = getattr(context, 'repository', None)
    source_options = {'repository': repository} if repository is not None else {}
    if artifacts is not None:
        for ref in submission_refs:
            try:
                loaded = load_outline_source(artifacts, ref, **source_options)
                unit = loaded['unit']
                text = unit.get('text', '')
                if unit.get('grid'):
                    text = _json(unit['grid'])
                excerpts = whole_source_excerpts(artifacts, ref, **source_options) if excerpt_bindings is not None else []
                source(text, _json(ref), [ref], excerpts)
            except (OSError, ValueError, KeyError):
                continue
    return {'submission_rules': rules, 'submission_sources': sources}


def _chapter_refs(chapters):
    part_ids, excerpt_ids, rule_ids = set(), set(), set()
    def walk(nodes):
        for chapter in nodes:
            for target in chapter.get('targets') or []:
                part_ids.update(target.get('template_part_ids') or [])
                excerpt_ids.update(target.get('range_ids') or [])
                rule_ids.update(target.get('rule_ids') or [])
                table = target.get('response_table') or {}
                excerpt_ids.update(table.get('context_range_ids') or [])
                for row in table.get('rows') or []:
                    excerpt_ids.update(row.get('range_ids') or [])
                    excerpt_ids.update(row.get('context_range_ids') or [])
                    rule_ids.update(row.get('rule_ids') or [])
                for guidance in table.get('source_guidance') or []:
                    excerpt_ids.update(guidance.get('range_ids') or [])
            walk(chapter.get('children') or [])
    walk(chapters)
    return part_ids, excerpt_ids, rule_ids


def _assembly_subset(payload, chapters, *, include_unassigned):
    """One fitting assembly request: this chapter group plus shared submission constraints."""
    part_ids, excerpt_ids, rule_ids = _chapter_refs(chapters)
    candidate_rules = [rule for rule in payload.get('candidate_rules') or [] if rule.get('rule_id') in rule_ids]
    unassigned_rules = list(payload.get('unassigned_rules') or []) if include_unassigned else []
    for rule in [*candidate_rules, *unassigned_rules]:
        for evidence in (rule.get('rule') or {}).get('evidence') or []:
            excerpt_ids.update(evidence.get('range_ids') or [])
    result = {key: value for key, value in payload.items() if key not in {
        'chapters', 'template_parts', 'template_runs', 'source_excerpts', 'candidate_rules', 'unassigned_rules'}}
    result['chapters'] = chapters
    result['template_parts'] = [part for part in payload.get('template_parts') or [] if part.get('part_id') in part_ids]
    result['source_excerpts'] = [item for item in payload.get('source_excerpts') or [] if item.get('range_id') in excerpt_ids]
    result['candidate_rules'] = candidate_rules
    result['unassigned_rules'] = unassigned_rules
    order = [part.get('part_id') for part in payload.get('template_parts') or []]
    index = {identity: position for position, identity in enumerate(order)}
    runs = []
    for run in payload.get('template_runs') or []:
        if len(run) < 2 or run[0] not in index or run[1] not in index:
            continue
        start, end = index[run[0]], index[run[1]]
        if start <= end and all(order[point] in part_ids for point in range(start, end + 1)):
            runs.append(run)
    result['template_runs'] = runs
    return result


def _request_fits(budget, payload):
    if _fits(budget, 'outline', payload):
        return True
    from docreader.tender.authoring_view import compact_assembly_payload
    try:
        compact = compact_assembly_payload(payload)
    except (KeyError, TypeError, ValueError):
        return False
    return _fits(budget, 'outline', compact)


def _assembly_batches(payload, budget):
    """Send one assembly when it fits. Otherwise pack whole chapters; do not resend the full tree."""
    if _request_fits(budget, payload):
        return [payload]
    chapters = list(payload.get('chapters') or [])
    if len(chapters) <= 1:
        return [payload]
    groups, current = [], []
    for index, chapter in enumerate(chapters):
        trial = _assembly_subset(payload, [*current, chapter], include_unassigned=index == len(chapters) - 1)
        if current and not _request_fits(budget, trial):
            groups.append(current)
            current = [chapter]
        else:
            current.append(chapter)
    if current:
        groups.append(current)
    if len(groups) <= 1:
        return [payload]
    return [_assembly_subset(payload, group, include_unassigned=index == len(groups) - 1)
            for index, group in enumerate(groups)]


def _targets_of(chapters):
    for chapter in _flatten(chapters):
        yield from chapter.get('targets') or []


def _strip_assembly_bodies(payload):
    """Drop adopted prose the host already holds so one merge can still see every candidate."""
    result = deepcopy(payload)
    def strip(entry):
        if not any(key in entry for key in ('text', 'text_lines', 'grid')):
            return entry
        kept = {key: value for key, value in entry.items() if key not in {'text', 'text_lines', 'grid'}}
        kept['retained'] = True
        return kept
    if isinstance(result.get('template_parts'), list):
        result['template_parts'] = [strip(part) for part in result['template_parts']]
    if isinstance(result.get('source_excerpts'), list):
        result['source_excerpts'] = [strip(item) for item in result['source_excerpts']]
    result['assembly_bodies'] = 'retained_on_host'
    return result


def _join_assemblies(results):
    """Host-concatenate fitting batches. No second full-tree assembly call."""
    chapters, omitted, issues, origins, constraints, intents = [], False, [], set(), [], []
    bindings, selections = {}, {}
    alias_lists, selection_list, scoped_ids = [], [], []
    for scope, (numbered, one_omitted, one_issues, one_origins, one_constraints, one_intents, one_bindings, one_selections) in enumerate(results):
        omitted = omitted or one_omitted
        issues.extend(one_issues)
        origins.update(one_origins)
        constraints.extend(one_constraints)
        intents.extend(one_intents)
        for target in _targets_of(numbered):
            scoped_ids.append((scope, target['target_id']))
            alias_lists.append(one_bindings.get(target['target_id'], []))
            selection_list.append(one_selections.get(target['target_id'], {}))
        chapters.extend(numbered)
    combined = _renumber(chapters)
    new_targets = list(_targets_of(combined))
    remap = dict(zip(scoped_ids, (target['target_id'] for target in new_targets)))
    shared = {}
    for (_, identity), mapped in remap.items():
        shared.setdefault(identity, []).append(mapped)
    for (scope, _), target, aliases, selection in zip(scoped_ids, new_targets, alias_lists, selection_list):
        if aliases:
            bindings[target['target_id']] = aliases
        if selection:
            selections[target['target_id']] = selection
        related = []
        for identity in target.get('related_target_ids') or []:
            matches = [remap[(scope, identity)]] if (scope, identity) in remap else shared.get(identity, [])
            if len(matches) == 1:
                related.append(matches[0])
            else:
                issues.append({'code': 'outline_reference_ambiguous',
                    'detail': '跨位置引用未能确定唯一填写实例，保留待核。',
                    'rule_refs': target.get('rule_refs', []), 'target_id': target['target_id']})
        target['related_target_ids'] = related
    return combined, omitted, issues, origins, constraints, intents, bindings, selections


def _flatten(chapters):
    for chapter in chapters:
        yield chapter
        yield from _flatten(chapter['children'])


def _renumber(chapters):
    def walk(entries, parent=()):
        for ordinal, chapter in enumerate(entries, 1):
            path = (*parent, ordinal)
            chapter['chapter_id'] = 'chapter-' + '.'.join(map(str, path))
            for i, target in enumerate(chapter['targets'], 1):
                target['target_id'] = f"{chapter['chapter_id']}:target-{i}"
                for row in (target.get('response_table') or {}).get('rows', []):
                    row['row_id'] = response_row_id(target['target_id'], row['source_excerpts'], row['rule_refs'])
            walk(chapter['children'], path)
    result = deepcopy(chapters)
    walk(result)
    return result


async def _organize(artifacts, context, budget, execute, gaps, *, submission_sources=(), structure_results=None,
                    concurrency=1):
    if concurrency < 1:
        raise ValueError('source concurrency must be positive')
    def prepare_batch(batch, template_parts):
        local_payload = _outline_payload(context, batch, template_parts)
        wire = WirePayload('outline', local_payload)
        local_rules = frozenset(r['rule_id'] for row in batch for r in row.get('rules', []))
        read_sources = frozenset(source['range_id'] for row in batch for source in _record_sources(row))
        local_sources = frozenset(wire.mapping.get('r', {})) & context.sources.keys()
        local_parts = frozenset(part['part_id'] for part in local_payload['template_parts'])
        read_units = {(ref['source_index_ref']['sha256'], ref['unit_id'])
                      for identity in read_sources for ref in context.sources.get(identity, [])}

        def validate_local(raw):
            value = _parse(OutlineResponse, wire.decode(raw))
            validate_outline_references(value, rule_ids=local_rules, source_ids=local_sources,
                template_part_ids=local_parts, constraint_source_ids=local_sources,
                context_source_ids=read_sources)
            return context.validate(value, template_part_ids=local_parts, read_source_ids=read_sources)

        return wire.payload, validate_local, read_units

    def plan_sources():
        records, template_parts = prepare_template_parts(artifacts, context, _source_records(artifacts, context))
        # Original source content is visible in each local pass. No model coverage echo.
        context.read_units = set()
        def build(batch):
            return WirePayload('outline', _outline_payload(context, batch, template_parts)).payload
        expanded = []
        for row in records:
            if _record_sources(row) and row.get('rules') and not _fits(
                    budget, 'outline', build([row]), headroom_tokens=_OUTLINE_CORRECTION_TOKENS):
                expanded.append({key: value for key, value in row.items() if key != 'rules'} | {'rules': []})
                expanded.extend({'rules': [rule]} for rule in row['rules'])
            else:
                expanded.append(row)
        batches = list(_batches(expanded, budget, 'outline', build,
            headroom_tokens=_OUTLINE_CORRECTION_TOKENS)) or [[]]
        return [prepare_batch(batch, template_parts) for batch in batches]
    source_batches = await asyncio.to_thread(plan_sources)
    source_jobs = iter(enumerate(source_batches))
    stopped = False

    async def organize_sources():
        nonlocal stopped
        completed = []
        try:
            while not stopped:
                job = next(source_jobs, None)
                if job is None:
                    break
                ordinal, (payload, validate_local, read_units) = job
                try:
                    fragment = await _ask(execute, budget, 'outline', payload, validate_local)
                except _LOCAL_FAILURES as error:
                    completed.append((ordinal, None, set(), _gap('outline_batch_unavailable',
                        f'部分原文组织未完成（{type(error).__name__}），保留待核对。')))
                else:
                    completed.append((ordinal, fragment, read_units, None))
        except BaseException:
            stopped = True
            raise
        return completed

    workers = [asyncio.create_task(organize_sources()) for _ in range(min(concurrency, len(source_batches)))]
    try:
        completed = await asyncio.gather(*workers)
    except BaseException:
        for worker in workers:
            worker.cancel()
        await asyncio.gather(*workers, return_exceptions=True)
        raise
    fragments = []
    for _, fragment, read_units, gap in sorted(result for batch in completed for result in batch):
        if gap is not None:
            gaps.append(gap)
        else:
            fragments.append(fragment)
            context.read_units.update(read_units)
    if not fragments:
        return AuthoringOutline(schema_version=2, authoring_input=context.input, issues=[], chapters=[{
            'chapter_id': 'chapter-1', 'title': '投标文件待补内容', 'children': [],
            'targets': [{'target_id': 'chapter-1:target-1', 'kind': 'narrative',
                         'purpose': '根据现有资料起草；招标目录及要求待补，不声称已覆盖。',
                         'rule_refs': [], 'source_refs': []}]}]).model_dump()
    refs = list({rule_ref_key(r): r for f in fragments for r in f.get('constraint_rule_refs', [])}.values())
    submission_refs = {_json(r): r for f in fragments for r in f.get('submission_refs', [])}
    available_sources = {_json(ref) for refs in context.sources.values() for ref in refs}
    for raw in submission_sources:
        ref = SourceUnitRef.model_validate(raw).model_dump()
        if _json(ref) in available_sources:
            submission_refs[_json(ref)] = ref
        else:
            gaps.append(_gap('outline_submission_source_unavailable',
                             '补充提交依据不属于当前可读的冻结来源，保留待核；其余内容继续。'))
    submission_refs = list(submission_refs.values())
    mapping, origins, entries, parts, part_map, part_ids = {}, {}, [], [], {}, {}
    candidate_roles, previous_positions, previous_selections = {}, {}, {}
    decoration_exclusions = {}
    instance_decoration_exclusions, target_decoration_exclusions = {}, {}
    ambiguous_previous_scopes = set()
    previously_scoped_candidates = set()
    repository = getattr(context, 'repository', None) or EvidenceRepository(artifacts)
    source_bindings, submission_excerpt_bindings, submission_source_ids = {}, {}, set()
    excerpt_ids, excerpt_map, excerpt_views, assembly_rules = {}, {}, [], {}
    missing_rules = {}
    def candidates(chapters, parent=(), role='body'):
        for chapter in chapters:
            path = (*parent, chapter['title'])
            current = 'submission_note' if role == 'submission_note' else chapter.get('presentation_role', 'body')
            yield chapter, path, current
            yield from candidates(chapter['children'], path, current)
    def prepare_assembly():
        repository = getattr(context, 'repository', None)
        source_options = {'repository': repository} if repository is not None else {}
        def register_part(part):
            key = template_part_key(part)
            if key in part_ids:
                return part_ids[key]
            identity = f'p{len(parts) + 1}'
            part_ids[key] = identity
            part_map[identity] = part
            descriptor = {'part_id': identity}
            try:
                loaded = load_template_part(artifacts, part, **source_options)
                unit, grid = loaded['unit'], loaded.get('grid')
                descriptor.update(kind=unit['kind'], heading_path=unit.get(
                    'heading_path', unit.get('locator', {}).get('heading_path', '')))
                if grid:
                    descriptor['grid'] = grid
                else:
                    descriptor['text'] = loaded.get('text', '')
                    if unit['kind'] == 'image_region' and not descriptor['text'].strip():
                        descriptor['unavailable'] = True
            except (OSError, ValueError, KeyError):
                descriptor['unavailable'] = True
            parts.append(descriptor)
            return identity

        def register_excerpt(ref):
            key = source_excerpt_key(ref)
            if key not in excerpt_ids:
                identity = f'x{len(excerpt_map) + 1}'
                excerpt_ids[key] = identity
                excerpt_map[identity] = ref
                view = {'range_id': identity}
                try:
                    loaded = load_source_excerpt(artifacts, ref, **source_options)
                    view.update(text=loaded['text'], heading_path=loaded['unit'].get('heading_path', ''))
                except (OSError, ValueError, KeyError):
                    view['unavailable'] = True
                excerpt_views.append(view)
            return excerpt_ids[key]

        def register_rule(ref):
            ref_key = rule_ref_key(ref)
            for identity, value in assembly_rules.items():
                if rule_ref_key(value) == ref_key:
                    return identity
            identity = f'a{len(assembly_rules) + 1}'
            assembly_rules[identity] = ref
            return identity
        for fragment_ordinal, fragment in enumerate(fragments):
            for chapter, path, role in candidates(fragment['chapters']):
                targets = []
                for target in chapter['targets']:
                    key = f't{len(mapping) + 1}'
                    mapping[key] = target
                    candidate_roles[key] = role
                    origins[key] = (fragment_ordinal, target['target_id'])
                    linked_parts = [register_part(part) for part in target.get('template_refs', [])]
                    descriptor = {'target_id': key, 'kind': target['kind'], 'purpose': target['purpose'],
                                    **({'subject': target['subject']} if target.get('subject') else {}),
                                    'manual_fill': target.get('manual_fill', False),
                                    'support_origins': target.get('support_origins', ['knowledge']),
                                    'template_part_ids': linked_parts,
                                    'rule_ids': [register_rule(ref) for ref in target['rule_refs']]}
                    if target.get('source_excerpts'):
                        descriptor['range_ids'] = [register_excerpt(ref) for ref in target['source_excerpts']]
                    if target.get('response_table'):
                        descriptor['response_table'] = {'context_range_ids': [register_excerpt(ref)
                            for ref in target['response_table'].get('context_source_excerpts', [])], 'rows': [
                            {'range_ids': [register_excerpt(ref) for ref in row['source_excerpts']],
                             **({'context_range_ids': [register_excerpt(ref) for ref in row['context_source_excerpts']]}
                                if row.get('context_source_excerpts') else {}),
                             'rule_ids': [register_rule(ref) for ref in row['rule_refs']]}
                            for row in target['response_table']['rows']]}
                        if target['response_table'].get('source_guidance'):
                            descriptor['response_table']['source_guidance'] = [
                                {'kind': entry['kind'], 'range_ids': [register_excerpt(ref) for ref in entry['source_excerpts']]}
                                for entry in target['response_table']['source_guidance']]
                    targets.append(descriptor)
                entries.append({'title': chapter['title'], 'title_path': list(path),
                                'is_volume': chapter.get('is_volume', False),
                                **({'presentation_role': role} if role != 'body' else {}), 'targets': targets})
        # A continuation already named by any candidate is reachable as a format.
        # Native membership authorizes availability, never automatic insertion.
        candidate_units = {_json(ref) for target in mapping.values() for ref in [
            *target['source_refs'], *(part['source_ref'] for part in target.get('template_refs', []))]}
        for part in getattr(context, 'template_parts', {}).values():
            if _json(part['source_ref']) in candidate_units:
                register_part(part)
        source_repository = repository or EvidenceRepository(artifacts)
        if parts:
            ordered = ordered_template_part_ids(part_map, source_repository)
            by_id = {part['part_id']: part for part in parts}
            parts[:] = [by_id[identity] for identity in ordered]
        covered = {rule_ref_key(ref) for ref in refs}
        covered.update(rule_ref_key(ref) for target in mapping.values() for ref in target['rule_refs'])
        views = {entry['rule_id']: entry for entry in context.payload['rules']}
        def semantic_rule(view):
            semantic = deepcopy(view.get('item', view.get('effective_rule', {})))
            semantic.pop('item_id', None)
            evidence = []
            for entry in semantic.get('evidence', []):
                ranges = [register_excerpt(ref) for ref in
                          getattr(context, 'excerpts', {}).get(entry['range_id'], [])]
                evidence.append({'quote': entry['quote'], **({'range_ids': ranges} if ranges else {})})
            semantic['evidence'] = evidence
            return semantic

        unassigned = []
        for identity, reference in context.rules.items():
            if rule_ref_key(reference) in covered:
                continue
            alias = f'u{len(missing_rules) + 1}'
            missing_rules[alias] = reference
            view = views.get(identity, {})
            unassigned.append({'rule_id': alias, 'rule': semantic_rule(view)})
        by_ref = {rule_ref_key(ref): views.get(identity, {}) for identity, ref in context.rules.items()}
        candidate_rules = []
        for identity, reference in assembly_rules.items():
            view = by_ref.get(rule_ref_key(reference), {})
            candidate_rules.append({'rule_id': identity, 'rule': semantic_rule(view)})
        sections, projected_parts = project_source_sections(parts)
        excerpt_sections, projected_excerpts = project_source_sections(excerpt_views)
        assembly_rules.update(missing_rules)
        submission = _constraint_context(context, refs, submission_refs, source_bindings=source_bindings,
                                         excerpt_bindings=submission_excerpt_bindings)
        submission_source_ids.update(source['source_id'] for source in submission['submission_sources'])
        return {'schema_version': 2, 'operation': 'assemble', 'chapters': entries,
                'template_sections': sections, 'template_parts': projected_parts,
                'template_runs': template_part_runs(part_map, source_repository,
                    all_parts=getattr(context, 'template_parts', None)),
                **({'source_sections': excerpt_sections, 'source_excerpts': projected_excerpts} if excerpt_views else {}),
                'unassigned_rules': unassigned, 'candidate_rules': candidate_rules,
                'findings': list({_json({'code': issue['code'], 'detail': issue['detail']}):
                    {'code': issue['code'], 'detail': issue['detail']}
                    for fragment in fragments for issue in fragment['issues']
                    if issue['code'] != 'outline_rule_unassigned'}.values()),
                **submission}
    payload = await asyncio.to_thread(prepare_assembly)
    assembly_gap = None
    try:
        def assemble(raw, provided):
            def provided_ids(field, key):
                entries = provided.get(field) or []
                return set(entries) if isinstance(entries, dict) else {entry[key] for entry in entries}

            target_ids = {target['target_id'] for chapter in provided['chapters'] for target in chapter['targets']}
            unassigned_ids = provided_ids('unassigned_rules', 'rule_id')
            rule_ids = provided_ids('candidate_rules', 'rule_id') | unassigned_ids
            part_aliases = provided_ids('template_parts', 'part_id')
            excerpt_aliases = provided_ids('source_excerpts', 'range_id')
            visible_targets = {identity: target for identity, target in mapping.items() if identity in target_ids}
            visible_rules = {identity: ref for identity, ref in assembly_rules.items() if identity in rule_ids}
            visible_unassigned = {identity: ref for identity, ref in missing_rules.items()
                                  if identity in unassigned_ids}
            visible_parts = {identity: part for identity, part in part_map.items()
                             if identity in part_aliases}
            visible_excerpts = {identity: ref for identity, ref in excerpt_map.items()
                                if identity in excerpt_aliases}
            visible_submission_sources = provided_ids('submission_sources', 'source_id')
            parsed = _parse(OutlineAssemblyResponse, raw)
            used, used_rules, header_issues = set(), set(), []
            fully_used, scoped_coverage = set(), {}
            owners, links, selections = {}, [], []
            selected_parts = deepcopy(visible_parts)
            descriptors = {part['part_id']: part for part in parts if part['part_id'] in visible_parts}
            header_counts, invalid_headers = {}, set()
            for ordinal, entry in enumerate(parsed['table_headers']):
                identity, count = entry['part_id'], entry['header_row_count']
                if identity not in visible_parts:
                    raise ModelContractError([{'path': ['table_headers', ordinal, 'part_id'],
                        'code': 'outline_unknown_template',
                        'detail': 'Select a template part supplied in this request.'}])
                grid = descriptors.get(identity, {}).get('grid') or {}
                previous = header_counts.get(identity, count)
                header_counts[identity] = count
                if (identity in invalid_headers or identity not in selected_parts
                        or (count and count >= grid.get('row_count', 0))
                        or not grid or previous != count
                        or any(cell['row'] < count < cell['row'] + cell.get('row_span', 1)
                               for cell in grid.get('cells', []))):
                    invalid_headers.add(identity)
                    header_issues.append({'code': 'outline_table_header_unverified',
                        'detail': '原表表头展示范围不可确认，保留原表并沿已有标记显示。', 'rule_refs': []})
                    if identity in selected_parts:
                        selected_parts[identity].pop('header_row_count', None)
                    continue
                selected_parts[identity]['header_row_count'] = count

            def rule_refs(identities, path):
                _check_reference_ids(identities, visible_rules, path, 'Select only supplied rule references.')
                return [deepcopy(visible_rules[i]) for i in dict.fromkeys(identities)]

            views = {entry['rule_id']: entry for entry in context.payload['rules']}
            by_ref = {rule_ref_key(ref): views.get(identity, {}) for identity, ref in context.rules.items()}

            def template_override(path):
                value = raw
                for key in path:
                    value = value[key]
                return 'template_part_ids' in value or 'template_spans' in value

            def bind(chapters, path=('chapters',)):
                output = []
                for ordinal, c in enumerate(chapters):
                    chapter_path = (*path, ordinal)
                    targets = []
                    instances = []
                    for number, target in enumerate(c['targets']):
                        for field in ('target_ids', 'related_target_ids'):
                            _check_reference_ids(target[field], visible_targets, (*chapter_path, 'targets', number, field),
                                                 'Select supplied response-target references.')
                        subjects = list(dict.fromkeys(visible_targets[identity]['subject'] for identity in target['target_ids']
                            if identity in visible_targets and visible_targets[identity].get('subject')))
                        unknown_subjects = [identity for identity in dict.fromkeys(target['target_ids'])
                                            if not visible_targets[identity].get('subject')]
                        raw_target = raw
                        for key in (*chapter_path, 'targets', number):
                            raw_target = raw_target[key]
                        previous_key = (tuple(sorted(set(target['target_ids']))), target['subject'])
                        target = {**target, 'same_instance': raw_target.get('same_instance',
                            previous_selections.get(previous_key, {}).get('same_instance', False))}
                        if not target['same_instance'] and (len(subjects) > 1 or (subjects and unknown_subjects
                                and target['subject'] != subjects[0])):
                            for subject in subjects:
                                instances.append((number, {**target, 'subject': subject,
                                    'target_ids': [identity for identity in target['target_ids']
                                        if visible_targets.get(identity, {}).get('subject') == subject]}))
                            instances.extend((number, {**target, 'subject': None, 'target_ids': [identity]})
                                             for identity in unknown_subjects)
                            header_issues.append({'code': 'outline_subject_instances_retained',
                                'detail': '组装合并了不同填写主体，已保留来源明确的各份实例，归属待核。', 'rule_refs': []})
                        else:
                            instances.append((number, {**target, 'subject': target['subject'] or
                                (subjects[0] if len(subjects) == 1 else None)}))
                    for number, target in instances:
                        target_path = (*chapter_path, 'targets', number)
                        for field in ('target_ids', 'related_target_ids'):
                            _check_reference_ids(target[field], visible_targets, (*target_path, field),
                                                 'Select supplied response-target references.')
                        selected = [visible_targets[key] for key in dict.fromkeys(target['target_ids'])]
                        used.update(target['target_ids'])
                        previous_key = (tuple(sorted(set(target['target_ids']))), target['subject'])
                        previous = previous_positions.get(previous_key)
                        raw_target = raw
                        for key in target_path:
                            raw_target = raw_target[key]
                        scope = target['candidate_scope'] if 'candidate_scope' in raw_target else (
                            previous_selections.get(previous_key, {}).get('candidate_scope'))
                        if 'candidate_scope' not in raw_target and (
                                previous_key in ambiguous_previous_scopes or (
                                    previous_key not in previous_positions
                                    and previously_scoped_candidates.intersection(target['target_ids']))):
                            raise ModelContractError([{'path': [*target_path, 'candidate_scope'],
                                'code': 'outline_scope_ambiguous',
                                'detail': 'Select this position\'s candidate_scope explicitly; the previous candidate has multiple scoped positions.'}])
                        candidate_rules = [ref for entry in selected for ref in entry['rule_refs']]
                        candidate_excerpts = [ref for entry in selected for ref in entry.get('source_excerpts', [])]
                        candidate_sources = [ref for entry in selected for ref in entry['source_refs']]
                        if scope is not None:
                            if (len(selected) != 1 or target['kind'] != 'narrative' or selected[0]['kind'] != 'narrative'
                                    or any(entry.get('template_refs') or entry.get('response_table')
                                           for entry in [*selected, *([previous] if previous else [])])
                                    or target['template_part_ids'] or target['template_spans'] or target['response_table']):
                                raise ModelContractError([{'path': [*target_path, 'candidate_scope'],
                                    'code': 'outline_scope_carrier_conflict',
                                    'detail': 'Scope only one narrative candidate without a native template or response table; retain complete carriers.'}])
                            allowed_ranges = {excerpt_ids[source_excerpt_key(ref)] for ref in candidate_excerpts} & visible_excerpts.keys()
                            candidate_rule_keys = {rule_ref_key(ref) for ref in candidate_rules}
                            allowed_rules = {identity for identity, ref in visible_rules.items() if rule_ref_key(ref) in candidate_rule_keys}
                            _check_reference_ids(scope['range_ids'], allowed_ranges,
                                (*target_path, 'candidate_scope', 'range_ids'), 'Select excerpts belonging to this candidate.')
                            _check_reference_ids(scope['rule_ids'], allowed_rules,
                                (*target_path, 'candidate_scope', 'rule_ids'), 'Select complete rules belonging to this candidate.')
                            candidate_rules = rule_refs(scope['rule_ids'], (*target_path, 'candidate_scope', 'rule_ids'))
                            candidate_excerpts = [deepcopy(visible_excerpts[identity])
                                for identity in dict.fromkeys(scope['range_ids'])]
                            candidate_sources = [ref['source_ref'] for ref in candidate_excerpts]
                        else:
                            fully_used.update(target['target_ids'])
                        manual_fill = target['manual_fill']
                        if 'manual_fill' not in raw_target and previous:
                            manual_fill = previous.get('manual_fill', False)
                        selection = {field: list(dict.fromkeys(target[field] if field in raw_target else
                            previous_selections.get(previous_key, {}).get(field, [])))
                            for field in ('rule_ids', 'source_ids', 'related_target_ids')}
                        if scope is not None:
                            selection['candidate_scope'] = deepcopy(scope)
                        if target['same_instance'] or 'same_instance' in raw_target:
                            selection['same_instance'] = target['same_instance']
                        _check_reference_ids(selection['related_target_ids'], visible_targets,
                            (*target_path, 'related_target_ids'), 'Select supplied response-target references.')
                        additional = rule_refs(selection['rule_ids'], (*target_path, 'rule_ids'))
                        used_rules.update(selection['rule_ids'])
                        _check_reference_ids(target['template_part_ids'], visible_parts, (*target_path, 'template_part_ids'),
                            'Select template parts provided in this assembly request.', code='outline_unknown_template')
                        template_ids = list(dict.fromkeys([*target['template_part_ids'],
                            *expand_template_spans(visible_parts, target['template_spans'],
                                repository or EvidenceRepository(artifacts), path=[*target_path, 'template_spans'],
                                all_parts=getattr(context, 'template_parts', visible_parts))]))
                        if target['template_spans']:
                            template_ids = ordered_template_part_ids({identity: visible_parts[identity]
                                for identity in template_ids}, repository or EvidenceRepository(artifacts))
                        inherited = [previous] if previous else selected
                        if not template_override(target_path):
                            template_ids = list(dict.fromkeys(part_ids[template_part_key(part)]
                                for entry in inherited for part in entry.get('template_refs', [])))
                        excluded_parts = set().union(*(decoration_exclusions.get(identity, set())
                                                       for identity in target['target_ids']))
                        raw_key = (tuple(sorted(set(raw_target.get('target_ids', [])))), raw_target.get('subject'))
                        excluded_parts.update(instance_decoration_exclusions.get(raw_key, set()))
                        template_ids = [identity for identity in template_ids
                                        if template_part_key(visible_parts[identity]) not in excluded_parts]
                        additional_sources = []
                        _check_reference_ids(selection['source_ids'], visible_submission_sources, (*target_path, 'source_ids'),
                            'Select sources supplied in this assembly request.', code='outline_unknown_source')
                        _check_reference_ids(selection['source_ids'], source_bindings, (*target_path, 'source_ids'),
                            'This source has no frozen reference; select a bindable source or leave it unresolved.',
                            code='outline_source_unbindable')
                        additional_sources.extend(ref for identity in selection['source_ids'] for ref in source_bindings[identity])
                        exact_sources = list(candidate_excerpts)
                        exact_sources.extend(ref for identity in selection['source_ids']
                                             for ref in submission_excerpt_bindings.get(identity, []))
                        for reference in [*(candidate_rules if scope is not None else []), *additional]:
                            view = by_ref[rule_ref_key(reference)]
                            semantic = view.get('item', view.get('effective_rule', {}))
                            for evidence in semantic.get('evidence', []):
                                exact = getattr(context, 'excerpts', {}).get(evidence['range_id'], [])
                                additional_sources.extend([ref['source_ref'] for ref in exact] if scope is not None and exact
                                    else context.sources.get(evidence['range_id'], []))
                                exact_sources.extend(exact)
                        bound = {'kind': target['kind'], 'purpose': target['purpose'],
                            **({'subject': target['subject']} if target['subject'] else {}),
                            'manual_fill': manual_fill or any(t.get('manual_fill', False) for t in selected),
                            'support_origins': list(dict.fromkeys(origin for t in selected
                                for origin in t.get('support_origins', ['knowledge']))) if selected else target['support_origins'],
                            'rule_refs': list({rule_ref_key(r): r for r in [
                                *candidate_rules, *additional]}.values()),
                            'source_refs': list({_json(r): r for r in [
                                *candidate_sources, *additional_sources]}.values()),
                            'template_refs': [deepcopy(selected_parts[p]) for p in template_ids]}
                        if exact_sources:
                            bound['source_excerpts'] = list({source_excerpt_key(ref): ref for ref in exact_sources}.values())
                        row_values = []
                        table_context = [deepcopy(ref) for t in inherited
                            for ref in (t.get('response_table') or {}).get('context_source_excerpts', [])]
                        explicit_context = []
                        if target.get('response_table'):
                            if bound['template_refs']:
                                raise ModelContractError([{'path': [*target_path, 'response_table'],
                                    'code': 'outline_conflicting_table',
                                    'detail': 'Select one actual table carrier per position; use distinct targets for distinct deliverables.'}])
                            _check_reference_ids(target['response_table']['context_range_ids'], visible_excerpts,
                                (*target_path, 'response_table', 'context_range_ids'),
                                'Select supplied exact source references.')
                            explicit_context = [deepcopy(visible_excerpts[i])
                                for i in target['response_table']['context_range_ids']]
                            table_context.extend(explicit_context)
                            for row_number, row in enumerate(target['response_table']['rows']):
                                _check_reference_ids(row['range_ids'], visible_excerpts,
                                    (*target_path, 'response_table', 'rows', row_number, 'range_ids'),
                                    'Select supplied exact source references.')
                                row_values.append({'source_excerpts': [deepcopy(visible_excerpts[i])
                                    for i in dict.fromkeys(row['range_ids'])],
                                    'rule_refs': rule_refs(row['rule_ids'], (*target_path, 'response_table', 'rows', row_number, 'rule_ids'))})
                                used_rules.update(row['rule_ids'])
                        elif not bound['template_refs']:
                            row_values = [deepcopy(row) for t in inherited for row in
                                          (t.get('response_table') or {}).get('rows', [])]
                        inherited_rows = [row for t in inherited
                                          for row in (t.get('response_table') or {}).get('rows', [])]
                        if inherited_rows and bound['template_refs']:
                            raise ModelContractError([{'path': [*target_path, 'template_part_ids'],
                                'code': 'outline_conflicting_table',
                                'detail': 'Keep known response rows in their business table; select native forms as separate grounded positions.'}])
                        def row_atoms(row):
                            return {(ref['source_ref']['source_index_ref']['sha256'], ref['source_ref']['unit_id'],
                                     s['source_range_id'], s['start'], s['end'])
                                    for ref in row['source_excerpts'] for s in ref['selectors']}
                        covered_atoms = set().union(*(row_atoms(row) for row in row_values)) if row_values else set()
                        context_sets = {frozenset(source_excerpt_key(ref) for ref in
                            t['response_table'].get('context_source_excerpts', []))
                            for t in inherited if t.get('response_table')}
                        if len(context_sets) > 1:
                            assigned_context = {source_excerpt_key(ref) for ref in explicit_context}
                            if ('context_range_ids' not in (raw_target.get('response_table') or {})
                                    or set().union(*context_sets) - assigned_context
                                    or any(row_atoms(row) - covered_atoms for row in inherited_rows)):
                                known_excerpts = {source_excerpt_key(ref) for t in inherited for ref in [
                                    *t.get('source_excerpts', []),
                                    *(t.get('response_table') or {}).get('context_source_excerpts', []),
                                    *(ref for row in (t.get('response_table') or {}).get('rows', [])
                                      for ref in row['source_excerpts'])]}
                                extra_excerpts = list({source_excerpt_key(ref): ref for ref in [*exact_sources,
                                    *explicit_context, *(ref for row in row_values for ref in row['source_excerpts'])]
                                    if source_excerpt_key(ref) not in known_excerpts}.values())
                                known_sources = {_json(ref) for t in inherited for ref in t['source_refs']}
                                extra_sources = list({_json(ref): ref for ref in [*bound['source_refs'],
                                    *(ref['source_ref'] for ref in extra_excerpts)]
                                    if _json(ref) not in known_sources}.values())
                                known_rules = {rule_ref_key(ref) for t in inherited for ref in t['rule_refs']}
                                extra_rules = list({rule_ref_key(ref): ref for ref in [*bound['rule_refs'],
                                    *(ref for row in row_values for ref in row['rule_refs'])]
                                    if rule_ref_key(ref) not in known_rules}.values())
                                location = '章节「' + c['title'] + '」，原用途：' + '；'.join(t['purpose'] for t in inherited)
                                header_issues.append({'code': 'outline_response_context_unassigned',
                                    'detail': location + '。不同原表的条件及响应行尚未完整交接，已保留各份原表及空应答；'
                                        '新增来源仅保留为理解上下文，归属待核。', 'rule_refs': []})
                                if extra_rules:
                                    header_issues.append({'code': 'outline_rule_unassigned',
                                        'detail': location + '。新增规则尚未确定响应位置，归属待核。', 'rule_refs': extra_rules})
                                for position, (identity, original) in enumerate(zip(
                                        dict.fromkeys(target['target_ids']), inherited)):
                                    retained = deepcopy(original)
                                    if position == 0:
                                        retained['source_refs'] = list({_json(ref): ref for ref in [
                                            *retained['source_refs'], *extra_sources]}.values())
                                        if extra_excerpts:
                                            retained['source_excerpts'] = list({source_excerpt_key(ref): ref for ref in [
                                                *retained.get('source_excerpts', []), *extra_excerpts]}.values())
                                    targets.append(retained)
                                    selections.append((retained, {'related_target_ids': selection['related_target_ids']}))
                                    links.append((retained, selection['related_target_ids']))
                                    owners.setdefault(identity, []).append(retained)
                                continue
                            table_context = explicit_context
                        for row in inherited_rows:
                            if row_atoms(row) - covered_atoms:
                                row_values.append(deepcopy(row))
                                covered_atoms.update(row_atoms(row))
                                header_issues.append({'code': 'outline_response_row_unassigned',
                                    'detail': '组装未完整安排已知响应行，宿主保留其完整要求及空应答，归属待核。',
                                    'rule_refs': row['rule_refs']})
                        if row_values:
                            bound['response_table'] = {'rows': list({_json({k: v for k, v in row.items() if k != 'row_id'}):
                                {**row, 'row_id': 'pending-row'} for row in row_values}.values()),
                                **({'context_source_excerpts': list({source_excerpt_key(ref): ref
                                    for ref in table_context}.values())} if table_context else {})}
                            bound['source_refs'] = list({_json(ref): ref for ref in [*bound['source_refs'],
                                *(ref['source_ref'] for ref in table_context),
                                *(ref['source_ref'] for row in row_values for ref in row['source_excerpts'])]}.values())
                            bound['rule_refs'] = list({rule_ref_key(ref): ref for ref in [*bound['rule_refs'],
                                *(ref for row in row_values for ref in row['rule_refs'])]}.values())
                        targets.append(bound)
                        if scope is not None:
                            identity = target['target_ids'][0]
                            covered = scoped_coverage.setdefault(identity, {'ranges': set(), 'rules': set()})
                            covered['ranges'].update(source_excerpt_key(ref) for ref in bound.get('source_excerpts', []))
                            covered['rules'].update(rule_ref_key(ref) for ref in bound['rule_refs'])
                        selections.append((bound, selection))
                        links.append((bound, selection['related_target_ids']))
                        for identity in dict.fromkeys(target['target_ids']):
                            owners.setdefault(identity, []).append(bound)
                    output.append({'title': c['title'], 'is_volume': c['is_volume'],
                                   'presentation_role': c['presentation_role'],
                                   'targets': targets, 'children': bind(c['children'], (*chapter_path, 'children'))})
                return output
            result = bind(parsed['chapters'])
            excluded, exclusion_issues = set(), list(header_issues)
            retained_constraints = provided_ids('submission_rules', 'rule_id')
            _check_reference_ids(parsed['constraint_rule_ids'], visible_rules.keys() | retained_constraints,
                                 ['constraint_rule_ids'], 'Select only supplied rule references.')
            new_constraints = [identity for identity in parsed['constraint_rule_ids'] if identity not in retained_constraints]
            added_constraints = rule_refs(new_constraints, ['constraint_rule_ids'])
            used_rules.update(new_constraints)
            for number, entry in enumerate(parsed['excluded_rules']):
                path = ['excluded_rules', number, 'rule_ids']
                excluded_refs = rule_refs(entry['rule_ids'], path)
                _check_reference_ids(entry['rule_ids'], visible_unassigned.keys() - used_rules, path,
                    'A rule cannot be both assigned and excluded.', code='outline_conflicting_disposition')
                exclusion_issues.append({'code': 'outline_rule_excluded', 'detail': entry['reason'],
                                         'rule_refs': excluded_refs})
            for number, entry in enumerate(parsed['excluded_targets']):
                identities = set(entry['target_ids'])
                path = ['excluded_targets', number, 'target_ids']
                _check_reference_ids(entry['target_ids'], visible_targets, path,
                                     'Exclude only supplied response-target references.')
                _check_reference_ids(entry['target_ids'], visible_targets.keys() - used, path,
                    'A candidate cannot be both selected and excluded.', code='outline_conflicting_disposition')
                excluded.update(identities)
                selected = [visible_targets[key] for key in dict.fromkeys(entry['target_ids'])]
                exclusion_issues.append({'code': 'outline_target_excluded',
                    'detail': entry['reason'] + ' 原候选用途：' + '；'.join(t['purpose'] for t in selected),
                    'rule_refs': list({rule_ref_key(ref): ref for t in selected for ref in t['rule_refs']}.values())})
            used.update(excluded)
            omitted = {}
            for key, original in visible_targets.items():
                if key in excluded or key in fully_used:
                    continue
                if key not in scoped_coverage:
                    omitted[key] = deepcopy(original)
                    continue
                covered = scoped_coverage[key]
                remaining_excerpts = [ref for ref in original.get('source_excerpts', [])
                                      if source_excerpt_key(ref) not in covered['ranges']]
                remaining_rules = [ref for ref in original['rule_refs'] if rule_ref_key(ref) not in covered['rules']]
                accounted_sources = {_json(ref['source_ref']) for ref in original.get('source_excerpts', [])}
                for reference in original['rule_refs']:
                    view = by_ref.get(rule_ref_key(reference), {})
                    semantic = view.get('item', view.get('effective_rule', {}))
                    accounted_sources.update(_json(ref) for evidence in semantic.get('evidence', [])
                        for ref in context.sources.get(evidence['range_id'], []))
                if any(_json(ref) not in accounted_sources for ref in original['source_refs']):
                    omitted[key] = deepcopy(original)
                    continue
                if not remaining_excerpts and not remaining_rules:
                    continue
                retained = deepcopy(original)
                retained['rule_refs'] = remaining_rules
                retained['source_excerpts'] = deepcopy(remaining_excerpts)
                sources = [ref['source_ref'] for ref in remaining_excerpts]
                for reference in remaining_rules:
                    view = by_ref.get(rule_ref_key(reference), {})
                    semantic = view.get('item', view.get('effective_rule', {}))
                    for evidence in semantic.get('evidence', []):
                        exact = getattr(context, 'excerpts', {}).get(evidence['range_id'], [])
                        retained['source_excerpts'].extend(deepcopy(exact))
                        sources.extend([ref['source_ref'] for ref in exact] if exact
                                       else context.sources.get(evidence['range_id'], []))
                retained['source_excerpts'] = list({source_excerpt_key(ref): ref
                    for ref in retained['source_excerpts']}.values())
                retained['source_refs'] = list({_json(ref): deepcopy(ref) for ref in sources}.values())
                omitted[key] = retained
                selections.append((retained, {'candidate_scope': {
                    'range_ids': [excerpt_ids[source_excerpt_key(ref)] for ref in remaining_excerpts],
                    'rule_ids': [identity for reference in remaining_rules
                                 for identity, ref in visible_rules.items() if rule_ref_key(reference) == rule_ref_key(ref)]}}))
            for role in ('body', 'submission_note'):
                retained = [target for key, target in omitted.items()
                            if (candidate_roles[key] == 'submission_note') == (role == 'submission_note')]
                if retained:
                    result.append({'title': '待核提交待办' if role == 'submission_note' else '待整理内容',
                        'is_volume': False, 'presentation_role': role, 'targets': retained, 'children': []})
            for key, target in omitted.items():
                owners.setdefault(key, []).append(target)
            # Resolve links after the same host numbering used by the persisted tree.
            for ordinal, chapter in enumerate(_flatten(result)):
                for number, target in enumerate(chapter['targets']):
                    target['_assembly_ordinal'] = (ordinal, number)
            numbered = _renumber(result)
            formal = {target['_assembly_ordinal']: target for c in _flatten(numbered) for target in c['targets']}
            for target, identities in links:
                bound = formal[target['_assembly_ordinal']]
                bound['related_target_ids'] = []
                for identity in dict.fromkeys(identities):
                    candidates = owners.get(identity, [])
                    if len(candidates) != 1:
                        exclusion_issues.append({'code': 'outline_reference_ambiguous',
                            'detail': '跨位置引用未能确定唯一填写实例，保留待核。', 'rule_refs': bound['rule_refs']})
                    else:
                        linked = formal[candidates[0]['_assembly_ordinal']]['target_id']
                        if linked != bound['target_id'] and linked not in bound['related_target_ids']:
                            bound['related_target_ids'].append(linked)
            for target in formal.values():
                target.pop('_assembly_ordinal', None)
            added_intents = []
            for number, intent in enumerate(parsed['search_intents']):
                references = rule_refs(intent['for_ids'], ['search_intents', number, 'for_ids'])
                source_refs = [source for ref in references
                    for e in by_ref[rule_ref_key(ref)].get('item', by_ref[rule_ref_key(ref)].get('effective_rule', {})).get('evidence', [])
                    for source in context.sources.get(e['range_id'], [])]
                added_intents.append({'query': intent['query'], 'rule_refs': references,
                    'source_refs': list({_json(ref): ref for ref in source_refs}.values()), 'range_ids': []})
            candidate_bindings = {}
            for alias, selected in owners.items():
                for target in selected:
                    candidate_bindings.setdefault(formal[target['_assembly_ordinal']]['target_id'], []).append(alias)
            candidate_selections = {formal[target['_assembly_ordinal']]['target_id']: selection
                                    for target, selection in selections}
            return (numbered, bool(omitted), exclusion_issues, {origins[key] for key in excluded},
                    added_constraints, added_intents, candidate_bindings, candidate_selections)
        def assembly_validator(view):
            return lambda raw: assemble(raw, view)

        batches = _assembly_batches(payload, budget)
        if len(batches) == 1:
            assembled = await _ask(execute, budget, 'outline', batches[0], assembly_validator(batches[0]))
        else:
            batch_results = [await _ask(execute, budget, 'outline', batch, assembly_validator(batch)) for batch in batches]
            slim = _strip_assembly_bodies(payload)
            try:
                assembled = await _ask(execute, budget, 'outline', slim, assembly_validator(slim))
            except _LOCAL_FAILURES:
                assembled = _join_assemblies(batch_results)
        (chapters, omitted, exclusion_issues, excluded_origins, added_constraints, added_intents,
         candidate_bindings, candidate_selections) = assembled
        refs = list({rule_ref_key(ref): ref for ref in [*refs, *added_constraints]}.values())
        if omitted:
            assembly_gap = _gap('outline_assembly_incomplete', '归并未安排的响应位置已保留在末尾，顺序待核对。')
            gaps.append(assembly_gap)
    except _LOCAL_FAILURES:
        exclusion_issues = [{'code': 'outline_assembly_unavailable',
            'detail': '正式归并未完成，已保留来源局部章节及全部响应位置，全局目录、重复及顺序待核对。',
            'rule_refs': []}]
        excluded_origins = set()
        added_intents = []
        candidate_selections = {}
        chapters = _renumber([chapter for fragment in fragments for chapter in fragment['chapters']])
        retained = (target for chapter in _flatten(chapters) for target in chapter['targets'])
        candidate_bindings = {target['target_id']: [alias] for alias, target in zip(mapping, retained)}
        assembly_gap = _gap('outline_assembly_unavailable', '正式归并未完成，已保留来源局部章节及全部响应位置，全局目录、重复及顺序待核对。')
        gaps.append(assembly_gap)
    def finalize(chapters, exclusion_issues, excluded_origins, added_intents):
        issues = [{**i, 'detail': '来源读取阶段观察（最终情况以全局核对为准）：' + i['detail']}
                  for ordinal, f in enumerate(fragments) for i in f['issues']
                  if i['code'] != 'outline_rule_unassigned'
                  and (ordinal, i.get('target_id')) not in excluded_origins]
        issues.extend(exclusion_issues)
        assigned = {rule_ref_key(r) for c in _flatten(chapters) for t in c['targets'] for r in t['rule_refs']}
        assigned.update(rule_ref_key(r) for r in refs)
        assigned.update(rule_ref_key(ref) for issue in exclusion_issues for ref in issue['rule_refs'])
        for ref in context.rules.values():
            if rule_ref_key(ref) not in assigned:
                issues.append({'code': 'outline_rule_unassigned', 'detail': '该规则尚未安排响应或交付位置。', 'rule_refs': [ref]})
        # Local path IDs cannot survive assembly. Issues remain truthful at outline level.
        for issue in issues:
            issue.pop('target_id', None)
        return AuthoringOutline(schema_version=2, authoring_input=context.input, chapters=_renumber(chapters),
                                issues=issues, constraint_rule_refs=refs, submission_refs=submission_refs,
                                search_intents=list({_json(intent): intent for intent in [
                                    *(intent for f in fragments for intent in f.get('search_intents', [])),
                                    *added_intents]}.values())).model_dump(exclude_unset=True)
    outline = finalize(chapters, exclusion_issues, excluded_origins, added_intents)
    if structure_results is not None:
        from docreader.tender.authoring_structure import review_structure
        previous_parts = {target['target_id']: {template_part_key(part)
            for part in target.get('template_refs', [])}
            for chapter in _flatten(outline['chapters']) for target in chapter['targets']}
        findings, structure_gaps = await review_structure(
            artifacts, outline, [], budget, execute, outline_phase=True,
            apply_source_decoration_exclusions=True, apply_title_edits=True)
        choices, candidate_instances = {}, {}
        for chapter in _flatten(outline['chapters']):
            for target in chapter['targets']:
                aliases = candidate_bindings.get(target['target_id'], [])
                if aliases:
                    key = (tuple(sorted(aliases)), target.get('subject'))
                    choices.setdefault(key, []).append(target)
                    for identity in aliases:
                        candidate_instances.setdefault(identity, []).append(target['target_id'])
        for chapter in _flatten(outline['chapters']):
            for target in chapter['targets']:
                removed = previous_parts[target['target_id']] - {
                    template_part_key(part) for part in target.get('template_refs', [])}
                if removed:
                    target_decoration_exclusions[target['target_id']] = removed
                    aliases = candidate_bindings.get(target['target_id'], [])
                    key = (tuple(sorted(aliases)), target.get('subject'))
                    if aliases and len(choices[key]) == 1:
                        instance_decoration_exclusions[key] = removed
                    for identity in aliases:
                        if len(candidate_instances[identity]) == 1:
                            decoration_exclusions.setdefault(identity, set()).update(removed)
        if findings:
            previous_positions = {key: values[0] for key, values in choices.items() if len(values) == 1}
            ambiguous_previous_scopes = {key for key, values in choices.items() if len(values) > 1
                and any(candidate_selections.get(target['target_id'], {}).get('candidate_scope') is not None
                        for target in values)}
            previously_scoped_candidates = {alias for chapter in _flatten(outline['chapters'])
                for target in chapter['targets']
                if candidate_selections.get(target['target_id'], {}).get('candidate_scope') is not None
                for alias in candidate_bindings.get(target['target_id'], [])}
            previous_selections = {key: candidate_selections.get(target['target_id'], {})
                                   for key, target in previous_positions.items()}
            previous_targets = {target['target_id']: f'v{ordinal}' for ordinal, target in enumerate(
                (t for c in _flatten(outline['chapters']) for t in c['targets']), 1)}

            def previous_structure(entries):
                return [{'chapter_id': chapter['chapter_id'], 'title': chapter['title'],
                         'is_volume': chapter.get('is_volume', False),
                         'presentation_role': chapter.get('presentation_role', 'body'),
                         'targets': [{'review_target_id': previous_targets[target['target_id']],
                                      'candidate_target_ids': candidate_bindings.get(target['target_id'], []),
                                      'kind': target['kind'], 'purpose': target['purpose'],
                                      'manual_fill': target.get('manual_fill', False),
                                      **candidate_selections.get(target['target_id'], {}),
                                      **({'subject': target['subject']} if target.get('subject') else {}),
                                      'template_part_ids': [part_ids[template_part_key(part)]
                                          for part in target.get('template_refs', [])],
                                      **({'excluded_template_part_ids': [identity
                                          for identity, part in part_map.items() if template_part_key(part)
                                          in target_decoration_exclusions[target['target_id']]]}
                                         if target['target_id'] in target_decoration_exclusions else {}),
                                      **({'response_table': {'context_range_ids': [excerpt_ids[source_excerpt_key(ref)]
                                              for ref in target['response_table'].get('context_source_excerpts', [])],
                                          'rows': [{'range_ids': [excerpt_ids[source_excerpt_key(ref)]
                                                        for ref in row['source_excerpts']],
                                                    'rule_ids': [identity for ref in row['rule_refs']
                                                        for identity, value in assembly_rules.items() if rule_ref_key(value) == rule_ref_key(ref)]}
                                                   for row in target['response_table']['rows']]}}
                                         if target.get('response_table') else {})}
                                     for target in chapter['targets']],
                         'children': previous_structure(chapter['children'])} for chapter in entries]

            from docreader.tender.authoring_view import slim_revision_payload
            feedback = slim_revision_payload(payload, previous_structure(outline['chapters']), findings)
            feedback.update({'review_findings': findings,
                        **({'candidate_excluded_template_part_ids': {identity: [part_id
                            for part_id, part in part_map.items() if template_part_key(part) in removed]
                            for identity, removed in decoration_exclusions.items()}} if decoration_exclusions else {})})
            try:
                repaired, omitted, new_issues, new_excluded, new_constraints, new_intents, _, _ = await _ask(
                    execute, budget, 'outline', feedback, assembly_validator(feedback))
                refs = list({rule_ref_key(ref): ref for ref in [*refs, *new_constraints]}.values())
                candidate = finalize(repaired, new_issues, new_excluded, new_intents)
                changed = candidate['chapters'] != outline['chapters']
                outline = candidate
                if assembly_gap is not None:
                    gaps[:] = [gap for gap in gaps if gap is not assembly_gap]
                if changed:
                    new_findings, new_gaps = await review_structure(
                        artifacts, outline, [], budget, execute, outline_phase=True,
                        apply_source_decoration_exclusions=True, apply_hierarchy_edits=True,
                        apply_title_edits=True)
                    if any(g['code'] == 'structure_review_unavailable' for g in new_gaps):
                        findings = [{**f, 'can_revise': False,
                            'detail': '结构修订后尚未复核的上一版观察：' + f['detail']} for f in findings]
                    else:
                        findings = new_findings
                    structure_gaps.extend(new_gaps)
                if omitted:
                    gaps.append(_gap('outline_assembly_incomplete', '结构修订仍有未安排位置，保留待整理内容。'))
            except _LOCAL_FAILURES as error:
                structure_gaps.append(_gap('outline_revision_unavailable',
                    f'本次结构修订未完成（{type(error).__name__}），保留已有大纲及意见继续生成。'))
        structure_results.update(findings=findings, gaps=structure_gaps)
        outline['issues'].extend({'code': finding['code'],
            'detail': finding['detail'] + (' 建议：' + finding['suggestion'] if finding['suggestion'] else ''),
            'rule_refs': []} for finding in findings)
    return outline


def plan_writing_tasks(outline):
    result = []
    identities = {target['target_id'] for chapter in _flatten(outline['chapters']) for target in chapter['targets']}
    def walk(chapters, titles=()):
        for chapter in chapters:
            if chapter.get('presentation_role') == 'submission_note':
                continue
            path, group = (*titles, chapter['title']), []
            def emit():
                if group:
                    result.append(WritingTask(task_id=f'w{len(result) + 1}', chapter_id=chapter['chapter_id'],
                                              title_path=list(path), targets=deepcopy(group)).model_dump(exclude_unset=True))
                    group.clear()
            for target in chapter['targets']:
                if target.get('credential_response_replaced'):
                    continue
                related = target.get('related_target_ids', [])
                if (target['kind'] == 'narrative' and not target.get('manual_fill', False)
                        and not any(target.get(field) for field in ('rule_refs', 'source_refs', 'template_refs', 'project_context_excerpts'))
                        and related and all(identity in identities and identity != target['target_id']
                                            for identity in related)):
                    if target.get('support_origins') == ['knowledge'] and str(target.get('target_id', '')).endswith(':answer'):
                        emit()
                        group.append(target)
                        emit()
                    continue
                if target['kind'] != 'narrative' or target.get('manual_fill', False):
                    emit()
                    group.append(target)
                    emit()
                else:
                    group.append(target)
            emit()
            walk(chapter['children'], path)
    walk(outline['chapters'])
    return result


def _load_task(artifacts, context, task, basis, materials):
    from docreader.tender.source_body import load_source_body, load_source_context
    rules, sources, issues, seed_ranges = [], {}, [], set()
    repository = getattr(context, 'repository', None)
    source_options = {'repository': repository} if repository is not None else {}
    template_supports, response_context_supports, response_rows = {}, {}, []
    source_context_supports, body_cache = {}, {}
    views = {r['rule_id']: r for r in context.payload['rules']}
    by_ref = {rule_ref_key(ref): views[identity] for identity, ref in context.rules.items()}
    needed = {m['material_id']: m for m in _task_materials(materials, task)}
    global_issues = [i['issue_id'] for i in context.payload.get('issues', [])
                     if not i.get('range_id') and not i.get('item_ids') and i.get('issue_id')]

    def references(identity):
        return getattr(context, 'excerpts', {}).get(identity) or context.sources.get(identity, [])

    def register(ref, target_alias, *, template=None):
        exact = 'selectors' in ref and template is None
        precise = ref if exact else ({'source_ref': ref, 'selectors': template['selectors']}
                                    if template and template['selectors'] else None)
        key = ('excerpt', source_excerpt_key(precise)) if precise else _json(ref)
        if key not in sources:
            try:
                loaded = (load_source_excerpt(artifacts, ref, **source_options) if exact else
                          load_template_part(artifacts, template, **source_options) if template else
                          load_outline_source(artifacts, ref, **source_options))
                unit = loaded['unit']
                text, ocr = loaded.get('text', unit.get('text', '')), loaded.get('ocr')
                if ocr and ocr.get('text', '').strip() and ocr['text'] != text:
                    text = '\n'.join(part for part in (text, ocr['text']) if part)
                sources[key] = {'support_id': f's{len(sources) + 1}', 'origin': 'tender',
                    'reference': precise or ref, 'target_ids': [], 'text': text,
                    'grid': loaded.get('grid', unit.get('grid')),
                    'document_role': loaded['document'].get('role'),
                    'heading_path': unit.get('heading_path', unit.get('locator', {}).get('heading_path', ''))}
                if ocr:
                    sources[key]['ocr'] = {k: v for k, v in ocr.items() if k != 'text'}
            except (OSError, ValueError, KeyError):
                issues.append('相关原文或模板暂不可读取，保留待核对位置。')
                return None
        if target_alias not in sources[key]['target_ids']:
            sources[key]['target_ids'].append(target_alias)
        return sources[key]['support_id']

    for ordinal, target in enumerate(task['targets'], 1):
        alias = f't{ordinal}'
        if (target.get('source_body_contract') is not None or target.get('source_body_ref') is not None
                or target.get('source_context_excerpts')):
            load_source_body(artifacts, context.input, target, repository=repository, cache=body_cache)
        elif target.get('project_context_excerpts'):
            load_source_context(artifacts, context.input, target, repository=repository)
        target_rules = [by_ref[rule_ref_key(ref)] for ref in target['rule_refs'] if rule_ref_key(ref) in by_ref]
        rules.extend(target_rules)
        rows = (target.get('response_table') or {}).get('rows', [])
        table_context = (target.get('response_table') or {}).get('context_source_excerpts', [])
        source_context = target.get('project_context_excerpts') or target.get('source_context_excerpts', [])
        excerpts = [*target.get('source_excerpts', []), *source_context,
                    *response_table_excerpts(target.get('response_table') or {})]
        precise_units = {_json(ref['source_ref']) for ref in excerpts}
        coarse_refs = [ref for ref in target['source_refs'] if _json(ref) not in precise_units]
        refs = [*excerpts, *coarse_refs]
        original_ranges = {selected['range_id'] for ref in excerpts for selected in ref['selectors']}
        original_ranges.update(identity for identity, values in context.sources.items()
                               if any(ref in coarse_refs for ref in values))
        for part in target.get('template_refs', []):
            if part['selectors']:
                original_ranges.update(selected['range_id'] for selected in part['selectors'])
            else:
                original_ranges.update(identity for identity, values in context.sources.items()
                                       if part['source_ref'] in values)
        for rule in target_rules:
            semantic = rule.get('item', rule.get('effective_rule', {}))
            for evidence in semantic.get('evidence', []):
                original_ranges.add(evidence['range_id'])
                refs.extend(references(evidence['range_id']))
        seed_ranges.update(original_ranges)
        associated = _analysis_context(context, target_rules, original_ranges, global_issues)

        def collect_references(value):
            if isinstance(value, dict):
                if 'range_id' in value:
                    refs.extend(references(value['range_id']))
                for entry in value.values():
                    collect_references(entry)
            elif isinstance(value, list):
                for entry in value:
                    collect_references(entry)

        collect_references(associated)
        for ref in refs:
            register(ref, alias)
        response_context_supports[alias] = list(dict.fromkeys(identity for ref in table_context
            if (identity := register(ref, alias))))
        source_context_supports[alias] = list(dict.fromkeys(identity for ref in source_context
            if (identity := register(ref, alias))))
        template_supports[alias] = []
        for part in target.get('template_refs', []):
            identity = register(part['source_ref'], alias, template=part)
            if identity:
                template_supports[alias].append(identity)
        for row in rows:
            identities = [register(ref, alias) for ref in [*row.get('context_source_excerpts', []), *row['source_excerpts']]]
            response_rows.append({'row_id': f'q{len(response_rows) + 1}', 'target_id': alias,
                'rule_ids': [by_ref[rule_ref_key(ref)]['rule_id'] for ref in row['rule_refs'] if rule_ref_key(ref) in by_ref],
                'support_ids': list(dict.fromkeys(identity for identity in identities if identity)),
                **({'source_join': row['source_join']} if row.get('source_join') else {})})
    material_aliases = {identity: f'k{i}' for i, identity in enumerate(needed, 1)}
    shared = {**basis['response'], 'project_background': '', 'approach': '',
              'shared_arrangements': [{**{k: v for k, v in entry.items() if k != 'material_ids'},
                                       'support_ids': [material_aliases[m] for m in entry['material_ids']]}
                  for entry in basis['response']['shared_arrangements']
                  if entry['material_ids'] and set(entry['material_ids']) <= needed.keys()]}
    supports = [{k: v for k, v in s.items() if k != 'reference'} for s in sources.values()]
    supports.extend(_material_views(list(needed.values()), task['targets']))
    payload = {'grounding_policy': AUTHORING_GROUNDING_POLICY, 'title_path': task['title_path'],
               'targets': [{'target_id': f't{i + 1}', 'kind': t['kind'], 'purpose': t['purpose'],
                            **({'subject': t['subject']} if t.get('subject') else {}),
                            'manual_fill': t.get('manual_fill', False),
                            'support_origins': t.get('support_origins', ['knowledge']),
                            'template_support_ids': template_supports[f't{i + 1}'],
                            **({'response_context_support_ids': response_context_supports[f't{i + 1}']}
                               if response_context_supports[f't{i + 1}'] else {}),
                            'rule_ids': [by_ref[rule_ref_key(ref)]['rule_id'] for ref in t['rule_refs'] if rule_ref_key(ref) in by_ref],
                            'support_ids': [s['support_id'] for s in supports if f't{i + 1}' in s['target_ids']
                                            and s['origin'] in t.get('support_origins', ['knowledge'])]}
                           for i, t in enumerate(task['targets'])],
               'rules': list({_json(r): r for r in rules}.values()),
               'rule_origin': context.payload.get('rule_origin', 'extraction'),
               'supports': supports, 'basis': shared,
               'input_issues': context.input['issues'], 'source_issues': issues,
               **_analysis_context(context, rules, seed_ranges, global_issues)}
    if response_rows:
        payload['response_rows'] = response_rows
    for descriptor, target in zip(payload['targets'], task['targets']):
        if target.get('source_body_contract'):
            descriptor['source_body_contract'] = target['source_body_contract']
        if target.get('source_body_contract') or target.get('project_context_excerpts'):
            descriptor['source_context_support_ids'] = source_context_supports[descriptor['target_id']]
        table = target.get('response_table') or {}
        if table.get('native_layout'):
            descriptor['native_response_layout'] = {key: value for key, value in table['native_layout'].items()
                                                     if key != 'source_ref'}
        if table.get('source_guidance'):
            descriptor['source_guidance'] = [
                {'kind': entry['kind'], 'support_ids': list(dict.fromkeys(identity for ref in entry['source_excerpts']
                    if (identity := register(ref, descriptor['target_id']))))}
                for entry in table['source_guidance']]
    return payload, {s['support_id']: s['reference'] for s in sources.values()}, issues


def bind_writing_response(task, raw, sources, materials, *, support_views=None):
    response = _parse(WritingResponse, raw)
    gaps, blocks, fills, row_fills = list(response['gaps']), [], [], {}
    targets = {f't{i + 1}': target for i, target in enumerate(task['targets'])}
    selected = {m['material_id']: m for m in materials}
    bindings = _support_bindings(sources, materials)
    if support_views is None:
        support_views = [*_material_views(materials, task['targets']),
                         *({'support_id': identity, 'origin': 'tender',
                            'target_ids': [key for key, target in targets.items() if ref in target['source_refs']]}
                           for identity, ref in sources.items())]
    views = {s['support_id']: s for s in support_views}

    def applies(identity, target_id):
        support, target = views.get(identity, {}), targets[target_id]
        return (not target.get('manual_fill', False)
                and support.get('origin') in target.get('support_origins', ['knowledge'])
                and target_id in support.get('target_ids', []))

    def grounded(content):
        target_ids = list(dict.fromkeys(content['target_ids']))
        support_ids = list(dict.fromkeys(content['support_ids']))
        if (not target_ids or not support_ids or set(target_ids) - targets.keys()
                or set(support_ids) - bindings.keys()):
            gaps.append('本处生成应答缺少可绑定的目标或资料依据，未采用，响应留空。')
            return None
        linked = [targets[i] for i in target_ids]
        if (any(not any(applies(s, t) for s in support_ids) for t in target_ids)
                or any(not any(applies(s, t) for t in target_ids) for s in support_ids)):
            gaps.append('本处生成应答引用了不属于该目标的资料或人工填写位置，未采用，响应留空。')
            return None
        return {**{k: v for k, v in content.items() if k != 'support_ids'},
                'target_ids': [t['target_id'] for t in linked],
                'material_ids': [bindings[s]['material_id'] for s in support_ids if bindings[s]['origin'] == 'knowledge'],
                'source_refs': list({_json(bindings[s]['source_ref']): bindings[s]['source_ref']
                    for s in support_ids if bindings[s]['origin'] == 'tender'}.values()),
                'source_excerpts': [bindings[s]['source_excerpt'] for s in support_ids
                    if bindings[s]['origin'] == 'tender' and bindings[s].get('source_excerpt')]}

    for block in response['blocks']:
        bound = grounded(block)
        if bound is not None:
            if any(t['kind'] == 'form' for t in task['targets'] if t['target_id'] in bound['target_ids']):
                gaps.append('表单使用已选原格式和格填值，未另采用重写正文；相关说明应安排独立正文位置。')
                continue
            blocks.append(bound)
    for fill in response['cell_fills']:
        if fill['source_id'] not in sources:
            gaps.append('填写值未能绑定原模板，请人工定位：' + fill['text'])
            continue
        bound = grounded(fill)
        if bound is not None:
            linked = [t for t in task['targets'] if t['target_id'] in bound['target_ids']]
            if any(t['kind'] != 'form' or sources[fill['source_id']] not in
                   [part['source_ref'] for part in t.get('template_refs', [])] for t in linked):
                gaps.append('填写值的模板不属于其引用的响应位置，未采用，原表对应单元格留空。')
                continue
            fills.append({'source_ref': sources[fill['source_id']],
                          **{k: v for k, v in bound.items() if k != 'source_id'}})
    rows = {f'q{ordinal}': (f't{number}', row) for ordinal, (number, row) in enumerate(
        ((number, row) for number, target in enumerate(task['targets'], 1)
         for row in (target.get('response_table') or {}).get('rows', [])), 1)}
    conflicted_rows = set()
    for fill in response.get('row_fills', []):
        if fill['row_id'] not in rows:
            gaps.append('企业应答未绑定本次技术响应行，未采用；原要求保留。')
            continue
        alias, row = rows[fill['row_id']]
        if (not fill['text'].strip() or not any(bindings.get(s, {}).get('origin') == 'knowledge'
                and applies(s, alias) for s in fill['support_ids'])):
            if fill['text'].strip():
                gaps.append('技术响应行没有适用的企业资料支持，应答留空，原要求保留。')
            continue
        value = grounded({**fill, 'target_ids': [alias]})
        if value is None:
            continue
        value = {k: v for k, v in value.items() if k not in {'target_ids', 'row_id'}}
        value['row_id'] = row['row_id']
        previous = row_fills.get(row['row_id'])
        if previous is not None and previous != value:
            conflicted_rows.add(row['row_id'])
            gaps.append('同一技术响应行收到冲突应答，该格留空待核，原要求保留。')
        row_fills[row['row_id']] = value
    row_fills = [value for key, value in row_fills.items() if key not in conflicted_rows]
    attachments = []
    for alias in dict.fromkeys(response['attachment_ids']):
        identity = bindings.get(alias, {}).get('material_id')
        material = selected.get(identity)
        if material is None:
            gaps.append('模型选择了未提供的资料引用，未将其作为证明。')
        elif (material['attachment'] and material['original_ref'] and material['text'].strip() and
              any(_material_applies(material, t) and (t['kind'] == 'attachment'
                  or material.get('locator', {}).get('kind') != 'search_excerpt') for t in task['targets'])):
            attachments.append({'material_id': identity, 'title': material['title'], 'artifact_ref': material['original_ref']})
    covered = {identity for content in [*blocks, *fills] for identity in content['target_ids']}
    answered = {fill['row_id'] for fill in row_fills}
    covered.update(target['target_id'] for target in task['targets']
                   if any(row['row_id'] in answered for row in (target.get('response_table') or {}).get('rows', [])))
    covered.update(t['target_id'] for t in task['targets'] if t['kind'] == 'attachment' and
                   any(_material_applies(selected[a['material_id']], t) for a in attachments))
    for target in task['targets']:
        if target['target_id'] not in covered:
            gaps.append('本响应位置未取得有依据的应答，保留空位：' + target['purpose'])
    has_content = _has_content({'blocks': blocks, 'cell_fills': fills, 'row_fills': row_fills, 'attachments': attachments})
    if not has_content:
        gaps.append('本响应位置尚无实际正文、填写值或证明原件，待补充。')
    return WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                       target_ids=[t['target_id'] for t in task['targets']],
                       status='generated' if has_content else 'placeholder', blocks=blocks,
                       cell_fills=fills, row_fills=row_fills, attachments=attachments, gaps=gaps).model_dump()


def _has_content(unit):
    return bool(any(b['text'].strip() or any(h.strip() for h in b['headers']) or
                    any(any(c.strip() for c in row) for row in b['rows']) for b in unit['blocks'])
                or any(f['text'].strip() for f in [*unit['cell_fills'], *unit.get('row_fills', [])])
                or unit['attachments'])


def _review_record(task, unit, payload, sources, materials, *, rule_refs=None):
    if (not _has_content(unit) and not materials
            and all(target['kind'] == 'form' for target in task['targets'])):
        return None
    from docreader.tender.authoring_revision import _previous_content
    content = _previous_content(task, unit, payload, sources, _support_bindings(sources, materials))
    if content is None:
        unit['gaps'].append('已有内容的依据无法绑定本次复核，保留草稿待核。')
        return None
    return {'task_id': task['task_id'], 'title_path': task['title_path'],
            '_coverage_refs': task_coverage_refs(task, payload, rule_refs or {}),
            'rules': payload['rules'], 'supports': payload['supports'], 'targets': payload['targets'], 'basis': payload['basis'],
            **{key: payload[key] for key in ('analysis_issues', 'issue_resolutions', 'relations',
                                           'unverified_relation_ids', 'related_rules')},
            **({'response_rows': payload['response_rows']} if payload.get('response_rows') else {}),
            'content': {'status': unit['status'], **content}}


def _content_rejected(finding):
    return finding.get('reject_content') is True or finding['code'] == 'unsupported_content'


def _adopt_reviewed(units, findings, artifacts, superseded):
    """Remove explicit semantic rejections from the version that was reviewed."""
    result = []
    for unit in units:
        rejected = [f for f in findings if unit['task_id'] in f['task_ids'] and _content_rejected(f)]
        if not rejected:
            result.append(unit)
            continue
        value = deepcopy(unit)
        clear = any(not f.get('content_ids') for f in rejected)
        ids = {i for f in rejected for i in f.get('content_ids', [])}
        value['blocks'] = [b for i, b in enumerate(unit['blocks'], 1) if not clear and f'b{i}' not in ids]
        value['cell_fills'] = [f for i, f in enumerate(unit['cell_fills'], 1) if not clear and f'f{i}' not in ids]
        value['row_fills'] = [f for i, f in enumerate(unit.get('row_fills', []), 1) if not clear and f'r{i}' not in ids]
        value['attachments'] = [a for i, a in enumerate(unit['attachments'], 1) if not clear and f'a{i}' not in ids]
        value['status'] = 'generated' if _has_content(value) else 'placeholder'
        value['gaps'].append('复核明确否定的内容未采用，相应位置留空；其他有据内容保留。')
        if value != unit:
            superseded.append(artifacts.put_json(unit))
        result.append(value)
    return result


async def generate_document(artifacts, snapshot, budget, execute, *, materials=(), knowledge=None,
                            concurrency=2, review_request=None, submission_sources=(), on_outline=None,
                            outline=None, outline_only=False, saved_units=(), on_unit=None,
                            saved_plan=None, on_plan=None, directory_plans=None, directory_documents=None,
                            directory_contract=None, response_form_contract=None, parameter_source_contract=None,
                            source_group_contract=None, native_requirement_contract=None, source_carrier_contract=None,
                            reviewed_source_facts_ref=None, on_source_closure=None, on_reviewed_facts=None,
                            render_pdf=None, on_layout=None, saved_layout_exports=None, reviewed_body_entry_contract=None,
                            native_body_contract=None, on_source_bodies=None, requirement_chain_contract=None):
    """Plan an outline or compile its fixed tree; the executor supplies document usage."""
    if directory_plans is not None and directory_documents is None:
        raise ValueError('directory plans require registered documents')
    if reviewed_body_entry_contract not in (None, REVIEWED_BODY_ENTRY_CONTRACT):
        raise ModelConfigurationError('unsupported reviewed body entry contract')
    from docreader.tender.native_body_selection import CONTRACT as NATIVE_BODY_CONTRACT
    if native_body_contract not in (None, NATIVE_BODY_CONTRACT):
        raise ModelConfigurationError('unsupported final source body contract')
    if native_body_contract is not None and (outline is not None or not outline_only):
        raise ModelConfigurationError('final source resolution requires automatic directory outline generation')
    from docreader.tender.item_placement import REQUIREMENT_CHAIN_CONTRACT
    if requirement_chain_contract not in (None, REQUIREMENT_CHAIN_CONTRACT):
        raise ModelConfigurationError('unsupported requirement chain contract')
    if requirement_chain_contract is not None and (
            outline is not None or not outline_only or directory_contract != 'bid_format_source_outline_v1'):
        raise ModelConfigurationError('the requirement chain builds an automatic bid-format outline')
    chain_items = chain_added = None
    source_body_result = None
    snapshot = AuthoringInput.model_validate(snapshot).model_dump()
    if outline is None and directory_documents is None and directory_plans is None and outline_only:
        loaded = None
        if snapshot.get('frozen_ref'):
            try:
                frozen = await asyncio.to_thread(artifacts.read_json, snapshot['frozen_ref'])
                loaded = await asyncio.to_thread(documents_from_frozen, artifacts, frozen)
            except DirectoryPlanRejected:
                loaded = []
            except (OSError, ValueError, KeyError):
                loaded = None
        if loaded:
            directory_documents = loaded
        else:
            directory_documents, directory_plans = [], []
    reviewed_source_facts = ()
    reviewed_fact_records = None
    if reviewed_source_facts_ref is not None and outline is None:
        if directory_documents is None:
            raise ModelConfigurationError('reviewed source facts require the frozen directory outline flow')
        if source_carrier_contract != SOURCE_CARRIER_CONTRACT:
            raise ModelConfigurationError('reviewed source facts require source_carriers_closed_v1')
        if directory_plans is not None:
            raise ModelConfigurationError('reviewed source facts require the model directory and material outline flow')
        reviewed_source_facts = await asyncio.to_thread(
            load_reviewed_facts, artifacts, snapshot, directory_documents, reviewed_source_facts_ref)
    if directory_documents is not None:
        if outline is not None:
            raise ValueError('directory compilation replaces a model outline')
        directory_facts = tuple(f for f in reviewed_source_facts if f['kind'] != 'credential_slot')
        if directory_plans is None and directory_contract == 'bid_format_source_outline_v1':
            if execute is None:
                raise ModelConfigurationError('bid-format semantic selection requires a model')
            from docreader.tender.bid_format_outline import select_bid_format_plan
            selected = await select_bid_format_plan(directory_documents, budget, execute, snapshot)
            if selected and requirement_chain_contract is not None:
                from docreader.tender.directory_compile import bid_format_section
                from docreader.tender.item_placement import _items, add_submission_forms
                context = await asyncio.to_thread(_OutlineContext, artifacts, snapshot)
                chain_items = await asyncio.to_thread(_items, context)
                found = bid_format_section(directory_documents)
                format_pages = {(row[0], row[1]) for row in (found[0], *found[1])} if found else set()
                selected, chain_added = add_submission_forms(selected, directory_documents, chain_items, format_pages)
            directory_plans = [selected] if selected else []
        elif directory_plans is None and execute is None:
            from docreader.tender.directory_compile import bid_format_plan
            host = bid_format_plan(directory_documents)
            directory_plans = [host] if host else []
        fetch_details = directory_plans is None and bool(directory_documents)
        if directory_plans is None:
            directory_plans, plan_failures = ([], [])
            if directory_documents:
                directory_plans, plan_failures = await _request_directory_plans(
                    directory_documents, budget, execute, snapshot, contract=directory_contract)
        else:
            plan_failures = []
        compiled = compile_plans(directory_plans, directory_documents)
        compiled['issues'].extend({'code': 'outline_directory_plan_failed', 'detail': detail} for detail in plan_failures)
        if fetch_details and compiled['chapters']:
            source_options = {}
            if directory_facts:
                source_options['reviewed_source_facts'] = directory_facts
            if reviewed_body_entry_contract is not None:
                source_options['reviewed_body_entry_contract'] = reviewed_body_entry_contract
            if on_source_closure is not None:
                source_options['on_source_closure'] = on_source_closure
            if response_form_contract is not None:
                source_options['response_form_contract'] = response_form_contract
            if parameter_source_contract is not None:
                source_options['parameter_source_contract'] = parameter_source_contract
            if source_group_contract is not None:
                source_options['source_group_contract'] = source_group_contract
            if source_carrier_contract is not None:
                source_options['source_carrier_contract'] = source_carrier_contract
            if native_requirement_contract is not None:
                source_options['native_requirement_contract'] = native_requirement_contract
                source_options['native_requirement_concurrency'] = concurrency
            if source_options:
                source_options['artifacts'] = artifacts
            compiled = await _request_directory_details(
                compiled, directory_documents, budget, execute, snapshot, **source_options)
        elif directory_facts:
            # A failed to supply a usable directory. Preserve every reviewed fact
            # as named pending work without manufacturing a material or C owner.
            compiled['_reviewed_required_native_table_ids'] = sorted(f['_table_id'] for f in directory_facts if f['kind'] == 'required_native')
            apply_reviewed_material_facts(compiled, directory_documents, directory_facts)
            apply_carrier_closure(compiled, directory_documents, [], [])
            bind_reviewed_carrier_results(compiled, directory_facts)
            apply_reviewed_body_dispositions(compiled, directory_documents, [
                {'fact_id': f['fact_id'], 'scope': 'unassigned', 'chapter_id': None}
                for f in directory_facts if f['kind'] == 'independent_body_response'], directory_facts,
                **({'entry_contract': reviewed_body_entry_contract} if reviewed_body_entry_contract is not None else {}))
            if on_source_closure is not None:
                await on_source_closure(deepcopy(compiled.get('_source_carrier_closure', [])))
        outline = await asyncio.to_thread(outline_from_compiled, snapshot, compiled, artifacts=artifacts)
        if chain_items is not None:
            from docreader.tender.item_placement import place_items
            from docreader.tender.opening_project_name import attach_opening_project_name
            outline, _placement = await place_items(
                outline, chain_items, budget, execute, _ask,
                lambda payload: _fits(budget, AuthoringTask.ITEM_PLACEMENT, payload), outside_format=chain_added or ())
            attach_w2_proposal_answers(outline)
            await asyncio.to_thread(attach_opening_project_name, artifacts, outline)
        if directory_facts:
            await asyncio.to_thread(bind_reviewed_outline_facts, outline, compiled, directory_facts)
        if native_body_contract is not None:
            from docreader.tender.native_body_scope import NativeBodyScope
            from docreader.tender.native_body_flow import resolve_native_bodies
            scope = await asyncio.to_thread(NativeBodyScope, artifacts, snapshot, directory_documents, compiled, outline)
            outline, source_body_result = await resolve_native_bodies(artifacts, snapshot, outline, scope, budget, execute)
        if reviewed_source_facts_ref is not None:
            await asyncio.to_thread(bind_reviewed_credential_slots, outline, compiled, reviewed_source_facts)
            reviewed_fact_records = deepcopy(compiled.get('_reviewed_source_fact_applications', []))
    elif native_body_contract is not None:
        raise ModelConfigurationError('final source resolution requires the registered directory source flow')
    supplied_outline = outline is not None
    if supplied_outline:
        parsed = AuthoringOutline.model_validate(outline)
        if parsed.authoring_input.model_dump() != snapshot:
            raise ValueError('fixed outline belongs to another authoring snapshot')
        outline = parsed.model_dump(exclude_unset=True)
        outline['authoring_input'] = deepcopy(snapshot)
    if review_request is not None:
        from docreader.tender.schema import AuthoringReviewRequest
        review_request = AuthoringReviewRequest.model_validate(review_request).model_dump()
    context = await asyncio.to_thread(_OutlineContext, artifacts, snapshot)
    materials, gaps = ([], []) if outline_only else prepare_materials(artifacts, materials)
    gaps.extend(_gap(i['code'], i['detail']) for i in context.availability_issues)
    gaps.extend(_gap(i['code'], '上游分析待核对：' + i['detail']) for i in context.payload.get('issues', [])
                if i.get('issue_id'))
    structure_results = {'findings': [], 'gaps': []}
    if not supplied_outline:
        organization_options = {'submission_sources': submission_sources} if submission_sources else {}
        outline = await _organize(artifacts, context, budget, execute, gaps,
                                  structure_results=structure_results, concurrency=concurrency, **organization_options)
    elif not outline_only and directory_plans is None:
        # Rebuild the same paragraph references registered during automatic organization.
        await asyncio.to_thread(prepare_template_parts, artifacts, context)
        from docreader.tender.authoring_structure import review_structure
        findings, structure_gaps = await review_structure(
            artifacts, outline, [], budget, execute, outline_phase=True)
        structure_results.update(findings=findings, gaps=structure_gaps)
    gaps.extend(structure_results['gaps'])
    if supplied_outline:
        _note_unassigned_rules(outline, context)
    if not any(c['targets'] for c in _flatten(outline['chapters'])):
        gaps.append(_gap('outline_empty', '当前大纲没有实际响应位置，保留待补正文；不代表已覆盖招标要求。'))
        if not supplied_outline:
            outline['chapters'].append({'chapter_id': f"chapter-{len(outline['chapters']) + 1}",
                'title': '待补响应内容', 'targets': [{'target_id': 'pending-response', 'kind': 'narrative',
                    'purpose': '仅依据可用资料起草方法，完整招标要求和响应结构待补充。', 'rule_refs': [], 'source_refs': []}],
                'children': [], 'is_volume': False})
    if outline_only:
        issues = [*outline['issues'], *({'code': gap['code'], 'detail': gap['detail'], 'rule_refs': [],
                  **({'target_id': gap['target_id']} if gap.get('target_id') else {})} for gap in gaps)]
        outline['issues'] = list({_json(issue): issue for issue in issues}.values())
    from docreader.tender.source_body import validate_outline_source_bodies
    await asyncio.to_thread(validate_outline_source_bodies, artifacts, outline)
    outline_ref = artifacts.put_json(outline)
    if on_reviewed_facts is not None and reviewed_fact_records is not None:
        await on_reviewed_facts(outline_ref, reviewed_fact_records)
    if source_body_result is not None:
        body_receipt_ref = artifacts.put_json({**source_body_result, 'authoring_input': snapshot,
            'outline_ref': outline_ref, 'reviewed_source_facts_ref': reviewed_source_facts_ref})
        if on_source_bodies is not None:
            await on_source_bodies(body_receipt_ref)
        if not source_body_result['complete']:
            from docreader.tender.native_body_flow import SourceBodyResolutionPending
            raise SourceBodyResolutionPending('Final source bodies require named source review; diagnostic receipt retained.')
    if on_outline is not None:
        await on_outline(outline_ref)
    if outline_only:
        return outline_ref
    from docreader.tender.authoring_requirements import preserve_requirements
    requirements = await asyncio.to_thread(preserve_requirements, context, outline)
    requirements_ref = artifacts.put_json(requirements)
    gaps.extend(_gap(issue['code'], issue['detail']) for issue in requirements['issues'])
    tasks = plan_writing_tasks(outline)
    requested = set(review_request['target_ids']) if review_request else set()
    available = {target['target_id'] for task in tasks for target in task['targets']}
    if requested - available:
        gaps.append(_gap('review_request_unbound', '部分重审位置未能绑定当前大纲，保留待核；其余位置继续。'))
    entries = build_ledger(outline, requirements)
    query_plan = {'queries': [], 'search': [], 'gaps': []}
    knowledge_issues = []
    if knowledge is not None:
        from docreader.tender.authoring_coverage import apply_retrieved_knowledge, tender_source_identity
        identity = tender_source_identity(artifacts, snapshot)
        scope = {**knowledge.spec(), 'exclude_content_hashes': identity['source_shas'],
                 'exclude_document_ids': identity['document_ids']}
        texts = await asyncio.to_thread(_excerpt_texts, artifacts, outline)
        query_plan = plan_queries(outline, tasks, entries, requirements, texts, scope)
        gaps.extend(query_plan['gaps'])
        if query_plan['search']:
            answer_targets = [target for task in tasks for target in task['targets']
                              if target.get('support_origins') == ['knowledge']]
            consumed = await apply_retrieved_knowledge(
                knowledge, query_plan['search'], artifacts, identity, answer_targets)
            retrieved = {'materials': consumed['materials'], 'issues': consumed['issues']}
            retrieved_materials = consumed['materials']
            combined, issues = prepare_materials(artifacts, [*materials, *retrieved_materials])
            materials = combined
            gaps.extend(issues)
            knowledge_issues = retrieved['issues']
            gaps.extend({k: v for k, v in issue.items() if k in {'code', 'detail', 'target_id'}}
                        for issue in knowledge_issues)
    # Summarize supplied methods only. Tender requirements are never bidder proof.
    basis_response = AuthoringBasisContent().model_dump()
    candidates = [m for m in materials if m['text'].strip() and
                  any(_material_applies(m, t) for task in tasks for t in task['targets'])]
    basis_targets = [(task['title_path'], target) for task in tasks for target in task['targets']
                     if not target.get('manual_fill', False)]
    rule_views = {r['rule_id']: r for r in context.payload['rules']}
    by_ref = {rule_ref_key(ref): rule_views[identity] for identity, ref in context.rules.items()}
    material_aliases = {m['material_id']: f'k{i}' for i, m in enumerate(candidates, 1)}
    by_alias = {alias: identity for identity, alias in material_aliases.items()}
    basis_views = _material_views(candidates, [t for _, t in basis_targets])
    basis_records = [{'support': view} for view in basis_views]
    def basis_payload(batch):
        selected = {row['support']['support_id'] for row in batch}
        batch_materials = {by_alias[alias] for alias in selected}
        related = [(i, path, target) for i, (path, target) in enumerate(basis_targets, 1)
                   if any(m['material_id'] in batch_materials and _material_applies(m, target) for m in candidates)]
        rules = list({rule_ref_key(ref): by_ref[rule_ref_key(ref)] for _, _, t in related for ref in t['rule_refs']
                      if rule_ref_key(ref) in by_ref}.values())
        return {'grounding_policy': AUTHORING_GROUNDING_POLICY,
                'supports': [view for view in basis_views if view['support_id'] in selected],
                'input_issues': snapshot['issues'],
                'rules': rules,
                'targets': [{'target_id': f't{i}', 'title_path': path, 'kind': t['kind'], 'purpose': t['purpose'],
                             'rule_ids': [by_ref[rule_ref_key(ref)]['rule_id'] for ref in t['rule_refs'] if rule_ref_key(ref) in by_ref],
                             'support_ids': [material_aliases[m['material_id']] for m in candidates
                                              if m['material_id'] in batch_materials and _material_applies(m, t)]}
                            for i, path, t in related],
                'rule_origin': context.payload.get('rule_origin', 'extraction'),
                **_analysis_context(context, rules)}
    # Each batch reads its own evidence once. The host retains previously bound
    # arrangements instead of repeatedly asking the model to copy and recheck them.
    basis_batches = list(_batches(basis_records, budget, 'authoring_basis', basis_payload))
    for batch in basis_batches:
        try:
            def validate_basis(raw):
                value = _parse(AuthoringBasisResponse, raw)
                value['project_background'] = value['approach'] = ''
                if len(_json(value).encode()) > 12000:
                    raise ModelContractError([{'path': [], 'code': 'basis_summary_too_large',
                        'detail': 'Keep the shared proposal within 12000 UTF-8 bytes; detailed requirements remain in writing tasks.'}])
                allowed = {s['support_id'] for s in basis_payload(batch)['supports']}
                arrangements = []
                for entry in value['shared_arrangements']:
                    if not entry['support_ids'] or set(entry['support_ids']) - allowed:
                        value['gaps'].append('一项共同安排缺少可用资料引用，未采用，相关应答留空。')
                        continue
                    arrangements.append({**{k: v for k, v in entry.items() if k != 'support_ids'},
                                         'material_ids': list(dict.fromkeys(by_alias[s] for s in entry['support_ids']))})
                return AuthoringBasisContent(**{**value, 'shared_arrangements': arrangements}).model_dump()
            value = await _ask(execute, budget, 'authoring_basis', basis_payload(batch), validate_basis)
            basis_response['shared_arrangements'] = list({_json(entry): entry for entry in [
                *basis_response['shared_arrangements'], *value['shared_arrangements']]}.values())
            basis_response['gaps'].extend(value['gaps'])
        except _LOCAL_FAILURES as error:
            gaps.append(_gap('basis_partial', f'部分共同依据未形成（{type(error).__name__}）；保留已有方案和待补信息。'))
    known_materials = {m['material_id'] for m in candidates}
    supported = []
    for arrangement in basis_response['shared_arrangements']:
        if not arrangement['material_ids'] or set(arrangement['material_ids']) - known_materials:
            basis_response['gaps'].append('一项共同安排缺少可用资料引用，未采用，相关应答留空。')
        else:
            supported.append(arrangement)
    basis_response['shared_arrangements'] = supported
    basis = AuthoringBasis(authoring_input=snapshot, response=basis_response, materials=materials).model_dump()
    basis_ref = artifacts.put_json(basis)
    materials_ref = artifacts.put_json(materials)
    plan_digest = _plan_input_digest(snapshot, outline_ref, materials_ref, basis_ref, budget)
    gaps.extend(_gap('basis_gap', detail) for detail in basis_response['gaps'] if detail.strip())
    # Keep forms as independent instances. Narrative may split at a target boundary
    # when its complete context does not fit; it is never split by page count.
    from docreader.tender.authoring_fragments import split_writing_task
    planned = []
    saved_by_id = {unit['task_id']: unit for unit in saved_units or []
                   if isinstance(unit, dict) and unit.get('task_id')}

    async def fit_task(task):
        if all(t.get('manual_fill', False) for t in task['targets']):
            planned.append(task)
            return
        payload, sources, _ = await asyncio.to_thread(_load_task, artifacts, context, task, basis, materials)
        if not _has_allowed_support(payload):
            planned.append(task)
            return
        hit, _ = await _replay(execute, 'authoring_write', _request(budget, 'authoring_write', payload),
            lambda raw: bind_writing_response(task, raw, sources, _task_materials(materials, task),
                                               support_views=payload['supports']))
        if hit or await asyncio.to_thread(_fits, budget, 'authoring_write', payload):
            planned.append(task)
            return
        groups = _retrieval_groups(materials, task)
        unique = list(dict.fromkeys(identity for values in groups.values() for identity in values))
        if groups and len(unique) > len(groups):
            limit = len(unique) - 1
            while limit >= len(groups):
                chosen, omitted = fair_select(groups, limit)
                trial = {**task, '_selected_material_ids': chosen}
                trial_payload, _, _ = await asyncio.to_thread(_load_task, artifacts, context, trial, basis, materials)
                if await asyncio.to_thread(_fits, budget, 'authoring_write', trial_payload):
                    task['_selected_material_ids'] = chosen
                    if omitted:
                        gaps.append(_gap('knowledge_candidates_not_loaded',
                                         '部分知识候选未装入本单元，完整片段仍保留在资料快照。', task))
                    planned.append(task)
                    return
                limit -= 1
            gaps.append(_gap('knowledge_candidates_not_loaded',
                             '知识候选无法按完整片段装入本单元，资料快照仍保留。', task))
        if len(task['targets']) > 1:
            middle = len(task['targets']) // 2
            parts = [{**task, 'targets': task['targets'][:middle]}, {**task, 'targets': task['targets'][middle:]}]
        else:
            parts = split_writing_task(task)
        if not parts:
            planned.append(task)  # write_one keeps the original carrier and a located capacity gap.
            return
        for part in parts:
            scope = hashlib.sha256(_json(part['targets']).encode()).hexdigest()[:16]
            part['task_id'] = task['task_id'].split(':part-')[0] + ':part-' + scope
            await fit_task(part)

    if saved_plan is not None:
        plan = AuthoringDocumentPlan.model_validate(saved_plan).model_dump(by_alias=True, exclude_unset=True)
        if plan['input_digest'] != plan_digest or any(plan[key] != value for key, value in (
                ('outline_ref', outline_ref), ('materials_ref', materials_ref), ('basis_ref', basis_ref))):
            raise Conflict('stored writing plan inputs changed; preserve this execution and create an explicit successor')
        planned = plan['tasks']
        gaps.extend(plan.get('gaps', []))
    else:
        start = len(gaps)
        for task in tasks:
            await fit_task(task)
        plan = AuthoringDocumentPlan(input_digest=plan_digest, outline_ref=outline_ref,
            materials_ref=materials_ref, basis_ref=basis_ref, tasks=planned,
            gaps=gaps[start:]).model_dump(by_alias=True, exclude_unset=True)
        planned = plan['tasks']
    plan_ref = artifacts.put_json(plan)
    if on_plan is not None:
        await on_plan(plan_ref)

    async def write_one(task):
        payload, sources, source_issues = await asyncio.to_thread(_load_task, artifacts, context, task, basis, materials)
        selected = _task_materials(materials, task)
        input_digest = _writing_input_digest(task, payload, sources, selected, budget)
        def done(unit, record):
            unit['input_digest'] = input_digest
            return unit, record
        if all(t.get('manual_fill', False) for t in task['targets']):
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                blocks=[{'kind': 'paragraph', 'text': ('________（待附证明材料）' if t['kind'] == 'attachment' else
                        '我方响应：________（人工填写）\n证明材料或附件位置：________（原条款要求时填写）'
                        if t.get('support_origins') == ['tender'] else '________（人工填写）')}
                        for t in task['targets'] if t['kind'] != 'form'],
                gaps=['此处为人工填写位置；最终报价等数据留空，不由模型代填或按0计算。']).model_dump()
            unit['gaps'].extend(source_issues)
            return done(unit, None)
        if all(t['kind'] == 'form' and not t.get('response_table') for t in task['targets']) and not any(
                support.get('grid') and support['support_id'] in target.get('template_support_ids', [])
                for target in payload['targets'] for support in payload['supports']):
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                gaps=['文字原格式保留空位，或实际原表尚未定位；不另外生成一份重写声明。', *source_issues]).model_dump()
            return done(unit, None)
        if (all(t['kind'] == 'attachment' for t in task['targets']) and
                not any(s.get('attachment') for s in payload['supports'] if s['origin'] == 'knowledge')):
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                gaps=['当前响应位置没有可用的真实证明原件，保留附件位置待人工补充。']).model_dump()
            unit['gaps'].extend(source_issues)
            return done(unit, None)
        if not _has_allowed_support(payload):
            answer_only = all(str(t.get('target_id', '')).endswith(':answer') for t in task['targets'])
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                blocks=[AuthoringBlock(kind='paragraph') for _ in task['targets']] if answer_only else [],
                gaps=['未取得本位置允许使用的可读依据，不生成无依据应答；原模板及响应位置保留空白。']).model_dump()
            unit['gaps'].extend(source_issues)
            return done(unit, None)
        saved = saved_by_id.get(task['task_id'])
        if saved is not None and saved.get('input_digest') == input_digest:
            saved = WritingUnit.model_validate(saved).model_dump()
            record = None if saved.get('error_type') else _review_record(
                task, saved, payload, sources, selected, rule_refs=context.rules)
            return done(saved, record)
        try:
            unit = await _ask(execute, budget, 'authoring_write', payload,
                              lambda raw: bind_writing_response(task, raw, sources,
                                  _task_materials(materials, task), support_views=payload['supports']))
        except _LOCAL_FAILURES as error:
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                               target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                               error_type=type(error).__name__, gaps=['本单元生成未完成，已保留待补位置。']).model_dump()
        unit['gaps'].extend(source_issues)
        record = None if unit.get('error_type') else _review_record(
            task, unit, payload, sources, _task_materials(materials, task), rule_refs=context.rules)
        return done(unit, record)
    limiter = asyncio.Semaphore(concurrency)
    async def bounded(task):
        async with limiter:
            unit, record = await write_one(task)
            if on_unit is not None and saved_by_id.get(task['task_id']) != unit:
                await on_unit(unit)
            return unit, record
    pending = [asyncio.create_task(bounded(task)) for task in planned]
    try:
        written = await asyncio.gather(*pending)
    except BaseException:
        for work in pending:
            work.cancel()
        await asyncio.gather(*pending, return_exceptions=True)
        raise
    units, unit_refs, review_records = [], [], []
    for task, (unit, review_record) in zip(planned, written):
        units.append(unit)
        unit_refs.append(artifacts.put_json(unit))
        if review_record is not None:
            review_records.append(review_record)
    def review_scope(task):
        return [f't{i}' for i, target in enumerate(task['targets'], 1) if target['target_id'] in requested]
    accepted_assessments = []
    coverage_index = coverage_scope_index(planned)
    async def review(records, previous=(), unit_by_task=None, *, review_round=0):
        findings, completed, replaced = [], set(), set()
        unit_by_task = unit_by_task or {}
        def review_payload(batch):
            identities = {r['task_id'] for r in batch}
            scopes = [{'task_id': task['task_id'], 'target_ids': review_scope(task)} for task in planned
                      if task['task_id'] in identities and review_scope(task)]
            coverage, _ = batch_coverage_view(entries, planned, batch, scope_index=coverage_index)
            return {'grounding_policy': AUTHORING_GROUNDING_POLICY,
                    'units': [{k: v for k, v in record.items() if k != '_coverage_refs'} for record in batch],
                    **({'coverage': coverage} if coverage else {}),
                    **({'review_request': {'reason': review_request['reason'], 'targets': scopes}} if scopes else {}),
                    'previous_findings': [{**{k: v for k, v in f.items() if k != 'content_ids'},
                                           'task_ids': [identity for identity in f['task_ids'] if identity in identities],
                                           'partial_context': bool(set(f['task_ids']) - identities),
                                           'original_content_ids': f.get('content_ids', [])}
                        for f in previous if set(f['task_ids']) & identities]}
        for batch in _batches(records, budget, 'authoring_review', review_payload):
            ids = {entry['task_id'] for entry in batch}
            def validate_review(raw):
                body = {key: value for key, value in raw.items() if key != 'assessments'} if isinstance(raw, dict) else raw
                value = _parse(AuthoringReviewResponse, body)
                for finding in value['findings']:
                    if set(finding['task_ids']) - ids:
                        raise ModelContractError([{'path': ['findings'], 'code': 'review_unknown_task',
                                                  'detail': 'Select only tasks supplied in this review batch.'}])
                    if _content_rejected(finding) and not finding['task_ids']:
                        raise ModelContractError([{'path': ['findings'], 'code': 'review_unlocated_content',
                                                  'detail': 'Identify the supplied task whose content is rejected.'}])
                    for identity in finding['task_ids']:
                        record = next(r for r in batch if r['task_id'] == identity)
                        content_ids = {entry['content_id'] for key in ('blocks', 'cell_fills', 'row_fills', 'attachments')
                                       for entry in record['content'].get(key, [])}
                        if set(finding['content_ids']) - content_ids:
                            raise ModelContractError([{'path': ['findings'], 'code': 'review_unknown_content',
                                                      'detail': 'Select content references in the identified task.'}])
                normalized = []
                for finding in value['findings']:
                    if _content_rejected(finding):
                        normalized.extend({**finding, 'task_ids': [identity]} for identity in finding['task_ids'])
                    else:
                        normalized.append(finding)
                value['findings'] = normalized
                _, mapping = batch_coverage_view(entries, planned, batch, scope_index=coverage_index)
                supplied = raw.get('assessments') if isinstance(raw, dict) else None
                accepted = accept_assessments(supplied, mapping, batch)
                if isinstance(supplied, list) and len(accepted) < len(supplied):
                    value['findings'].append({'code': 'coverage_assessment_invalid',
                        'detail': '部分逐要求判定未能绑定本批允许的内容或依据，保留正文及有效兄弟判定，相关要求待核。',
                        'task_ids': sorted(ids), 'content_ids': [], 'suggestion': '',
                        'can_revise': False, 'reject_content': False})
                for item in accepted:
                    item['review_round'] = review_round
                    item['unit_fps'] = {task_id: unit_fingerprint(unit_by_task[task_id])
                                        for task_id in item['reviewed_task_ids'] if task_id in unit_by_task}
                    item['reviewed_unit_refs'] = {task_id: artifacts.put_json(unit_by_task[task_id])
                        for task_id in item['reviewed_task_ids'] if task_id in unit_by_task}
                value['assessments'] = accepted
                return value
            try:
                value = await _ask(execute, budget, 'authoring_review', review_payload(batch), validate_review)
                findings.extend(value['findings'])
                accepted_assessments.extend(value.get('assessments') or [])
                completed.update(ids)
                replaced.update(i for i, f in enumerate(previous) if f['task_ids'] and set(f['task_ids']) <= ids)
            except _LOCAL_FAILURES as error:
                findings.append({'code': 'review_unavailable', 'detail': f'本批实际正文尚未复核（{type(error).__name__}）。',
                                 'task_ids': sorted(ids), 'suggestion': '保留文件，后续针对这些单元重试复核。'})
        return findings, completed, replaced
    findings, _, _ = await review(review_records, unit_by_task={unit['task_id']: unit for unit in units})
    initial_review_ref = artifacts.put_json(AuthoringReviewRecord(
        findings=findings, coverage_assessments=accepted_assessments).model_dump())
    from docreader.tender.authoring_revision import revise_units
    def revision_payload(task):
        payload, sources, _ = _load_task(artifacts, context, task, basis, materials)
        if review_scope(task):
            payload['review_request'] = {'reason': review_request['reason'], 'target_ids': review_scope(task)}
        return payload, sources, _support_bindings(sources, _task_materials(materials, task))
    def revision_bind(task, raw):
        payload, sources, source_issues = _load_task(artifacts, context, task, basis, materials)
        result = bind_writing_response(task, raw, sources, _task_materials(materials, task),
                                      support_views=payload['supports'])
        result['gaps'].extend(source_issues)
        return result
    superseded_refs = []
    safe_units = _adopt_reviewed(units, findings, artifacts, superseded_refs)
    revised, revision_refs, revision_gaps = await revise_units(
        artifacts, budget, execute, planned, units, findings, revision_payload, revision_bind)
    superseded_refs.extend(revision_refs)
    changed = {new['task_id'] for old, new in zip(units, revised) if old != new}
    if changed:
        affected = changed | {identity for f in findings if set(f['task_ids']) & changed for identity in f['task_ids']}
        records = []
        for task, unit in zip(planned, revised):
            if task['task_id'] in affected:
                payload, sources, _ = await asyncio.to_thread(_load_task, artifacts, context, task, basis, materials)
                record = _review_record(task, unit, payload, sources, _task_materials(materials, task), rule_refs=context.rules)
                if record is not None:
                    records.append(record)
        new_findings, completed, replaced = await review(
            records, findings, {unit['task_id']: unit for unit in revised}, review_round=1)
        adopted = []
        for old, new, safe in zip(units, revised, safe_units):
            identity = old['task_id']
            if identity in changed:
                if identity in completed:
                    adopted.extend(_adopt_reviewed([new], new_findings, artifacts, superseded_refs))
                else:
                    superseded_refs.append(artifacts.put_json(new))
                    adopted.append(safe)
            elif identity in completed:
                adopted.extend(_adopt_reviewed([old], [*findings, *new_findings], artifacts, superseded_refs))
            else:
                adopted.append(safe)
        safe_units = adopted
        findings = [f for i, f in enumerate(findings) if i not in replaced] + new_findings
    gaps.extend(revision_gaps)
    units = safe_units
    findings.extend(structure_results['findings'])
    superseded_refs = list({_json(ref): ref for ref in superseded_refs}.values())
    unit_refs = [artifacts.put_json(unit) for unit in units]
    for task, unit in zip(planned, units):
        gaps.extend(_gap('writing_gap', detail, task) for detail in unit['gaps'] if detail.strip())
    review_ref = artifacts.put_json(AuthoringReviewRecord(
        findings=findings, coverage_assessments=accepted_assessments).model_dump())
    settled = settle_coverage(entries, units, unit_refs, accepted_assessments, knowledge_issues, query_plan['queries'])
    retrieval_ref = artifacts.put_json(AuthoringQueryPlan.model_validate({'queries': query_plan['queries']}).model_dump())
    coverage = AuthoringCoverage(authoring_input=snapshot, outline_ref=outline_ref,
        requirements_ref=requirements_ref, retrieval_ref=retrieval_ref, unit_refs=unit_refs,
        review_ref=review_ref, entries=settled).model_dump()
    coverage_ref = artifacts.put_json(coverage)
    by_id = {t['task_id']: t for t in planned}
    for finding in findings:
        for task_id in finding['task_ids'] or [None]:
            gaps.append(_gap(finding['code'], finding['detail'] + (' 建议：' + finding['suggestion'] if finding['suggestion'] else ''),
                             by_id.get(task_id)))
    gaps.extend({'code': i['code'], 'detail': i['detail'],
                 **({'target_id': i['target_id']} if i.get('target_id') else {})} for i in outline['issues'])
    from docreader.tender.authoring_export import export_authoring_document
    export_digest = hashlib.sha256(_json({'contract': 'toc_layout_export_v1', 'outline': outline,
        'units': units, 'gaps': gaps, 'requirements': requirements, 'coverage': coverage}).encode()).hexdigest()
    saved_export = (saved_layout_exports or {}).get(export_digest) if render_pdf is not None else None
    if saved_export is not None:
        exported = await asyncio.to_thread(artifacts.read_json, saved_export)
        for key in ('docx_ref', 'package_ref', 'layout_ref'):
            await asyncio.to_thread(artifacts.path, exported[key])
        report = await asyncio.to_thread(artifacts.read_json, exported['layout_ref'])
        if report.get('status') != 'fixed' or report['documents'][0]['artifact_ref'] != exported['docx_ref']:
            raise ValueError('stored layout export is not bound to its verified document')
    else:
        layout_options = {'render_pdf': render_pdf} if render_pdf is not None else {}
        exported = await asyncio.to_thread(export_authoring_document, artifacts, outline, units, gaps,
                                          requirements=requirements, coverage=coverage, **layout_options)
        if on_layout is not None and exported.get('layout_ref') is not None:
            export_ref = await asyncio.to_thread(artifacts.put_json, exported)
            await on_layout({'layout_ref': exported['layout_ref'], 'outline_ref': outline_ref,
                'docx_ref': exported['docx_ref'], 'package_ref': exported['package_ref'],
                'export_input_digest': export_digest, 'export_result_ref': export_ref})
    all_gaps = list({_json(g): g for g in [*gaps, *exported['gaps']]}.values())
    result = AuthoringDocument(authoring_input=snapshot, outline_ref=outline_ref, requirements_ref=requirements_ref,
                               basis_ref=basis_ref,
                               unit_refs=unit_refs, review_ref=review_ref, initial_review_ref=initial_review_ref,
                               coverage_ref=coverage_ref,
                               superseded_unit_refs=superseded_refs, docx_ref=exported['docx_ref'],
                               package_ref=exported['package_ref'], gaps=all_gaps, usage=AuthoringUsage()).model_dump()
    return artifacts.put_json(result)
