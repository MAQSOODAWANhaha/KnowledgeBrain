"""Lossless request-local identities for the full-source material reading."""

from copy import deepcopy
import hashlib
from typing import Any, cast
from itertools import groupby
import json
import re

from docreader.llm.request import ModelContractError


MATERIAL_IDENTITY_CONTRACT = 'material_short_ids_v3'
LEGACY_MATERIAL_IDENTITY_CONTRACT = 'material_short_ids_v2'
_CATALOGS = ('chapters', 'table_header_catalog', 'chapter_title_catalog', 'directory_cases',
             'response_form_catalog', 'source_group_candidates', 'source_carrier_catalog',
             'reviewed_source_facts', 'unplaced_tables')
_POINT_FIELDS = ('document_id', 'page', 'line')
_WINDOW_FIELDS = ('document_id', 'page', 'start_line', 'end_line')
_FIELDS = {'document_id': 'd', 'chapter_id': 'c', 'parent_id': 'c', 'chapter_ids': 'c',
           'case_id': 'q', 'item_ids': 'q', 'fact_id': 'f', 'title_id': 'n', 'table_id': 't'}


def _walk(value, identity, path=()):
    if isinstance(value, list):
        return [_walk(entry, identity, (*path, index)) for index, entry in enumerate(value)]
    if not isinstance(value, dict):
        return deepcopy(value)
    result = {}
    for key, entry in value.items():
        namespace = _FIELDS.get(key)
        if key == 'id' and len(path) == 2 and path[0] == 'requirement_cases':
            namespace = 'q'
        if key == 'id' and ((len(path) == 2 and path[0] == 'chapters')
                or (len(path) == 3 and path[0] == 'directory_cases' and path[2] in {'current', 'alternative'})):
            namespace = 'c'
        selected_path = (*path, key)
        if key == 'grid':
            result[key] = deepcopy(entry)
        elif namespace and entry is not None:
            result[key] = ([identity(item, namespace, (*selected_path, index)) for index, item in enumerate(entry)]
                           if isinstance(entry, list) else identity(entry, namespace, selected_path))
        else:
            result[key] = _walk(entry, identity, selected_path)
    return result


def _column_blocks(records):
    blocks, fields = [], []
    for keys, group in groupby(records, key=tuple):
        group = list(group)
        first = {key: json.dumps(group[0][key], ensure_ascii=False, separators=(',', ':')) for key in keys}
        constants = {key: deepcopy(group[0][key]) for key in keys if all(
            json.dumps(record[key], ensure_ascii=False, separators=(',', ':')) == first[key]
            for record in group[1:])}
        columns = [key for key in keys if key not in constants]
        blocks.append({'columns': columns, 'constant_values': constants,
                       'rows': [[deepcopy(record[key]) for key in columns] for record in group]})
        fields.append(keys)
    return {'blocks': blocks}, fields


class MaterialIds:
    def __init__(self, payload, *, contract=LEGACY_MATERIAL_IDENTITY_CONTRACT):
        if contract not in (LEGACY_MATERIAL_IDENTITY_CONTRACT, MATERIAL_IDENTITY_CONTRACT):
            raise ModelContractError([{'path': ['identity_format'],
                'code': 'directory_material_identity_contract_invalid',
                'detail': 'unsupported material identity contract'}])
        found = {namespace: set() for namespace in _FIELDS.values()}

        def collect(value, namespace, _path):
            found[namespace].add(value)
            return value

        _walk(payload, collect)
        self.forward = {namespace: {value: f'{namespace}{index}' for index, value in enumerate(sorted(values), 1)}
                        for namespace, values in found.items()}
        self.reverse = {namespace: {alias: value for value, alias in values.items()}
                        for namespace, values in self.forward.items()}
        self._coordinates = {}
        self._catalog_fields = {}

        def compact(value, path=()):
            if isinstance(value, list):
                return [compact(entry, (*path, index)) for index, entry in enumerate(value)]
            if not isinstance(value, dict):
                return deepcopy(value)
            for fields in (_POINT_FIELDS, _WINDOW_FIELDS):
                if set(value) == set(fields):
                    self._coordinates[path] = fields
                    return [value[field] for field in fields]
            return {key: deepcopy(entry) if key == 'grid' else compact(entry, (*path, key))
                    for key, entry in value.items()}

        self.payload = cast(dict[str, Any], compact(
            _walk(payload, lambda value, namespace, _path: self.forward[namespace][value])))
        self.payload['identity_format'] = contract
        self.payload['source_coordinate_format'] = 'material_source_arrays_v1'
        self.payload['source_coordinate_fields'] = {'point': list(_POINT_FIELDS), 'window': list(_WINDOW_FIELDS)}
        self.payload['source_coordinate_note'] = '数组仅表示输入来源定位；输出仍按Schema返回对象，不返回坐标数组。'
        self.payload['identity_binding'] = hashlib.sha256(json.dumps(
            {'contract': contract, 'payload': payload},
            ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        if contract == MATERIAL_IDENTITY_CONTRACT:
            for key in _CATALOGS:
                records = self.payload.get(key)
                if isinstance(records, list) and len(records) >= 2 and all(isinstance(r, dict) for r in records):
                    self.payload[key], self._catalog_fields[key] = _column_blocks(records)
            self.payload['catalog_record_format'] = 'column_blocks_v1'

    def restore_payload(self):
        value = deepcopy(self.payload)
        if self._catalog_fields:
            for key, fields in self._catalog_fields.items():
                records = []
                for block, keys in zip(value[key]['blocks'], fields, strict=True):
                    for row in block['rows']:
                        record = {**block['constant_values'], **dict(zip(block['columns'], row, strict=True))}
                        records.append({name: deepcopy(record[name]) for name in keys})
                value[key] = records
        if value.get('identity_format') == MATERIAL_IDENTITY_CONTRACT:
            value.pop('catalog_record_format')
        for key in ('identity_format', 'identity_binding', 'source_coordinate_format',
                    'source_coordinate_fields', 'source_coordinate_note'):
            value.pop(key)
        for path, fields in self._coordinates.items():
            parent = value
            for key in path[:-1]:
                parent = parent[key]
            parent[path[-1]] = dict(zip(fields, parent[path[-1]], strict=True))
        return self.decode(value)

    def decode(self, raw):
        def restore(value, namespace, path):
            if not isinstance(value, str) or value not in self.reverse[namespace]:
                raise ModelContractError([{'path': list(path), 'code': 'directory_material_identity_invalid',
                    'detail': f'choose a supplied {namespace} identity from this request'}])
            return self.reverse[namespace][value]

        return _walk(raw, restore)

    def accept(self, validate):
        def accept(raw):
            try:
                return validate(self.decode(raw))
            except ModelContractError as error:
                aliases = {}
                for values in self.forward.values():
                    for value, alias in values.items():
                        aliases.setdefault(value, []).append(alias)
                errors = deepcopy(error.errors)
                if aliases:
                    pattern = re.compile('|'.join(re.escape(value) for value in sorted(aliases, key=len, reverse=True)))
                    for entry in errors:
                        entry['detail'] = pattern.sub(lambda match: '/'.join(aliases[match[0]]), entry['detail'])
                raise ModelContractError(errors) from None

        return accept
