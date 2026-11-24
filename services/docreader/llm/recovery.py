"""Bounded work-unit recovery with immutable rejection receipts and send fences."""
import asyncio
from copy import deepcopy
import hashlib
import json
import time

from docreader.integrations.store import Cancelled, StaleExecution
from docreader.llm.request import (
    MODEL_CONTRACT_ATTEMPTS, MODEL_TRANSPORT_RETRIES, ModelContractError, ModelCorrectionStalled,
    ModelInfrastructureError, ModelJSONError, ModelRateLimitError, ModelSemanticError, ModelWorkUnitExhausted,
    image_data_url, model_retry_delay,
)


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


def _json_feedback(error):
    diagnostic = error.feedback
    return {'errors': [{'path': [], 'code': diagnostic['code'], 'detail': diagnostic['message'],
                        **{key: diagnostic[key] for key in ('line', 'column', 'offset')}}],
            'fragments': [{'path': [], 'value': diagnostic['fragment']}]}


def _rejection_feedback(feedback, count, response_kind):
    feedback = {**deepcopy(feedback), 'rejection_count': count}
    if response_kind == 'invalid_json':
        fragment = feedback['fragments'][0]
        while len(json.dumps(feedback, ensure_ascii=False, separators=(',', ':'))) > 512 and fragment['value']:
            fragment['value'] = fragment['value'][:-1]
    # Match CAS ordering before the first send so restarts reuse the exact
    # embedded JSON string and therefore the same paid response.
    return json.loads(json.dumps(feedback, sort_keys=True, ensure_ascii=False))


async def call_validated(model, store, artifacts, lease, task, payload, validate, *, images=(), owner_metadata=None,
                         passthrough_exceptions=()):
    inputs = {'task': task, 'payload': payload}
    if images:
        inputs['images'] = [hashlib.sha256(image_data_url(image).encode()).hexdigest() for image in images]
    identity = _digest(inputs)
    current = deepcopy(payload)
    sequence = 0
    restored = None

    async def wait_rejection(deadline, scope):
        if remaining := max(0, deadline - time.time()):
            await asyncio.sleep(remaining)
            # Receipt reads retain the owner's cancellation/epoch fence without
            # keeping its transaction open while the worker waits.
            await store.receipt(lease, 'model-rejection', scope, input_digest=identity)

    # Recover feedback after a lost worker, without resending accepted requests.
    while True:
        scope = f'{identity}:{sequence}'
        row = await store.receipt(lease, 'model-rejection', scope, input_digest=identity)
        if row is None:
            break
        rejection = await asyncio.to_thread(artifacts.read_json, row['artifact_ref'])
        feedback = rejection.get('feedback')
        if feedback is None:
            response = await asyncio.to_thread(artifacts.read_json, rejection['response_ref'])
            feedback = {'errors': rejection['errors'], 'fragments': _fragments(response, rejection['errors'])}
        feedback = _rejection_feedback(feedback, sequence + 1, rejection.get('response_kind'))
        current = {**deepcopy(payload), 'contract_feedback': feedback}
        sequence += 1
        restored = row, rejection, scope

    if sequence >= MODEL_CONTRACT_ATTEMPTS:
        raise ModelWorkUnitExhausted(task)
    if restored:
        row, rejection, scope = restored
        deadline = rejection.get('not_before')
        if deadline is None:
            if committed_at := row.get('committed_at'):
                deadline = committed_at.timestamp() + model_retry_delay(sequence - 1)
            else:
                # Old authoring receipts have no timestamp. Record that gap and
                # anchor one conservative wait now, without extending it again.
                saved = await store.receipt(lease, 'model-rejection-wait', scope, input_digest=identity)
                if saved is None:
                    wait = {'not_before': time.time() + model_retry_delay(sequence - 1),
                            'timing_origin': 'legacy_unavailable'}
                    reference = await asyncio.to_thread(artifacts.put_json, wait)
                    await store.commit_artifact(lease, 'model-rejection-wait', reference,
                                                scope=scope, input_digest=identity)
                else:
                    wait = await asyncio.to_thread(artifacts.read_json, saved['artifact_ref'])
                deadline = wait['not_before']
        await wait_rejection(deadline, scope)

    # Corrections are counted by durable receipts; transport and other call
    # failures resend the same request under their own allowance. The stores
    # enforce both limits again at send time, so restarts cannot reset them.
    transport = 0

    async def resend(error, *, rate_limited=False, retry_after_seconds=None):
        nonlocal transport
        transport += 1
        if transport > MODEL_TRANSPORT_RETRIES:
            raise ModelWorkUnitExhausted(task) from error
        await asyncio.sleep(model_retry_delay(transport - 1, rate_limited=rate_limited,
                                              retry_after_seconds=retry_after_seconds))

    while True:
        response_kind = 'parsed'
        try:
            response = await model.call(task, current, images=images) if images else await model.call(task, current)
        except (Cancelled, StaleExecution, ModelWorkUnitExhausted):
            raise
        except ModelInfrastructureError as error:
            # This model transport error subclasses OSError; a caller passing
            # storage OSError through must still retain the paid-call retries.
            await resend(error, rate_limited=isinstance(error, ModelRateLimitError),
                         retry_after_seconds=getattr(error, "retry_after_seconds", None))
            continue
        except passthrough_exceptions:
            raise
        except ModelJSONError as error:
            feedback = _json_feedback(error)
            response, response_kind = {'json_feedback': feedback}, 'invalid_json'
        except ModelCorrectionStalled:
            feedback = {'errors': [{'path': [], 'code': 'cached_response_invalid',
                                   'detail': 'The cached model response could not be validated.'}], 'fragments': []}
            response, response_kind = {'cache_error': feedback}, 'invalid_cache'
        except Exception as error:
            # Configuration, uncertain payment and other call errors use the
            # transport allowance; private exception text is not model input.
            await resend(error)
            continue
        else:
            try:
                return await asyncio.to_thread(validate, response)
            except (Cancelled, StaleExecution, ModelWorkUnitExhausted):
                raise
            except passthrough_exceptions:
                raise
            except ModelSemanticError as error:
                errors = (error.errors if isinstance(error, ModelContractError) else
                          [{'path': [], 'code': 'response_contract', 'detail': str(error)}])
                fragments = error.feedback_fragments if isinstance(error, ModelContractError) else None
                feedback = {'errors': errors, 'fragments': (deepcopy(fragments) if fragments is not None
                                                           else _fragments(response, errors))}
            except Exception as error:
                await resend(error)
                continue

        # Receipt/DB errors escape to the command's infrastructure policy.
        feedback = _rejection_feedback(feedback, sequence + 1, response_kind)
        errors = feedback['errors']
        request_ref = await asyncio.to_thread(artifacts.put_json, current)
        response_ref = await asyncio.to_thread(artifacts.put_json, response)
        owner = owner_metadata if owner_metadata is not None else {'command_id': lease.command_id, 'epoch': lease.epoch}
        rejection = {'task': task, 'input_digest': identity, 'request_ref': request_ref,
                     'response_ref': response_ref, 'errors': errors, **owner,
                     'response_kind': response_kind, 'feedback': feedback,
                     'not_before': time.time() + model_retry_delay(sequence)}
        ref = await asyncio.to_thread(artifacts.put_json, rejection)
        scope = f'{identity}:{sequence}'
        await store.commit_artifact(lease, 'model-rejection', ref, scope=scope, input_digest=identity)
        sequence += 1
        current = {**deepcopy(payload), 'contract_feedback': feedback}
        if sequence >= MODEL_CONTRACT_ATTEMPTS:
            raise ModelWorkUnitExhausted(task)
        await wait_rejection(rejection['not_before'], scope)
