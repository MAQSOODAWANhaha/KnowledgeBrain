\set ON_ERROR_STOP on

DO $$
DECLARE
  project_id uuid:='00000000-0000-4000-8000-000000000010';
  workspace_id uuid:='00000000-0000-4000-8000-0000000000a0';
  actor kb_actor_identity:='user:00000000-0000-4000-8000-000000000001';
  head bid_workspace_heads%ROWTYPE;
  checkpoint jsonb; assessments jsonb; preview text; preview_input jsonb; request_value jsonb; input_value jsonb; result_value jsonb;
  content_request jsonb; content_input jsonb; content_request_id uuid; content_frozen_sha kb_sha256;
  request_bytes bytea; request_sha kb_sha256; request_id uuid; frozen_sha kb_sha256; manifest_sha kb_sha256;
  quote_payload jsonb; quote_bytes bytea; quote_sha kb_sha256; quote_id uuid; quote_snapshot_id uuid:=gen_random_uuid();
  quote_staging_id uuid:=gen_random_uuid(); quote_revision bigint;
  font_sha kb_sha256:='5d0df56f107605387e0de494b22dfc7fb05d8d79ffd981474e7be11dbe571882';
  output_bytes bytea:=convert_to('%PDF-1.7 frozen render output','UTF8'); output_sha kb_sha256;
  font_staging uuid:=gen_random_uuid(); output_staging uuid:=gen_random_uuid(); output_id uuid:=gen_random_uuid();
  snapshot_id uuid:=gen_random_uuid(); expected_manifest_id uuid:=gen_random_uuid();
BEGIN
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=workspace_id;
  request_bytes:=convert_to(jsonb_build_object('workspace_id',workspace_id,'checkpoint',head.artifact_id)::text,'UTF8');
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  checkpoint:=kb_bid_v2_create_outline_checkpoint(workspace_id,head.artifact_id,head.artifact_sha256,
    gen_random_uuid(),actor,'phase6-checkpoint',request_bytes,request_sha);
  IF checkpoint->>'sha256' IS NULL THEN RAISE EXCEPTION 'phase6 checkpoint missing identity'; END IF;

  SELECT (value->>'quote_id')::uuid,(value->>'next_revision')::bigint INTO quote_id,quote_revision
    FROM (SELECT kb_bid_v2_next_quote_snapshot_revision(project_id,actor) value) next_quote;
  quote_payload:=jsonb_build_object('schema_version',1,'quote_id',quote_id,'project_id',project_id,
    'revision',quote_revision,'currency_code','CNY','currency_scale',2,'tax_mode','tax_exclusive','title','阶段六正式报价','notes',NULL,
    'lines',jsonb_build_array(jsonb_build_object('id',gen_random_uuid(),'ordinal',0,'description','实施服务',
      'pricing_mode','unit_price','quantity','2.000000','unit','项','unit_price','100.000000','entered_amount',NULL,
      'tax_rate','0.060000','basis_amount','200.00','net_amount','200.00','tax_amount','12.00','gross_amount','212.00','user_confirmed',true)),
    'net_total','200.00','tax_total','12.00','gross_total','212.00','ceiling',NULL,
    'no_ceiling_review',jsonb_build_object('reviewed',true,'reason','招标文件未设置最高限价，已人工复核','actor_kind','user',
      'actor_id',substr(actor,6),'at','2026-01-02T03:04:05.000000Z'),
    'fact_revision',NULL,'pricing_revision',NULL,'pricing_set_sha256',NULL);
  quote_bytes:=convert_to(quote_payload::text,'UTF8');quote_sha:=kb_bid_v2_sha256_bytes(quote_bytes);
  PERFORM kb_object_upload_stage(quote_staging_id,('objects/'||quote_sha)::kb_object_ref,quote_sha,'application/json',
    octet_length(quote_bytes),actor);
  request_bytes:=convert_to('{"quote":"phase6"}','UTF8');request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  result_value:=kb_bid_v2_publish_quote_snapshot(project_id,quote_snapshot_id,quote_revision,quote_staging_id,
    ('objects/'||quote_sha)::kb_object_ref,quote_sha,octet_length(quote_bytes),quote_bytes,actor,
    'phase6-quote-snapshot',request_bytes,request_sha);
  IF (result_value->>'quote_snapshot_id')::uuid<>quote_snapshot_id OR result_value->>'sha256'<>quote_sha
     OR result_value#>>'{workspace_revision,revision_id}' IS NULL THEN
    RAISE EXCEPTION 'phase6 quote snapshot publication invalid: %',result_value;
  END IF;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=workspace_id;
  IF head.artifact_id::text<>result_value#>>'{workspace_revision,revision_id}'
     OR head.artifact_sha256<>result_value#>>'{workspace_revision,sha256}' THEN
    RAISE EXCEPTION 'phase6 quote publication did not atomically advance WorkspaceHead';
  END IF;

  -- ContentGenerate freezes the immutable QuoteSnapshot identity at request
  -- creation and the exact worker loader consumes that frozen artifact.
  request_bytes:=convert_to('{"content":"frozen-quote"}','UTF8');request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  content_request:=kb_bid_v2_create_content_request(workspace_id,head.artifact_id,head.artifact_sha256,
    'match_only','workspace',NULL,'append_candidate',NULL,'system_proposed',NULL,actor,
    'phase6-content-frozen-quote',request_bytes,request_sha);
  content_request_id:=(content_request->>'request_artifact_id')::uuid;
  content_frozen_sha:=(content_request->>'frozen_input_sha256')::kb_sha256;
  content_input:=kb_bid_v2_load_content_generation_input(content_request_id,1,content_frozen_sha);
  IF content_input#>>'{quote_snapshot,artifact_id}'<>quote_snapshot_id::text
    OR content_input#>>'{quote_snapshot,sha256}'<>quote_sha THEN
    RAISE EXCEPTION 'ContentGenerate did not consume its frozen QuoteSnapshot: %',content_input;
  END IF;
  PERFORM kb_bid_v2_mark_content_generation_failed(content_request_id,1,content_frozen_sha,'PHASE6_FIXTURE_COMPLETE');

  assessments:=kb_bid_v2_get_current_assessments(workspace_id,actor);
  IF assessments#>>'{outline,status}' NOT IN ('ready','has_warnings','has_critical_warnings')
     OR assessments#>>'{submission,status}' NOT IN ('ready','has_warnings','has_critical_warnings') THEN
    RAISE EXCEPTION 'phase6 assessments invalid: %',assessments;
  END IF;
 preview_input:=kb_bid_v2_load_preview_input(workspace_id,actor);
 IF preview_input->>'title' IS NULL OR jsonb_typeof(preview_input->'workspace')<>'object'
    OR jsonb_typeof(preview_input->'assets')<>'array' OR jsonb_typeof(preview_input->'preparations')<>'array' THEN
 RAISE EXCEPTION 'phase6 renderer preview input invalid: %',preview_input;
 END IF;
 preview:=kb_bid_v2_get_preview_html(workspace_id,actor);
 IF preview NOT LIKE '<!doctype html>%' OR preview NOT LIKE '%</html>' THEN
 RAISE EXCEPTION 'phase6 preview invalid';
 END IF;

  request_bytes:=convert_to(jsonb_build_object('mode','review','format','pdf',
    'expected_workspace_revision_id',head.artifact_id)::text,'UTF8');
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  request_value:=kb_bid_v2_create_submission_export_request(workspace_id,head.artifact_id,head.artifact_sha256,
    'review_draft','pdf','{"watermark":"评审稿","include_assessment_notices":true,"include_knowledge_sources":false}'::jsonb,
    actor,'phase6-export',request_bytes,request_sha);
  request_id:=(request_value->>'request_artifact_id')::uuid;
  frozen_sha:=(request_value->>'frozen_input_sha256')::kb_sha256;
  input_value:=kb_bid_v2_load_submission_export_input(request_id,1,frozen_sha);
  IF input_value#>>'{request,format}'<>'pdf' OR input_value#>>'{request,output_mode}'<>'review_draft' THEN
    RAISE EXCEPTION 'phase6 frozen export input invalid: %',input_value;
  END IF;

  output_sha:=kb_bid_v2_sha256_bytes(output_bytes);
  PERFORM kb_object_upload_stage(font_staging,'objects/'||font_sha,font_sha,'font/otf',4472168,
    'system:submission-export-v2');
  result_value:=kb_bid_v2_prepare_submission_export(request_id,1,frozen_sha,font_staging,
    ('objects/'||font_sha)::kb_object_ref,font_sha,'font/otf',snapshot_id,expected_manifest_id,
    'system:submission-export-v2');
  IF (result_value->>'artifact_id')::uuid<>expected_manifest_id
     OR (result_value->>'render_snapshot_id')::uuid<>snapshot_id THEN
    RAISE EXCEPTION 'phase6 prepare identity invalid: %',result_value;
  END IF;
  IF (SELECT margins_mm FROM bid_render_document_snapshot_artifacts WHERE id=snapshot_id)
       IS DISTINCT FROM '{"top":25.4,"right":25.4,"bottom":25.4,"left":25.4}'::jsonb THEN
    RAISE EXCEPTION 'page-size-only settings did not freeze renderer default margins';
  END IF;
  manifest_sha:=(result_value->>'sha256')::kb_sha256;
  input_value:=kb_bid_v2_load_submission_manifest_render_input(expected_manifest_id,manifest_sha);
  IF input_value#>>'{prepared_manifest,render_snapshot_id}'<>snapshot_id::text THEN
    RAISE EXCEPTION 'phase6 prepared manifest loader invalid: %',input_value;
  END IF;
  PERFORM kb_object_upload_stage(output_staging,'objects/'||output_sha,output_sha,'application/pdf',
    octet_length(output_bytes),'system:submission-export-v2');
  result_value:=kb_bid_v2_publish_submission_export(request_id,1,frozen_sha,font_staging,
    ('objects/'||font_sha)::kb_object_ref,font_sha,'font/otf',snapshot_id,expected_manifest_id,output_staging,
    output_id,('objects/'||output_sha)::kb_object_ref,output_sha,'application/pdf',octet_length(output_bytes),
    'system:submission-export-v2');
  IF (result_value->>'artifact_id')::uuid<>output_id OR (result_value->>'manifest_id')::uuid<>expected_manifest_id THEN
    RAISE EXCEPTION 'phase6 output publication identity invalid: %',result_value;
  END IF;
  IF NOT EXISTS(SELECT 1 FROM bid_async_request_snapshot_artifacts WHERE id=request_id AND status='succeeded')
     OR NOT EXISTS(SELECT 1 FROM bid_async_stage_receipts WHERE request_artifact_id=request_id AND stage_kind='render')
     OR NOT EXISTS(SELECT 1 FROM bid_async_stage_receipts WHERE request_artifact_id=request_id AND stage_kind='package')
     OR NOT EXISTS(SELECT 1 FROM bid_submission_manifest_dependencies dependency WHERE dependency.manifest_id=expected_manifest_id)
     OR NOT EXISTS(SELECT 1 FROM bid_submission_output_artifacts output WHERE output.id=output_id AND output.content_sha256=output_sha)
     OR NOT EXISTS(SELECT 1 FROM bid_submission_assessment_report_artifacts report WHERE report.submission_output_id=output_id) THEN
    RAISE EXCEPTION 'phase6 export graph incomplete';
  END IF;
  IF jsonb_array_length(kb_bid_v2_list_submission_exports(workspace_id,actor))<1
     OR kb_bid_v2_get_submission_export_object(workspace_id,output_id,actor)->>'object_ref'<>'objects/'||output_sha THEN
    RAISE EXCEPTION 'phase6 export retrieval invalid';
  END IF;
END $$;

-- Failure injection: a post-render SQL validation failure must not publish an
-- output or output-owner reference. The worker's error path abandons staging.
DO $$
DECLARE target_workspace_id uuid:='00000000-0000-4000-8000-0000000000a0';
  actor kb_actor_identity:='user:00000000-0000-4000-8000-000000000001';
  worker_actor kb_actor_identity:='system:submission-export-v2';head bid_workspace_heads%ROWTYPE;
  request_value jsonb; prepared jsonb; request_id uuid; frozen_sha kb_sha256;
  request_bytes bytea; request_sha kb_sha256; font_sha kb_sha256:='5d0df56f107605387e0de494b22dfc7fb05d8d79ffd981474e7be11dbe571882';
  font_staging uuid:=gen_random_uuid(); output_staging uuid:=gen_random_uuid(); output_id uuid;
  snapshot_id uuid:=gen_random_uuid(); failure_manifest_id uuid:=gen_random_uuid();
  output_ref kb_object_ref; output_sha kb_sha256; output_media text; output_length bigint;
  prior_owner_count bigint;
BEGIN
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=target_workspace_id;
  request_bytes:=convert_to('{"mode":"review_draft","format":"pdf","failure_injection":true}','UTF8');
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  request_value:=kb_bid_v2_create_submission_export_request(target_workspace_id,head.artifact_id,head.artifact_sha256,
    'review_draft','pdf','{"watermark":"失败注入","include_assessment_notices":true,"include_knowledge_sources":false}'::jsonb,
    actor,'phase6-export-failure-injection',request_bytes,request_sha);
  request_id:=(request_value->>'request_artifact_id')::uuid;frozen_sha:=(request_value->>'frozen_input_sha256')::kb_sha256;
  PERFORM kb_object_upload_stage(font_staging,('objects/'||font_sha)::kb_object_ref,font_sha,'font/otf',4472168,worker_actor);
  prepared:=kb_bid_v2_prepare_submission_export(request_id,1,frozen_sha,font_staging,
    ('objects/'||font_sha)::kb_object_ref,font_sha,'font/otf',snapshot_id,failure_manifest_id,worker_actor);
  SELECT output.id,output.object_ref,output.content_sha256,output.media_type,output.byte_length INTO STRICT
    output_id,output_ref,output_sha,output_media,output_length
    FROM bid_submission_output_artifacts output WHERE output.workspace_id=target_workspace_id
    ORDER BY output.created_at,output.id LIMIT 1;
  SELECT count(*) INTO prior_owner_count FROM object_owner_references
    WHERE owner_kind='bid_submission_output' AND owner_id=output_id;
  PERFORM kb_object_upload_stage(output_staging,output_ref,output_sha,output_media,output_length,worker_actor);
  BEGIN
    -- Object commit is byte/identity valid and succeeds first; reusing an
    -- existing output UUID then fails at the immutable output INSERT. The
    -- enclosing SQL transaction must roll back the attempted new owner ref.
    PERFORM kb_bid_v2_publish_submission_export(request_id,1,frozen_sha,font_staging,
      ('objects/'||font_sha)::kb_object_ref,font_sha,'font/otf',snapshot_id,failure_manifest_id,output_staging,output_id,
      output_ref,output_sha,output_media,output_length,worker_actor);
    RAISE EXCEPTION 'failure injection unexpectedly published output';
  EXCEPTION WHEN unique_violation THEN NULL; END;
  PERFORM kb_object_upload_abandon(output_staging,worker_actor);
  PERFORM kb_bid_v2_mark_submission_export_failed(request_id,1,frozen_sha,'OBJECT_COMMIT_FAILED');
  IF EXISTS(SELECT 1 FROM object_upload_staging WHERE id=output_staging)
    OR EXISTS(SELECT 1 FROM object_owner_references WHERE owner_kind='object_upload_staging' AND owner_id=output_staging) THEN
    RAISE EXCEPTION 'failed export did not abandon output staging';
  END IF;
  IF EXISTS(SELECT 1 FROM bid_submission_output_artifacts output WHERE output.manifest_id=failure_manifest_id) THEN
    RAISE EXCEPTION 'failed export published an output artifact';
  END IF;
  IF EXISTS(SELECT 1 FROM object_owner_references WHERE owner_kind='bid_submission_output' AND owner_id=output_id
      AND occurrence='output:'||head.project_id::text||':'||target_workspace_id::text||':'||failure_manifest_id::text)
    OR (SELECT count(*) FROM object_owner_references WHERE owner_kind='bid_submission_output' AND owner_id=output_id)<>prior_owner_count THEN
    RAISE EXCEPTION 'failed export leaked an output owner reference';
  END IF;
  IF NOT EXISTS(SELECT 1 FROM bid_async_request_snapshot_artifacts WHERE id=request_id AND status='failed') THEN
    RAISE EXCEPTION 'failed export request did not reach terminal failed';
  END IF;
  IF EXISTS(SELECT 1 FROM bid_async_stage_receipts WHERE request_artifact_id=request_id AND stage_kind IN ('object_commit','package')) THEN
    RAISE EXCEPTION 'failed export published a terminal stage receipt';
  END IF;
END $$;
