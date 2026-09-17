\set ON_ERROR_STOP on

DO $$
DECLARE
  project_id uuid:='00000000-0000-4000-8000-000000000010';
  workspace_id uuid:='00000000-0000-4000-8000-0000000000a0';
  actor kb_actor_identity:='user:00000000-0000-4000-8000-000000000001';
  head bid_workspace_heads%ROWTYPE;
  checkpoint jsonb; assessments jsonb; preview text; preview_input jsonb; result_value jsonb;
  content_request jsonb; content_input jsonb; content_request_id uuid; content_frozen_sha kb_sha256;
  request_bytes bytea; request_sha kb_sha256;
  quote_payload jsonb; quote_bytes bytea; quote_sha kb_sha256; quote_id uuid; quote_snapshot_id uuid:=gen_random_uuid();
  quote_staging_id uuid:=gen_random_uuid(); quote_revision bigint;
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
     OR result_value->>'workspace_apply_required'<>'true' THEN
    RAISE EXCEPTION 'phase6 quote snapshot publication invalid: %',result_value;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_workspace_heads WHERE scope_id=workspace_id
      AND artifact_id=head.artifact_id AND artifact_sha256=head.artifact_sha256) THEN
    RAISE EXCEPTION 'quote publication unexpectedly advanced WorkspaceHead';
  END IF;
  request_bytes:=convert_to('{"apply_quote":"phase6"}','UTF8');
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  result_value:=kb_bid_v2_apply_quote_snapshot(workspace_id,quote_snapshot_id,quote_sha,
    head.artifact_id,head.artifact_sha256,actor,'phase6-quote-apply',request_bytes,request_sha);
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=workspace_id;
  IF head.artifact_id::text<>result_value->>'revision_id'
     OR head.artifact_sha256<>result_value->>'sha256' THEN
    RAISE EXCEPTION 'explicit quote apply did not atomically advance WorkspaceHead';
  END IF;

  -- ContentGenerate freezes the immutable QuoteSnapshot identity at request
  -- creation and the exact worker loader consumes that frozen artifact.
  request_bytes:=convert_to('{"content":"frozen-quote"}','UTF8');request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  SELECT kb_knowledge_freeze_retrieval_identity_v1()||jsonb_build_object(
      'eligible_scope_sha256','715d78b3301b4e5901d8dc93c9d33776a0cef3378d65de32353ee8e998541901')
    INTO STRICT content_input;
  content_request:=kb_bid_v2_create_content_request(workspace_id,head.artifact_id,head.artifact_sha256,
    'match_only','workspace',NULL,'append_candidate',NULL,'system_proposed',NULL,
    content_input,convert_to(content_input::text,'UTF8'),NULL,NULL,NULL,actor,
    'phase6-content-frozen-quote',request_bytes,request_sha);
  content_request_id:=(content_request->>'request_artifact_id')::uuid;
  content_frozen_sha:=(content_request->>'frozen_input_sha256')::kb_sha256;
  content_input:=kb_bid_v2_load_content_generation_input(content_request_id,1,content_frozen_sha);
  IF content_input#>>'{quote_snapshot,artifact_id}'<>quote_snapshot_id::text
    OR content_input#>>'{quote_snapshot,sha256}'<>quote_sha THEN
    RAISE EXCEPTION 'ContentGenerate did not consume its frozen QuoteSnapshot: %',content_input;
  END IF;
  PERFORM kb_bid_v2_mark_content_generation_failed(content_request_id,1,content_frozen_sha,
    'CONTENT_MATCH_TIMEOUT','phase6 fixture complete',NULL,NULL);

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

END $$;

-- Formal export contract: SQL verifies frozen identities and atomicity. These
-- bytes are identity fixtures, not a real DOCX/PDF or semantic acceptance.
DO $$
DECLARE
 workspace_value uuid; export_project uuid:=gen_random_uuid();
 actor kb_actor_identity:='user:00000000-0000-4000-8000-000000000001';
 worker_actor kb_actor_identity:='system:submission-export-v2';
 mime text:='application/vnd.openxmlformats-officedocument.wordprocessingml.document';
 source_bytes bytea:=convert_to('SQL saved DOCX identity fixture','UTF8');
 pdf_bytes bytea:=convert_to('%PDF SQL conversion identity fixture','UTF8');
 source_sha kb_sha256; pdf_sha kb_sha256; staging uuid:=gen_random_uuid();
 basis jsonb; current_value jsonb; next_value jsonb; request_value jsonb; request_id uuid; frozen_sha kb_sha256;
 request_bytes bytea:=convert_to('{"export":"phase6-frozen-docx"}','UTF8'); request_sha kb_sha256;
 source_value jsonb; loaded jsonb; docx jsonb; pdf jsonb; report jsonb; result_value jsonb; replay jsonb;
 package_id uuid:=gen_random_uuid(); docx_id uuid:=gen_random_uuid(); pdf_id uuid:=gen_random_uuid();
 docx_staging uuid:=gen_random_uuid(); pdf_staging uuid:=gen_random_uuid(); report_value jsonb;
 claim jsonb; attempt integer; owner_token uuid; inventory jsonb; snapshot jsonb; render jsonb;
BEGIN
 -- The earlier phases deliberately use non-JSON legacy requirement payloads.
 -- Formal export uses a production-created project with a valid frozen basis.
 PERFORM kb_bid_v2_create_project(export_project,'phase6 saved DOCX export',
   '00000000-0000-4000-8000-000000000001',actor,gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
 SELECT id INTO STRICT workspace_value FROM bid_submission_workspaces WHERE project_id=export_project;
 source_sha:=kb_bid_v2_sha256_bytes(source_bytes);pdf_sha:=kb_bid_v2_sha256_bytes(pdf_bytes);
 SELECT jsonb_build_object('document_set_id',d.artifact_id,'document_set_sha256',d.artifact_sha256,
   'requirement_set_id',r.artifact_id,'requirement_set_sha256',r.artifact_sha256,
   'expected_version_id',NULL,'expected_docx_sha256',NULL,
   'docx_sha256',source_sha,'byte_length',octet_length(source_bytes)) INTO STRICT basis
 FROM bid_document_set_current d JOIN bid_requirement_set_current r ON r.scope_id=d.scope_id
 JOIN bid_submission_workspaces w ON w.project_id=d.scope_id WHERE w.id=workspace_value;
 PERFORM kb_object_upload_stage(staging,'objects/'||source_sha,source_sha,mime,octet_length(source_bytes),actor);
 current_value:=kb_bid_v2_create_docx_round(workspace_value,staging,basis,actor,'phase6-docx-round');
 request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
 BEGIN
   PERFORM kb_bid_v2_create_submission_export_request(workspace_value,gen_random_uuid(),source_sha,actor,'phase6-bad-cas',request_bytes,request_sha);
   RAISE EXCEPTION 'export accepted a stale version';
 EXCEPTION WHEN serialization_failure THEN
   IF SQLERRM<>'DOCX_VERSION_CAS_MISMATCH' THEN RAISE; END IF;
 END;
 UPDATE bid_docx_current SET editor_key=gen_random_uuid(),editor_base_version_id=version_id,pending_save_id=gen_random_uuid() WHERE scope_id=workspace_value;
 BEGIN
   PERFORM kb_bid_v2_create_submission_export_request(workspace_value,(current_value->>'version_id')::uuid,source_sha,actor,'phase6-pending',request_bytes,request_sha);
   RAISE EXCEPTION 'export accepted a pending save';
 EXCEPTION WHEN serialization_failure THEN
   IF SQLERRM<>'DOCX_SAVE_PENDING' THEN RAISE; END IF;
 END;
 UPDATE bid_docx_current SET pending_save_id=NULL,editor_error='{"kind":"callback","code":7}' WHERE scope_id=workspace_value;
 BEGIN
   PERFORM kb_bid_v2_create_submission_export_request(workspace_value,(current_value->>'version_id')::uuid,source_sha,actor,'phase6-error',request_bytes,request_sha);
   RAISE EXCEPTION 'export accepted a failed save';
 EXCEPTION WHEN object_not_in_prerequisite_state THEN
   IF SQLERRM<>'DOCX_SAVE_ERROR' THEN RAISE; END IF;
 END;
 UPDATE bid_docx_current SET editor_key=NULL,editor_base_version_id=NULL,editor_error=NULL WHERE scope_id=workspace_value;
 request_value:=kb_bid_v2_create_submission_export_request(workspace_value,(current_value->>'version_id')::uuid,source_sha,actor,'phase6-export',request_bytes,request_sha);
 request_id:=(request_value->>'request_artifact_id')::uuid;frozen_sha:=(request_value->>'frozen_input_sha256')::kb_sha256;
 loaded:=kb_bid_v2_load_submission_export_input(request_id,1,frozen_sha);source_value:=loaded->'source';
 claim:=kb_bid_v2_tender_agent_claim(request_id,1,frozen_sha);
 attempt:=(claim->>'attempt')::integer; owner_token:=(claim->>'execution_owner_token')::uuid;
 IF claim->>'disposition'<>'claimed' THEN RAISE EXCEPTION 'export owner not claimed'; END IF;
 IF source_value->>'docx_sha256'<>source_sha OR loaded->'published'<>'null'::jsonb OR loaded ? 'workspace' THEN
   RAISE EXCEPTION 'export did not freeze the saved DOCX';
 END IF;
 -- A later independently saved round must not change the export source.
 basis:=basis||jsonb_build_object('expected_version_id',current_value->'version_id','expected_docx_sha256',source_sha,
   'docx_sha256',kb_bid_v2_sha256_bytes(convert_to('later DOCX','UTF8')),'byte_length',10);
 staging:=gen_random_uuid();
 PERFORM kb_object_upload_stage(staging,'objects/'||(basis->>'docx_sha256'),(basis->>'docx_sha256')::kb_sha256,mime,10,actor);
 next_value:=kb_bid_v2_create_docx_round(workspace_value,staging,basis,actor,'phase6-docx-later');
 IF kb_bid_v2_load_submission_export_source(workspace_value,request_id,(current_value->>'version_id')::uuid,source_sha) IS DISTINCT FROM source_value THEN
   RAISE EXCEPTION 'source URL resolved the live edited head';
 END IF;
 BEGIN
   PERFORM kb_bid_v2_load_submission_export_source(workspace_value,request_id,(next_value->>'version_id')::uuid,(basis->>'docx_sha256')::kb_sha256);
   RAISE EXCEPTION 'source capability accepted another version';
 EXCEPTION WHEN no_data_found THEN NULL; END;
 replay:=kb_bid_v2_create_submission_export_request(workspace_value,(current_value->>'version_id')::uuid,source_sha,actor,'phase6-export',request_bytes,request_sha);
 IF replay IS DISTINCT FROM request_value THEN RAISE EXCEPTION 'request replay changed frozen identity'; END IF;
 PERFORM kb_object_upload_stage(docx_staging,'objects/'||source_sha,source_sha,mime,octet_length(source_bytes),worker_actor);
 PERFORM kb_object_upload_stage(pdf_staging,'objects/'||pdf_sha,pdf_sha,'application/pdf',octet_length(pdf_bytes),worker_actor);
 docx:=jsonb_build_object('staging_id',docx_staging,'artifact_id',docx_id,'object_ref','objects/'||source_sha,
   'sha256',source_sha,'media_type',mime,'byte_length',octet_length(source_bytes));
 pdf:=jsonb_build_object('staging_id',pdf_staging,'artifact_id',pdf_id,'object_ref','objects/'||pdf_sha,
   'sha256',pdf_sha,'media_type','application/pdf','byte_length',octet_length(pdf_bytes));
 render:=jsonb_build_object('schema_version',1,'source',source_value,'pdf',pdf-ARRAY['staging_id','artifact_id']);
 PERFORM kb_bid_v2_submission_export_render_put(request_id,frozen_sha,attempt,owner_token,pdf_staging,render);
 -- Synthetic parser manifest identities exercise persistence only, not semantic parsing.
 inventory:=jsonb_build_object('docx_sha256',source_sha,'pdf_sha256',pdf_sha,'units','[]'::jsonb,'images','{}'::jsonb,
   'parser_manifests',jsonb_build_array(
     jsonb_build_object('schema_version',1,'profile','output_inventory_v1','file_sha256',source_sha,'parser','synthetic-storage-fixture','config','{}'::jsonb),
     jsonb_build_object('schema_version',1,'profile','output_inventory_v1','file_sha256',pdf_sha,'parser','synthetic-storage-fixture','config','{}'::jsonb)));
 snapshot:=jsonb_build_object('schema_version',2,'inventory',inventory,
   'inventory_sha256',kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(inventory),'UTF8')));
 PERFORM kb_bid_v2_submission_export_snapshot_put(request_id,frozen_sha,attempt,owner_token,snapshot);
 -- The durable render adopted the first stage; publication adopts a new one.
 pdf_staging:=gen_random_uuid();
 PERFORM kb_object_upload_stage(pdf_staging,'objects/'||pdf_sha,pdf_sha,'application/pdf',octet_length(pdf_bytes),worker_actor);
 pdf:=pdf||jsonb_build_object('staging_id',pdf_staging);
 report:=jsonb_build_object('schema_version',2,'source',source_value,'outputs',jsonb_build_object(
   'docx',docx-ARRAY['staging_id','object_ref','media_type'],'pdf',pdf-ARRAY['staging_id','object_ref','media_type']),
   'output_images','{}'::jsonb,'output_inventory',jsonb_build_object('inventory_sha256',snapshot->'inventory_sha256'),
   'checks',jsonb_build_array(jsonb_build_object('id','export_review','status','not_checked','detail','SQL identity fixture only')));
 BEGIN
   PERFORM kb_bid_v2_publish_submission_export(request_id,1,frozen_sha,package_id,docx,pdf,
     report||jsonb_build_object('source',source_value||jsonb_build_object('version_id',next_value->'version_id')),worker_actor,attempt,owner_token);
   RAISE EXCEPTION 'report accepted the wrong source';
 EXCEPTION WHEN check_violation THEN
   IF SQLERRM<>'SUBMISSION_REPORT_IDENTITY_INVALID' THEN RAISE; END IF;
 END;
 -- PDF staging failure happens after DOCX commit; transaction rollback must
 -- remove the manifest, DOCX output, owner ref and all publication receipts.
 BEGIN
   PERFORM kb_bid_v2_publish_submission_export(request_id,1,frozen_sha,package_id,docx,
     pdf||jsonb_build_object('staging_id',gen_random_uuid()),report,worker_actor,attempt,owner_token);
   RAISE EXCEPTION 'export accepted missing PDF staging';
 EXCEPTION WHEN no_data_found OR check_violation THEN NULL; END;
 IF EXISTS(SELECT 1 FROM bid_submission_manifest_artifacts WHERE id=package_id)
   OR EXISTS(SELECT 1 FROM bid_submission_output_artifacts WHERE id=docx_id)
   OR EXISTS(SELECT 1 FROM object_owner_references WHERE owner_kind='bid_submission_output' AND owner_id=docx_id)
   OR EXISTS(SELECT 1 FROM bid_async_stage_receipts WHERE request_artifact_id=request_id AND stage_kind='package') THEN
   RAISE EXCEPTION 'failed pair publication leaked a partial artifact';
 END IF;
 result_value:=kb_bid_v2_publish_submission_export(request_id,1,frozen_sha,package_id,docx,pdf,report,worker_actor,attempt,owner_token);
 replay:=kb_bid_v2_publish_submission_export(request_id,1,frozen_sha,gen_random_uuid(),NULL,NULL,NULL,worker_actor,attempt,owner_token);
 IF replay IS DISTINCT FROM result_value OR result_value#>>'{outputs,docx,sha256}'<>source_sha
   OR result_value#>>'{outputs,pdf,sha256}'<>pdf_sha THEN RAISE EXCEPTION 'atomic package/replay identity invalid'; END IF;
 IF (SELECT count(*) FROM bid_submission_output_artifacts WHERE bid_submission_output_artifacts.manifest_id=package_id)<>2 THEN
   RAISE EXCEPTION 'package did not publish exactly two outputs';
 END IF;
 report_value:=kb_bid_v2_get_submission_assessment_report(workspace_value,package_id,actor);
 IF report_value-'content_sha256' IS DISTINCT FROM report
   OR report_value IS DISTINCT FROM kb_bid_v2_get_submission_assessment_report(workspace_value,pdf_id,actor)
   OR report_value IS DISTINCT FROM kb_bid_v2_get_submission_assessment_report(workspace_value,docx_id,actor) THEN
   RAISE EXCEPTION 'report not bound to both outputs';
 END IF;
 IF kb_bid_v2_get_submission_export(workspace_value,package_id,actor)->'source' IS DISTINCT FROM source_value
   OR kb_bid_v2_get_submission_export_object(workspace_value,docx_id,actor)->>'sha256'<>source_sha
   OR kb_bid_v2_get_submission_export_object(workspace_value,pdf_id,actor)->>'sha256'<>pdf_sha
   OR jsonb_array_length(kb_bid_v2_list_submission_exports(workspace_value,actor))<>1 THEN
   RAISE EXCEPTION 'package read APIs lost frozen identity';
 END IF;
 request_value:=kb_bid_v2_create_submission_export_request(workspace_value,(next_value->>'version_id')::uuid,
   (basis->>'docx_sha256')::kb_sha256,actor,'phase6-failed-export',request_bytes,request_sha);
 request_id:=(request_value->>'request_artifact_id')::uuid;frozen_sha:=(request_value->>'frozen_input_sha256')::kb_sha256;
 PERFORM kb_bid_v2_mark_submission_export_failed(request_id,1,frozen_sha,'RENDERER_FAILED');
 BEGIN
   PERFORM kb_bid_v2_load_submission_export_source(workspace_value,request_id,(next_value->>'version_id')::uuid,(basis->>'docx_sha256')::kb_sha256);
   RAISE EXCEPTION 'failed request still allowed conversion-source access';
 EXCEPTION WHEN no_data_found THEN NULL; END;
 BEGIN
   PERFORM kb_bid_v2_publish_submission_export(request_id,1,frozen_sha,gen_random_uuid(),docx,pdf,report,worker_actor,attempt,owner_token);
   RAISE EXCEPTION 'failed request was resurrected by publication';
 EXCEPTION WHEN object_not_in_prerequisite_state THEN
   IF SQLERRM<>'SUBMISSION_EXPORT_NOT_PENDING' THEN RAISE; END IF;
 END;
END $$;
