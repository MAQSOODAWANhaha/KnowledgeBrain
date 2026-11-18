"""Bounded work-unit recovery with immutable rejection receipts and send fences."""
import asyncio
from copy import deepcopy
import hashlib
import json
import time
from uuid import uuid4

from docreader.integrations.store import Cancelled, RequestReplayBlocked, StaleExecution
from docreader.llm.request import (
    ModelContractError, ModelCorrectionStalled, ModelInfrastructureError, ModelJSONError,
    ModelRateLimitError, ModelSemanticError, ModelWorkUnitExhausted, image_data_url, model_retry_delay,
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

    if sequence >= 4:
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

    for attempt in range(sequence, 4):
        response_kind = 'parsed'
        try:
            response = await model.call(task, current, images=images) if images else await model.call(task, current)
        except (Cancelled, StaleExecution, ModelWorkUnitExhausted):
            raise
        except ModelInfrastructureError as error:
            # This model transport error subclasses OSError; a caller passing
            # storage OSError through must still retain the paid-call retries.
            if attempt == 3:
                raise ModelWorkUnitExhausted(task) from error
            delay = model_retry_delay(attempt, rate_limited=isinstance(error, ModelRateLimitError),
                                      retry_after_seconds=getattr(error, "retry_after_seconds", None))
            await asyncio.sleep(delay)
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
            # Transport, configuration, uncertain payment and other call errors
            # share the same allowance; private exception text is not model input.
            if attempt == 3:
                raise ModelWorkUnitExhausted(task) from error
            await asyncio.sleep(2 ** attempt)
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
                if attempt == 3:
                    raise ModelWorkUnitExhausted(task) from error
                await asyncio.sleep(2 ** attempt)
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
                     'not_before': time.time() + model_retry_delay(attempt)}
        ref = await asyncio.to_thread(artifacts.put_json, rejection)
        scope = f'{identity}:{sequence}'
        await store.commit_artifact(lease, 'model-rejection', ref, scope=scope, input_digest=identity)
        sequence += 1
        current = {**deepcopy(payload), 'contract_feedback': feedback}
        if attempt == 3:
            raise ModelWorkUnitExhausted(task)
        await wait_rejection(rejection['not_before'], scope)


async def call_groups_validated(model, store, artifacts, lease, task, payload, validate, *, validate_shared):
    """Accept independent targets while retaining their complete, frozen shared basis.

    The existing artifact journal reserves attempts before request construction,
    including unsent configuration/admission failures. Raw physical successes are
    replayed before any new reservation, and a rejected member gets real feedback.
    """
    basis = {key: deepcopy(value) for key, value in payload.items()
             if key not in {"target_work_unit_ids", "contract_feedback"}}
    targets = list(payload["target_work_unit_ids"])
    members = [unit["work_unit_id"] for unit in payload["work_units"]]
    if not targets or len(set(targets)) != len(targets) or not set(targets) <= set(members):
        raise ValueError("batch targets must be distinct original members")
    model_type = f"{type(model).__module__}.{type(model).__qualname__}"
    spec_error = None
    try:
        spec = model.execution_spec() if hasattr(model, "execution_spec") else {"injected_model": model_type}
    except (Cancelled, StaleExecution, ModelWorkUnitExhausted):
        raise
    except Exception as error:
        # Invalid local settings cannot yield a reusable result, but must still
        # consume the durable allowance rather than restart an unbounded loop.
        spec_error = error
        spec = {"unavailable_execution_spec": type(error).__name__, "model_type": model_type}
    basis_digest = _digest({"task": task, "basis": basis, "execution_spec": spec})
    keys = {unit: _digest({"task": task, "operation": payload["operation"], "work_unit_id": unit})
            for unit in members}
    result_scopes = {unit: _digest({"basis": basis_digest, "work_unit_id": unit}) for unit in members}
    metadata = {"group_basis_digest": basis_digest}

    async def read(row):
        return await asyncio.to_thread(artifacts.read_json, row["artifact_ref"])

    async def commit(stage, scope, value, **extra):
        ref = await asyncio.to_thread(artifacts.put_json, value)
        return await store.commit_artifact(lease, stage, ref, scope=scope, input_digest=basis_digest,
                                           metadata={**metadata, **extra})

    def feedback(error, response):
        errors = (error.errors if isinstance(error, ModelContractError) else
                  [{"path": [], "code": "response_contract", "detail":
                    str(error) if isinstance(error, ModelSemanticError) else "Host validation could not accept this result"}])
        return {"errors": errors, "fragments": _fragments(response, errors)}

    async def retry_state(rows, pending):
        corrections, counts, not_before = {}, dict.fromkeys(pending, 0), 0
        for row in rows:
            if row["stage"] != "model-group-outcome":
                continue
            outcome = await read(row)
            for unit, error in outcome["rejected"].items():
                if unit not in counts:
                    continue
                counts[unit] += 1
                deadline = outcome.get("not_before")
                if deadline is None:
                    committed_at = row.get("committed_at")
                    deadline = (committed_at.timestamp() if committed_at else time.time()) + model_retry_delay(
                        counts[unit] - 1, rate_limited=outcome.get("failure_class") == "http_429",
                        retry_after_seconds=outcome.get("retry_after_seconds"))
                not_before = max(not_before, deadline)
                if error is not None:
                    corrections[unit] = error
        return corrections, counts, not_before

    async def process(attempt, rows):
        attempt_id = attempt["scope"]
        manifest = await read(attempt)
        current = manifest["payload"]
        active = current["target_work_unit_ids"]
        if "not_before" not in manifest:
            _, counts, not_before = await retry_state(rows, active)
            manifest = {**manifest, "not_before": not_before, "retry_attempt": max(counts.values(), default=0)}
        index = {(row["stage"], row["scope"]): row for row in rows}
        response_row = index.get(("model-group-response", attempt_id))
        if response_row is None:
            if remaining := max(0, manifest.get("not_before", 0) - time.time()):
                await asyncio.sleep(remaining)
                await store.assert_fence(lease)
            try:
                if spec_error is not None:
                    raise spec_error
                kwargs = ({"group_attempt": {"group_attempt_id": attempt_id,
                           "group_work_unit_keys": manifest["group_work_unit_keys"]}}
                          if getattr(model, "supports_group_attempts", False) else {})
                response = await model.call(task, current, **kwargs)
            except (Cancelled, StaleExecution, ModelWorkUnitExhausted, RequestReplayBlocked):
                raise
            except ModelJSONError as error:
                received = {"kind": "invalid_json", "feedback": _json_feedback(error)}
            except ModelCorrectionStalled:
                received = {"kind": "invalid_cache", "feedback": {"errors": [{"path": [],
                    "code": "cached_response_invalid", "detail": "The saved response could not be verified"}], "fragments": []}}
            except Exception as error:
                received = {"kind": "request_failure", "error_type": type(error).__name__}
                if isinstance(error, ModelRateLimitError):
                    received.update(failure_class="http_429", retry_after_seconds=error.retry_after_seconds)
            else:
                received = {"kind": "parsed", "response": response}
            if received["kind"] != "parsed" and getattr(model, "supports_group_attempts", False):
                # The physical audit is authoritative even if this response
                # journal was lost between the HTTP failure and its commit.
                received["not_before"] = await store.request_not_before(
                    lease, task, None, group_work_unit_keys=manifest["group_work_unit_keys"], group_attempt_id=attempt_id)
            received["completed_at"] = time.time()
            response_row = await commit("model-group-response", attempt_id, received)
        received = await read(response_row)
        rejected = {}
        if received["kind"] != "parsed":
            rejected = {unit: received.get("feedback") for unit in active}
        else:
            response = received["response"]
            shared_row = index.get(("model-group-shared", attempt_id))
            try:
                shared = (await read(shared_row) if shared_row else
                          await asyncio.to_thread(validate_shared, response, current))
            except (Cancelled, StaleExecution, ModelWorkUnitExhausted):
                raise
            except Exception as error:
                rejected = {unit: feedback(error, response) for unit in active}
            else:
                if shared_row is None:
                    await commit("model-group-shared", attempt_id, shared)
                answers = {unit["work_unit_id"]: unit for unit in response["work_units"]
                           if isinstance(unit, dict) and isinstance(unit.get("work_unit_id"), str)}
                for unit in active:
                    if ("model-group-result", result_scopes[unit]) in index:
                        continue
                    rejection_scope = f"{attempt_id}:{result_scopes[unit]}"
                    if rejection_row := index.get(("model-group-rejection", rejection_scope)):
                        rejected[unit] = await read(rejection_row)
                        continue
                    answer = answers.get(unit)
                    try:
                        if answer is None:
                            raise ModelContractError([{"path": ["work_units"], "code": "missing_work_unit",
                                                       "detail": "Return a result for this target work unit"}])
                        value = await asyncio.to_thread(validate, unit, answer)
                    except (Cancelled, StaleExecution, ModelWorkUnitExhausted):
                        raise
                    except Exception as error:
                        rejected[unit] = feedback(error, answer if answer is not None else {})
                        await commit("model-group-rejection", rejection_scope, rejected[unit], work_unit_id=unit)
                    else:
                        await commit("model-group-result", result_scopes[unit],
                                     {**value, "work_unit_id": unit}, work_unit_id=unit,
                                     group_attempt_id=attempt_id)
        delay = model_retry_delay(manifest.get("retry_attempt", 0),
            rate_limited=received.get("failure_class") == "http_429",
            retry_after_seconds=received.get("retry_after_seconds"))
        completed_at = received.get("completed_at")
        if completed_at is None:
            # Legacy journals predate deadlines; their commit is the first
            # durable failure timestamp available, never an assumed wait.
            committed_at = response_row.get("committed_at")
            completed_at = committed_at.timestamp() if committed_at else time.time()
        await commit("model-group-outcome", attempt_id, {"rejected": rejected,
                     "not_before": received.get("not_before") or completed_at + delay,
                     "failure_class": received.get("failure_class"),
                     "retry_after_seconds": received.get("retry_after_seconds")},
                     rejected_response=bool(rejected) and received["kind"] != "request_failure")

    while True:
        rows = await store.group_receipts(lease, basis_digest)
        finished = {row["scope"] for row in rows if row["stage"] == "model-group-outcome"}
        attempts = [row for row in rows if row["stage"] == "model-group-attempt"]
        unfinished = [row for row in attempts if row["scope"] not in finished and
                      set(row["metadata"]["group_work_unit_keys"]) & {keys[unit] for unit in targets}]
        if unfinished:
            # Even if one member was committed, finish replaying the original
            # physical response before filtering targets or sending corrections.
            await process(unfinished[0], rows)
            continue
        completed = {row["metadata"]["work_unit_id"]: row for row in rows
                     if row["stage"] == "model-group-result"}
        pending = [unit for unit in targets if unit not in completed]
        if not pending:
            shared, seen = [], set()
            for row in rows:
                if row["stage"] == "model-group-shared":
                    for finding in await read(row):
                        digest = _digest(finding)
                        if digest not in seen:
                            shared.append(finding)
                            seen.add(digest)
            return {"work_units": [await read(completed[unit]) for unit in targets], "shared_findings": shared}
        corrections, rejected_counts, not_before = await retry_state(rows, pending)
        current = {**deepcopy(basis), "target_work_unit_ids": pending}
        if corrections:
            current["contract_feedback"] = {"work_units": [{"work_unit_id": unit,
                **_rejection_feedback(corrections[unit], rejected_counts[unit], "parsed")}
                for unit in pending if unit in corrections]}
        attempt_id = uuid4().hex
        manifest = {"task": task, "payload": current, "group_work_unit_keys": [keys[unit] for unit in pending],
                    "not_before": not_before, "retry_attempt": max(rejected_counts.values(), default=0)}
        ref = await asyncio.to_thread(artifacts.put_json, manifest)
        await store.reserve_group_attempt(lease, attempt_id, ref, basis_digest, manifest["group_work_unit_keys"])
        # The reservation is durable before local configuration, token admission
        # or transport can fail. Replaying it never reserves a second attempt.
