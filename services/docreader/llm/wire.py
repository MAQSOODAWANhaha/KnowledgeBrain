"""Request-local references. Never rewrite prose or infer an unknown identity."""
from copy import deepcopy
import re
import hashlib
import json


_FIELDS = {
    'rule_id': 'e', 'rule_ids': 'e', 'constraint_rule_ids': 'e', 'from_rule_ids': 'e', 'to_rule_ids': 'e',
    'range_id': 'r', 'range_ids': 'r', 'target_range_ids': 'r', 'applies_to_range_ids': 'r',
    'reviewed_target_range_ids': 'r', 'decision_range_id': 'r',
    'item_id': 'i', 'item_ids': 'i', 'from_item_id': 'i', 'to_item_id': 'i',
    'target_item_id': 'i', 'replacement_item_ids': 'i',
    'relation_id': 'l', 'relation_ids': 'l', 'unverified_relation_ids': 'l',
    'decision_id': 'd', 'decision_ids': 'd', 'accepted_decision_ids': 'd',
    'candidate_id': 'c', 'candidate_ids': 'c', 'issue_id': 'q', 'issue_ids': 'q', 'related_issue_ids': 'q',
    'document_id': 'doc', 'section_id': 's', 'parent_section_id': 's',
    'work_unit_id': 'w', 'work_unit_ids': 'w', 'target_work_unit_ids': 'w',
    'pack_id': 'p', 'table_id': 't', 'table_ids': 't',
    'unit_id': 'u', 'image_unit_ids': 'u',
    'part_id': 'p', 'template_part_ids': 'p', 'start_part_id': 'p', 'end_part_id': 'p',
    'constraint_range_ids': 'r', 'context_range_ids': 'r',
}


class WirePayload:
    VERSION = 'tender_short_refs_v2'

    def __init__(self, task, payload):
        self.enabled = task in {'extract', 'verify', 'outline'} and payload.get('schema_version') == 2
        self.mapping = {}
        self.original_work_unit = payload.get('work_unit_id')
        self.original_work_units = {unit['work_unit_id'] for unit in payload.get('work_units', [])}
        if self.original_work_unit:
            self.original_work_units.add(self.original_work_unit)
        self.payload = deepcopy(payload)
        if not self.enabled:
            return
        canonical = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
        batched = task == 'verify' and 'work_units' in payload
        if batched:
            # CAS sorts object keys before recovery; preparation must produce the same wire bytes.
            self.payload = json.loads(canonical)
        self.payload['request_ref'] = hashlib.sha256(canonical.encode()).hexdigest()
        self.payload.pop('run_id', None)
        for view in self.payload.get('source_views', []):
            for field in ('body', 'supporting', 'pending_scope_context', 'scope_bindings'):
                for entry in view.get(field, []):
                    for key in ('source_range_id', 'parent_range_id', 'start', 'end'):
                        entry.pop(key, None)
        if batched:
            # Joint review adds proposals/findings; they must not renumber the frozen basis.
            # Corrections keep all members and sources, even when only one remains a target.
            for field in ('source_views', 'work_units', 'items', 'candidates', 'issues',
                          'dispositions', 'scope_decisions', 'relations'):
                self._collect(self.payload.get(field, []))
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
            if key in _FIELDS or key == 'for_ids':
                prefix = _FIELDS.get(key)
                result[key] = ([convert(prefix, identity) for identity in entry] if isinstance(entry, list)
                               else convert(prefix, entry))
            else:
                result[key] = self._walk(entry, convert)
        return result

    def _encode(self, prefix, identity):
        if prefix is None and isinstance(identity, str):
            selected = [self.mapping[p][identity] for p in ('e', 'r') if identity in self.mapping.get(p, {})]
            return selected[0] if len(selected) == 1 else '!unbound:' + identity
        return self.mapping.get(prefix, {}).get(identity, identity) if isinstance(identity, str) else identity

    def decode(self, response):
        if not self.enabled:
            return response
        reverse = {prefix: {short: original for original, short in values.items()}
                   for prefix, values in self.mapping.items()}
        namespaces = {short: original for original, short in self.mapping.get('w', {}).items()
                      if original in self.original_work_units}

        def restore(prefix, identity):
            if not isinstance(identity, str):
                return identity
            if prefix is None:
                selected = [reverse[p][identity] for p in ('e', 'r') if identity in reverse.get(p, {})]
                return selected[0] if len(selected) == 1 else '!unbound:' + identity
            if identity in reverse.get(prefix, {}):
                return reverse[prefix][identity]
            namespace, separator, suffix = identity.partition(':')
            if (prefix in {'i', 'l', 'c', 'd'} and separator and namespace in namespaces
                    and re.fullmatch(prefix + r'[1-9][0-9]*', suffix)):
                return namespaces[namespace] + ':' + suffix
            # Do not let a copied permanent ID bypass the request-local contract.
            return '!unbound:' + identity

        return self._walk(response, restore)
