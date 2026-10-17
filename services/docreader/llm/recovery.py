"""One validated call per execution, with immutable rejection receipts."""
import asyncio
from copy import deepcopy
import hashlib
import json

from docreader.llm.request import ModelContractError, ModelCorrectionStalled, ModelSemanticError


def _digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                    separators=(',', ':')).encode()).hexdigest()


def _fragments(response, errors):
    fragments = []
    for error in errors:
        value = response
        path = error['path']
        try:
            for key in path:
                value = value[key]
        except (KeyError, IndexError, TypeError):
            fragments.append({'path': path, 'missing': True})
        else:
            fragment = {'path': path, 'value': value}
            if fragment not in fragments:
                fragments.append(fragment)
    return fragments


async def call_validated(model, store, artifacts, lease, task, payload, validate):
    identity = _digest({'task': task, 'payload': payload})
    current = deepcopy(payload)
    seen, sequence = set(), 0
    # Read persisted history, not a provider retry loop. The only send is below.
    while True:
        digest = _digest(current)
        if digest in seen:
            raise ModelCorrectionStalled('Identical correction input was already rejected')
        seen.add(digest)
        scope = f'{identity}:{sequence}'
        row = await store.receipt(lease, 'model-rejection', scope, input_digest=identity)
        if row is None:
            break
        rejection = await asyncio.to_thread(artifacts.read_json, row['artifact_ref'])
        if rejection['command_id'] == lease.command_id and rejection['epoch'] == lease.epoch:
            raise ModelContractError(rejection['errors'])
        response = await asyncio.to_thread(artifacts.read_json, rejection['response_ref'])
        current = {**deepcopy(payload), 'contract_feedback': {
            'errors': rejection['errors'], 'fragments': _fragments(response, rejection['errors']),
        }}
        sequence += 1
    response = await model.call(task, current)
    try:
        return await asyncio.to_thread(validate, response)
    except ModelSemanticError as error:
        errors = (error.errors if isinstance(error, ModelContractError) else
                  [{'path': [], 'code': 'response_contract', 'detail': str(error)}])
        request_ref = await asyncio.to_thread(artifacts.put_json, current)
        response_ref = await asyncio.to_thread(artifacts.put_json, response)
        rejection = {'task': task, 'input_digest': identity, 'request_ref': request_ref,
                     'response_ref': response_ref, 'errors': errors,
                     'command_id': lease.command_id, 'epoch': lease.epoch}
        ref = await asyncio.to_thread(artifacts.put_json, rejection)
        await store.commit_artifact(lease, 'model-rejection', ref, scope=scope, input_digest=identity)
        raise ModelContractError(errors) from None
