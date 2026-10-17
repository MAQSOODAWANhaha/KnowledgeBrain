"""Request-local references. Never rewrite prose or infer an unknown identity."""
from copy import deepcopy
import re
import hashlib
import json


_FIELDS = {
    'range_id': 'r', 'range_ids': 'r', 'target_range_ids': 'r', 'applies_to_range_ids': 'r',
    'reviewed_target_range_ids': 'r', 'decision_range_id': 'r',
    'item_id': 'i', 'item_ids': 'i', 'from_item_id': 'i', 'to_item_id': 'i',
    'target_item_id': 'i', 'replacement_item_ids': 'i',
    'relation_id': 'l', 'relation_ids': 'l', 'decision_id': 'd', 'accepted_decision_ids': 'd',
    'candidate_id': 'c', 'issue_id': 'q', 'issue_ids': 'q', 'related_issue_ids': 'q',
    'document_id': 'doc', 'section_id': 's', 'parent_section_id': 's',
    'work_unit_id': 'w', 'pack_id': 'p', 'table_id': 't', 'table_ids': 't',
    'unit_id': 'u', 'image_unit_ids': 'u',
}


class WirePayload:
    VERSION = 'tender_short_refs_v2'

    def __init__(self, task, payload):
        self.enabled = task in {'extract', 'verify'} and payload.get('schema_version') == 2
        self.mapping = {}
        self.original_work_unit = payload.get('work_unit_id')
        self.payload = deepcopy(payload)
        if not self.enabled:
            return
        self.payload['request_ref'] = hashlib.sha256(json.dumps(payload, ensure_ascii=False,
            sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        self.payload.pop('run_id', None)
        for view in self.payload.get('source_views', []):
            for field in ('body', 'supporting', 'pending_scope_context', 'scope_bindings'):
                for entry in view.get(field, []):
                    for key in ('source_range_id', 'parent_range_id', 'start', 'end'):
                        entry.pop(key, None)
        self._collect({key: value for key, value in self.payload.items() if key != 'contract_feedback'})
        self.payload = self._walk(self.payload, self._encode)

    def _collect(self, value):
        if isinstance(value, dict):
            for key, entry in value.items():
                prefix = _FIELDS.get(key)
                if prefix:
                    values = entry if isinstance(entry, list) else [entry]
                    mapping = self.mapping.setdefault(prefix, {})
                    for identity in values:
                        if isinstance(identity, str) and identity not in mapping:
                            mapping[identity] = f'{prefix}{len(mapping) + 1}'
                else:
                    self._collect(entry)
        elif isinstance(value, list):
            for entry in value:
                self._collect(entry)

    def _walk(self, value, convert):
        if isinstance(value, list):
            return [self._walk(entry, convert) for entry in value]
        if not isinstance(value, dict):
            return value
        result = {}
        for key, entry in value.items():
            if key in _FIELDS:
                prefix = _FIELDS[key]
                result[key] = ([convert(prefix, identity) for identity in entry] if isinstance(entry, list)
                               else convert(prefix, entry))
            else:
                result[key] = self._walk(entry, convert)
        return result

    def _encode(self, prefix, identity):
        return self.mapping.get(prefix, {}).get(identity, identity) if isinstance(identity, str) else identity

    def decode(self, response):
        if not self.enabled:
            return response
        reverse = {prefix: {short: original for original, short in values.items()}
                   for prefix, values in self.mapping.items()}
        work_unit = self.mapping.get('w', {}).get(self.original_work_unit)

        def restore(prefix, identity):
            if not isinstance(identity, str):
                return identity
            if identity in reverse.get(prefix, {}):
                return reverse[prefix][identity]
            if prefix in {'i', 'l', 'c', 'd'} and work_unit and re.fullmatch(
                    re.escape(work_unit) + ':' + prefix + r'[1-9][0-9]*', identity):
                return self.original_work_unit + identity[len(work_unit):]
            # Do not let a copied permanent ID bypass the request-local contract.
            return '!unbound:' + identity

        return self._walk(response, restore)
