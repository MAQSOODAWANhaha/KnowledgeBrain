\set ON_ERROR_STOP on

-- Phase 3 live publication: a user-triggered ContentGenerate freezes the current
-- checkpoint, worker publication creates explicit no-evidence bundles and a
-- candidate, and match_only reuses the same coarse job without a Candidate.
-- Arbitrary prompt/schema bytes can never create a generate Request. Positive
-- generate publication is exercised by content_agent_run_postgres with checked-in bytes.
DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  workspace_id uuid; workspace_value jsonb; retrieval jsonb;
  request_bytes bytea:=convert_to('{"target":"workspace","operation":"generate-negative"}','UTF8');
BEGIN
  SELECT id INTO STRICT workspace_id FROM bid_submission_workspaces
    WHERE project_id='10000000-0000-4000-8000-000000000010';
  workspace_value:=kb_bid_v2_load_workspace_for_actor(workspace_id,actor);
  PERFORM kb_bid_v2_create_outline_checkpoint(workspace_id,
    (workspace_value->>'revision_id')::uuid,(workspace_value->>'sha256')::kb_sha256,
    gen_random_uuid(),actor,'phase3-content-checkpoint',
    convert_to('{"checkpoint":"phase3-content"}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"checkpoint":"phase3-content"}','UTF8')));
  retrieval:=kb_knowledge_freeze_retrieval_identity_v1()||jsonb_build_object(
    'eligible_scope_sha256','715d78b3301b4e5901d8dc93c9d33776a0cef3378d65de32353ee8e998541901');
  BEGIN
    PERFORM kb_bid_v2_create_content_request(
      workspace_id,(workspace_value->>'revision_id')::uuid,(workspace_value->>'sha256')::kb_sha256,
      'generate','workspace',NULL,'append_candidate',NULL,'system_proposed',NULL,
      retrieval,convert_to(retrieval::text,'UTF8'),
      '{"base_url":"http://127.0.0.1:18080","credential_ref":"env:KNOWLEDGEBRAIN_CHAT_API_KEY","endpoint":"http://127.0.0.1:18080/v1/chat/completions","max_tokens":8192,"model_id":"scripted-content","protocol":"openai_chat_completions_sse","reasoning_effort":null,"response_mode":"strict_json_schema","schema_version":1,"stream":true,"temperature":null,"timeout_ms":180000,"transport_retries":0}'::jsonb,
      convert_to('arbitrary-prompt','UTF8'),convert_to('{}','UTF8'),actor,
      'phase3-arbitrary-agent-contract',request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
    RAISE EXCEPTION 'arbitrary Content prompt/schema unexpectedly accepted';
  EXCEPTION WHEN check_violation THEN
    IF SQLERRM NOT LIKE 'FROZEN_INPUT_DIGEST_MISMATCH%' THEN RAISE; END IF;
  END;
END $$;
