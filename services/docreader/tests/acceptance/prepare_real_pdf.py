"""Offline private-PDF preflight: no model client, no network, no document text output."""
from __future__ import annotations

import argparse
import asyncio
from collections import Counter, defaultdict
import hashlib
import json
import logging
import math
import os
from pathlib import Path
import platform
import re
import stat
import time
import warnings

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.evidence import EvidenceRepository, freeze_sources, register_manifest
from docreader.tender.prompts import get_prompt
from docreader.tender.requirements.extract import extract_payload


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def token_interval(task, payload):
    text = get_prompt(task) + encoded(payload)
    # Planning envelope only. The exact tokenizer and generated requirements are
    # unknown; the upper estimate matches the application's byte-based admission.
    return {"low": math.ceil(len(text) / 6), "high": RequestBudget().measure(task, payload)['input_tokens']}


def validate_coverage(raw, packs, index):
    body = [entry for pack in packs for entry in pack['body']]
    by_id = {entry['range_id']: entry for entry in body}
    by_unit = defaultdict(list)
    for entry in body:
        by_unit[entry['unit_id']].append(entry)
    bad_text_units, bad_tables, uncovered_units, invalid_context, invalid_targets = 0, 0, 0, 0, 0
    invalid_pending, pending_text, invalid_bindings = 0, 0, 0
    for unit in raw['units']:
        ranges = by_unit[unit['unit_id']]
        if not ranges:
            uncovered_units += 1
        if unit.get('grid'):
            expected = set(range(unit['grid']['row_count']))
            actual = {row for entry in ranges for row in range(entry.get('row_start', -1), entry.get('row_end', -1))}
            bad_tables += actual != expected
        elif unit['kind'] == 'section':
            offset = 0
            for entry in sorted(ranges, key=lambda value: value['start']):
                if entry['start'] != offset or entry['text'] != unit['text'][entry['start']:entry['end']]:
                    bad_text_units += 1
                    break
                offset = entry['end']
            if offset != len(unit['text']):
                bad_text_units += 1
    for pack in packs:
        pack_ids = {entry['range_id'] for entry in pack['body']}
        for binding in pack.get('scope_bindings', []):
            invalid_targets += not set(binding['scope_binding']['applies_to_range_ids']).issubset(pack_ids)
            intervals = sorted((entry['start'], entry['end']) for entry in [*pack['body'], *pack['supporting']]
                               if entry.get('source_range_id', entry['range_id']) == binding['source_range_id'])
            covered = binding['start']
            for start, end in intervals:
                if start <= covered:
                    covered = max(covered, end)
            invalid_bindings += 'text' in binding or covered < binding['end']
        for context in pack['supporting']:
            parent_id = context.get('parent_range_id')
            if parent_id:
                parent = by_id.get(parent_id)
                if parent is None:
                    invalid_context += 1
                else:
                    start, end = context['start'] - parent['start'], context['end'] - parent['start']
                    if not (0 <= start <= end <= len(parent['text'])) or context['text'] != parent['text'][start:end]:
                        invalid_context += 1
            targets = context.get('scope_binding', {}).get('applies_to_range_ids', [])
            invalid_targets += not set(targets).issubset(pack_ids)
        for pending in pack.get('pending_scope_context', []):
            parent = by_id.get(pending.get('parent_range_id'))
            invalid_pending += (parent is None or pending['unit_id'] != parent['unit_id']
                                or not (parent['start'] <= pending['start'] <= pending['end'] <= parent['end']))
            pending_text += any(key in pending for key in ('text', 'cells', 'image_ref'))
            pending_text += any('cells' in (pending.get(key) or {}) for key in ('locator', 'physical_locator'))
            targets = pending.get('scope_binding', {}).get('applies_to_range_ids', [])
            invalid_targets += not set(targets).issubset(pack_ids)
    result = {
        'body_count_matches_index': len(body) == index['range_count'],
        'unique_body_range_count': len(by_id), 'duplicate_body_range_count': len(body) - len(by_id),
        'uncovered_native_unit_count': uncovered_units, 'noncontiguous_native_text_unit_count': bad_text_units,
        'incomplete_table_row_coverage_count': bad_tables, 'invalid_supporting_excerpt_count': invalid_context,
        'invalid_scope_target_count': invalid_targets,
        'invalid_pending_reference_count': invalid_pending, 'pending_context_with_text_count': pending_text,
        'invalid_scope_binding_count': invalid_bindings,
    }
    result['passed'] = result['body_count_matches_index'] and not any(result[key] for key in (
        'duplicate_body_range_count', 'uncovered_native_unit_count', 'noncontiguous_native_text_unit_count',
        'incomplete_table_row_coverage_count', 'invalid_supporting_excerpt_count', 'invalid_scope_target_count',
        'invalid_pending_reference_count', 'pending_context_with_text_count', 'invalid_scope_binding_count'))
    return result


def propagate_private_attributes(source, private_root):
    attributes = [(name, os.getxattr(source, name)) for name in os.listxattr(source)]
    for path in private_root.rglob('*'):
        if path.is_file():
            previous_mode = path.stat().st_mode
            try:
                path.chmod(stat.S_IMODE(previous_mode) | stat.S_IWUSR)
                for name, value in attributes:
                    os.setxattr(path, name, value)
            finally:
                path.chmod(stat.S_IMODE(previous_mode))
    return len(attributes)


async def prepare(args):
    logging.disable(logging.CRITICAL)
    warnings.filterwarnings('ignore')
    source = args.source.resolve()
    private_root = args.private_root.resolve()
    private_root.mkdir(mode=0o700, parents=True, exist_ok=True)
    private_root.chmod(0o700)
    started = time.monotonic()
    with source.open('rb') as stream:
        source_digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    if args.expected_sha256 and source_digest != args.expected_sha256:
        raise ValueError('Private source digest mismatch')
    artifacts = ArtifactStore(private_root / 'artifacts')
    if args.reuse_frozen:
        frozen_ref = json.loads((private_root / 'frozen-index-ref.json').read_text(encoding='utf-8'))
        registration_seconds = None
    else:
        document = {
            'document_id': 'private-real-pdf-1', 'document_version': source_digest,
            'role': 'main', 'path': str(source),
        }
        manifest = {'documents': [document]}
        if args.image_dispositions:
            manifest['image_dispositions'] = json.loads(args.image_dispositions.read_text())['image_dispositions']
        registered = register_manifest(manifest, artifacts)
        (private_root / 'registration.json').write_text(encoded(registered), encoding='utf-8')
        registration_seconds = time.monotonic() - started
        # No OCR callback: unknown images remain unresolved; explicit source-bound
        # reviewed decorative occurrences preserve their non-requirement basis.
        frozen_ref = await freeze_sources(registered, artifacts, budget=RequestBudget(args.context_tokens))
        (private_root / 'frozen-index-ref.json').write_text(encoded(frozen_ref), encoding='utf-8')
    index = artifacts.read_json(frozen_ref)
    if len(index['documents']) != 1 or 'raw_ref' not in index['documents'][0]:
        raise RuntimeError('Native parsing did not produce a frozen document; details remain private')
    raw = artifacts.read_json(index['documents'][0]['raw_ref'])
    if raw['original_ref']['sha256'] != source_digest:
        raise ValueError('Frozen source digest differs from requested source')
    packs = list(EvidenceRepository(artifacts).iter_packs(frozen_ref))
    preparation_seconds = time.monotonic() - started
    body = [entry for pack in packs for entry in pack['body']]
    classifications = Counter(page['classification'] for page in raw['source_contract']['page_manifest'])
    issues = Counter(problem['code'] if re.fullmatch('[a-z0-9_:-]+', problem['code']) else 'unclassified_issue'
                     for problem in index['issues'])
    scope_candidates = {entry['range_id']: entry for pack in packs for entry in pack['supporting'] if entry.get('scope_binding')}
    budget = RequestBudget(args.context_tokens)
    extract_estimates, verify_estimates, pack_measurements = [], [], []
    extract_payload_bytes = []
    for pack in packs:
        payload = extract_payload(pack)
        measured = budget.measure('extract', payload)
        if payload['body']:
            extract_estimates.append(token_interval('extract', payload))
            extract_payload_bytes.append(len(encoded(payload).encode('utf-8')))
        verify_estimates.append(token_interval('verify', {
            'pack_id': pack['pack_id'], 'body': pack['body'], 'supporting': pack['supporting'],
            'pending_scope_context': pack.get('pending_scope_context', []),
            'scope_bindings': pack.get('scope_bindings', []),
            'requirements': [], 'dispositions': [],
        }))
        pack_measurements.append({
            'ordinal': len(pack_measurements) + 1, 'pack_id': pack['pack_id'],
            'body_ranges': len(pack['body']), 'sent_body_ranges': len(payload['body']),
            'support_ranges': len(pack['supporting']),
            'body_support_duplicate_ids': len({r['range_id'] for r in payload['body']} &
                                             {r['range_id'] for r in pack['supporting']}),
            'boundary_reason': pack['planning_details']['boundary_reason'],
            'section_count': len(pack['planning_details']['section_ids']),
            'recorded_probe_matches_actual_request': measured == pack['planning_details']['budget'],
            'extract_request': measured,
        })
    image_count = index['stats']['image_count']
    unique_images = len({ref['sha256'] for ref in raw['images'].values()})
    image_classes = Counter(
        'supplemented_pdf_image_object' if ':native-image:' in unit['unit_id'] else
        'bounded_native_image_or_vector' if unit['locator'].get('left') is not None else 'unbounded_image_carrier'
        for unit in raw['units'] if unit['kind'] == 'image_region'
    )
    reviewed_images = {unit['unit_id'] for unit in raw['units']
                       if raw.get('ocr', {}).get(unit['unit_id'], {}).get('disposition', {}).get('classification') == 'decorative'}
    remaining_images = {ref['sha256'] for unit in raw['units'] if unit['kind'] == 'image_region' and unit['unit_id'] not in reviewed_images
                        for ref in [raw['images'][unit['locator']['image_key']]]}
    minimum_requests = len(remaining_images) + len(extract_estimates) + len(packs)
    maximum_requests = minimum_requests + len(packs)
    known_low = sum(value['low'] for value in extract_estimates + verify_estimates)
    known_high = sum(value['high'] for value in extract_estimates + verify_estimates)
    scope_issue_ids = {problem.get('range_id') for problem in index['issues'] if problem['code'] == 'condition_scope_unresolved'}
    range_pages = {entry['range_id']: (entry.get('physical_locator') or entry.get('locator') or {}).get('page_ordinal') for entry in body}
    scope_issue_pages = sorted({range_pages.get(problem.get('parent_range_id')) for problem in index['issues']
                                if problem['code'] == 'condition_scope_unresolved'} - {None})
    report = {
        'schema_version': 2, 'evidence_kind': 'offline_real_pdf_structure_preflight',
        'source': {'sha256': source_digest, 'bytes': source.stat().st_size},
        'environment': {'python': platform.python_version(), 'configured_text_context_tokens': args.context_tokens},
        'execution': {'model_calls': 0, 'network_calls': 0, 'ocr_executed': False,
                      'reused_frozen_refs_for_reporting': args.reuse_frozen,
                      'semantic_acceptance': False, 'registration_seconds': registration_seconds,
                      'total_preparation_seconds': preparation_seconds,
                      'native_parse_seconds': index['documents'][0]['parse_seconds'],
                      'freeze_seconds': index['stats']['duration_seconds']},
        'counts': {'pages': len(raw['source_contract']['page_manifest']), 'page_classifications': dict(classifications),
                   'native_units': len(raw['units']), 'tables': sum(bool(unit.get('grid')) for unit in raw['units']),
                   'native_grid_rows': sum(unit['grid']['row_count'] for unit in raw['units'] if unit.get('grid')),
                   'native_grid_cells': sum(len(unit['grid']['cells']) for unit in raw['units'] if unit.get('grid')),
                   'image_occurrences': image_count, 'image_refs': len(raw['images']), 'unique_image_payloads': unique_images,
                   'reviewed_decorative_occurrences': len(reviewed_images), 'ocr_needed_unique_images': len(remaining_images),
                   'image_occurrences_by_origin': dict(image_classes),
                   'native_embedded_image_count': raw['metadata'].get('embedded_image_count'),
                   'native_vector_figure_count': raw['metadata'].get('vector_figure_count'),
                   'body_ranges': index['range_count'], 'native_ranges': index['native_range_count'], 'packs': len(packs),
                   'compacted_blank_ranges': sum(len(pack['compacted_blank_ranges']) for pack in packs),
                   'supporting_occurrences': sum(len(pack['supporting']) for pack in packs),
                   'scope_binding_occurrences': sum(len(pack.get('scope_bindings', [])) for pack in packs),
                   'pending_scope_occurrences': sum(len(pack.get('pending_scope_context', [])) for pack in packs),
                   'unique_pending_scope_references': len({entry['range_id'] for pack in packs for entry in pack.get('pending_scope_context', [])}),
                   'unique_supporting_conditions': len(scope_candidates)},
        'issues': {'by_code': dict(issues), 'blocking_occurrences': sum(bool(problem.get('blocking', True)) for problem in index['issues']),
                   'unresolved_scope_count': len(scope_issue_ids), 'unresolved_scope_pages_zero_based': scope_issue_pages},
        'coverage': validate_coverage(raw, packs, index),
        'budget': budget.spec(), 'packs': pack_measurements,
        'storage': {
            'original_bytes': source.stat().st_size, 'canonical_raw_bytes': index['documents'][0]['raw_ref']['size'],
            'source_index_bytes': index['documents'][0]['source_index_ref']['size'],
            'pack_plan_bytes': sum(item['artifact_ref']['size'] for item in index['packs']),
            'unique_image_bytes': sum(ref['size'] for ref in {ref['sha256']: ref for ref in raw['images'].values()}.values()),
            'frozen_index_bytes': frozen_ref['size'], 'checkpoint_bytes': None, 'model_audit_bytes': 0,
            'checkpoint_note': 'Offline preparation does not execute the graph; real PostgreSQL growth is measured separately.',
        },
        'largest_pack': {'body_characters': max((sum(len(entry['text']) for entry in pack['body']) for pack in packs), default=0),
                         'supporting_characters': max((sum(len(entry['text']) for entry in pack['supporting']) for pack in packs), default=0),
                         'extract_payload_bytes': max(extract_payload_bytes, default=0),
                         'extract_input_token_interval': {'low': max((value['low'] for value in extract_estimates), default=0),
                                                          'high': max((value['high'] for value in extract_estimates), default=0)},
                         'source_only_context_overflow_pack_count': sum(not value['extract_request']['fits'] for value in pack_measurements)},
        'prospective_model_work': {
            'assumptions': ['No retries, semantic repair, scope review or repacking calls.', 'Plan calls occur only after issues are resolved and confirmation succeeds.',
                            'Literal OCR shares one result per image content SHA and fixed OCR specification; every occurrence remains independently located.',
                            'Reviewed decorative image dispositions are source/hash/occurrence bound; unknown images still require review/OCR. No OCR text has been invented.',
                            f'Normal text model context is {args.context_tokens} tokens; independent VLM capacity is not inferred.'],
            'requests': {'ocr': len(remaining_images), 'extract': len(extract_estimates), 'verify': len(packs),
                         'plan_min': 0, 'plan_max': len(packs), 'total_min': minimum_requests, 'total_max': maximum_requests},
            'known_extract_and_verify_source_input_token_interval': {'low': known_low, 'high': known_high},
            'total_text_input_capacity_envelope': {'low': known_low, 'high': args.context_tokens * (len(extract_estimates) + 2 * len(packs))},
            'unknown_input': ['Generated requirements and dispositions in verify requests.', 'Confirmed requirements in plan requests.',
                              'Image tokens and OCR response text in later extraction/verification requests.'],
            'output_tokens': 'unknown',
            'method': 'Planning low=ceil(source+prompt characters/6); high=complete request UTF8 byte estimate plus framing (not billed tokens). '
                      'The total upper value is an input-capacity envelope, not expected usage. No tokenizer or paid usage was measured.',
        },
    }
    report['execution']['source_xattr_count_preserved_on_private_artifacts'] = propagate_private_attributes(source, private_root)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'report': str(args.report), 'counts': report['counts'], 'issues': report['issues'],
                      'coverage': report['coverage'], 'largest_pack': report['largest_pack']}, ensure_ascii=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--private-root', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--expected-sha256')
    parser.add_argument('--context-tokens', type=int, default=131072)
    parser.add_argument('--reuse-frozen', action='store_true')
    parser.add_argument('--image-dispositions', type=Path)
    asyncio.run(prepare(parser.parse_args()))


if __name__ == '__main__':
    main()
