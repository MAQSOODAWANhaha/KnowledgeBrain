-- Live semantic-spine contract upgrade. Safe for an existing V2 database.
BEGIN;

CREATE OR REPLACE FUNCTION kb_actor_identity_valid(value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $$
    SELECT value ~ '^(user|api_key):[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        OR value IN (
            'system:bid-convert-worker',
            'system:bid-attachment-preparation',
            'system:bid-extraction-worker',
            'system:content-generate-v2',
            'system:clause-lifecycle',
            'system:kind-router-promotion',
            'system:maintenance',
            'system:knowledge-document-delete',
            'system:knowledge-document-ingest',
            'system:matching-invalidation',
            'system:matching-publication',
            'system:requirement-set-compile-v2',
            'system:requirement-set-compile-v3',
            'system:retention-consumer',
            'system:submission-export-v2',
            'system:tender-document-process-v2'
        )
$$;

CREATE OR REPLACE FUNCTION kb_bid_v2_load_requirement_set_compile_input_v3(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_requirement_set_compile_request_identities%ROWTYPE;
BEGIN
  SELECT * INTO STRICT typed FROM bid_requirement_set_compile_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  RETURN jsonb_build_object(
    'schema_version',3,'project_id',typed.project_id,
    'source_units',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'source_unit_revision_id',source.id,'document_id',source.document_id,
      'unit_kind',source.unit_kind,'ordinal',source.ordinal,
      'text',convert_from(source.text_utf8,'UTF8')) ORDER BY source.document_id,source.ordinal,source.id)
      FROM bid_source_unit_disposition_set_items disposition
      JOIN bid_source_unit_revision_artifacts source
        ON source.project_id=disposition.project_id AND source.id=disposition.source_unit_revision_id
      WHERE disposition.disposition_set_id=typed.disposition_set_revision_id
        AND disposition.disposition='requirement'),'[]'::jsonb),
    'structured_forms',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'form_definition_revision_id',form.id,'form_definition_sha256',form.content_sha256,
      'source_unit_revision_id',form.source_unit_revision_id,
      'definition',convert_from(form.canonical_payload,'UTF8')::jsonb)
      ORDER BY form.source_unit_revision_id,form.id)
      FROM bid_tender_structured_form_definition_artifacts form
      JOIN bid_source_unit_disposition_set_items disposition
        ON disposition.project_id=form.project_id
        AND disposition.source_unit_revision_id=form.source_unit_revision_id
      WHERE disposition.disposition_set_id=typed.disposition_set_revision_id
        AND disposition.disposition='requirement'),'[]'::jsonb));
END $$;

CREATE OR REPLACE FUNCTION kb_bid_v2_publish_requirement_set_v3(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_compiled jsonb,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  typed bid_requirement_set_compile_request_identities%ROWTYPE;
  prior bid_async_stage_receipts%ROWTYPE; source_value bid_source_unit_revision_artifacts%ROWTYPE;
  requirement_value jsonb; source_identity jsonb; requirement_id uuid; requirement_lineage uuid;
  requirement_revision bigint; requirement_payload bytea; requirement_sha kb_sha256;
  requirement_text bytea; requirement_text_sha kb_sha256; fulfillment jsonb; applicability_value jsonb;
  requirement_items jsonb:='[]'::jsonb; ordinal_value integer:=0;
  set_id uuid:=gen_random_uuid(); set_revision bigint; set_payload bytea; set_sha kb_sha256;
  workspace_value bid_submission_workspaces%ROWTYPE;
  projection_head bid_workspace_requirement_projection_current%ROWTYPE;
  projection_id uuid:=gen_random_uuid(); projection_revision bigint;
  projection_payload bytea; projection_sha kb_sha256; publication_status text;
  result_value jsonb; result_sha kb_sha256;
BEGIN
  IF p_actor<>'system:requirement-set-compile-v3' THEN
    RAISE EXCEPTION 'SYSTEM_ACTOR_REQUIRED' USING ERRCODE='42501';
  END IF;
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id AND request_kind='requirement_set_compile'
      AND revision=p_request_revision AND frozen_input_sha256=p_frozen_input_sha256 FOR UPDATE;
  SELECT * INTO STRICT typed FROM bid_requirement_set_compile_request_identities
    WHERE request_artifact_id=p_request_artifact_id;
  SELECT * INTO prior FROM bid_async_stage_receipts WHERE request_artifact_id=p_request_artifact_id
    AND stage_kind='requirement_compile' AND frozen_input_sha256=p_frozen_input_sha256;
  IF FOUND THEN RETURN prior.result_identity||jsonb_build_object('replayed',true); END IF;
  IF request_value.status<>'pending' THEN
    RAISE EXCEPTION 'REQUIREMENT_COMPILE_REQUEST_NOT_PENDING' USING ERRCODE='23514';
  END IF;
  IF jsonb_typeof(p_compiled)<>'object'
    OR NOT kb_bid_v2_json_keys_exact(p_compiled,ARRAY['schema_version','source_unit_revision_ids','requirements','notices'])
    OR p_compiled->'schema_version' IS DISTINCT FROM '3'::jsonb
    OR jsonb_typeof(p_compiled->'source_unit_revision_ids')<>'array'
    OR jsonb_typeof(p_compiled->'requirements')<>'array'
    OR jsonb_array_length(p_compiled->'requirements') NOT BETWEEN 1 AND 100000
    OR jsonb_typeof(p_compiled->'notices')<>'array' THEN
    RAISE EXCEPTION 'REQUIREMENT_COMPILE_OUTPUT_INVALID' USING ERRCODE='23514';
  END IF;
  IF EXISTS (
    (SELECT source.id FROM bid_source_unit_disposition_set_items disposition
      JOIN bid_source_unit_revision_artifacts source
        ON source.project_id=disposition.project_id AND source.id=disposition.source_unit_revision_id
      WHERE disposition.disposition_set_id=typed.disposition_set_revision_id
        AND disposition.disposition='requirement')
    EXCEPT
    (SELECT value::uuid FROM jsonb_array_elements_text(p_compiled->'source_unit_revision_ids') value)
  ) OR EXISTS (
    (SELECT value::uuid FROM jsonb_array_elements_text(p_compiled->'source_unit_revision_ids') value)
    EXCEPT
    (SELECT source.id FROM bid_source_unit_disposition_set_items disposition
      JOIN bid_source_unit_revision_artifacts source
        ON source.project_id=disposition.project_id AND source.id=disposition.source_unit_revision_id
      WHERE disposition.disposition_set_id=typed.disposition_set_revision_id
        AND disposition.disposition='requirement')
  ) THEN
    RAISE EXCEPTION 'REQUIREMENT_COMPILE_SOURCE_CLOSURE_INVALID' USING ERRCODE='23514';
  END IF;
  PERFORM 1 FROM bid_projects WHERE id=typed.project_id FOR UPDATE;
  SELECT coalesce(max(revision),0)+1 INTO set_revision
    FROM bid_requirement_set_artifacts WHERE project_id=typed.project_id;
  FOR requirement_value IN SELECT value FROM jsonb_array_elements(p_compiled->'requirements') LOOP
    IF jsonb_typeof(requirement_value)<>'object'
      OR NOT kb_bid_v2_json_keys_exact(requirement_value,ARRAY['requirement_ref','requirement_kind','requiredness',
        'compliance_policy','requirement_text','channel','applicability','source_unit_revision_ids','structured_form_revision_ids'])
      OR NOT kb_bid_v2_sha256_text(requirement_value->>'requirement_ref')
      OR requirement_value->>'requirement_kind' NOT IN ('qualification','technical','commercial','pricing','delivery','evaluation','format','attachment','other')
      OR requirement_value->>'requiredness' NOT IN ('mandatory','optional','informational')
      OR requirement_value->>'compliance_policy' NOT IN ('must_comply','explicit_response','deviation_allowed','scored')
      OR requirement_value->>'channel' NOT IN ('narrative_content','response_table','deviation_statement','structured_form','evidence_attachment','quotation')
      OR octet_length(requirement_value->>'requirement_text') NOT BETWEEN 1 AND 32768
      OR jsonb_typeof(requirement_value->'source_unit_revision_ids')<>'array'
      OR jsonb_array_length(requirement_value->'source_unit_revision_ids') NOT BETWEEN 1 AND 1000
      OR jsonb_typeof(requirement_value->'structured_form_revision_ids')<>'array'
      OR jsonb_typeof(requirement_value->'applicability')<>'object'
      OR NOT kb_bid_v2_json_keys_exact(requirement_value->'applicability',ARRAY['status','reason','source_unit_revision_ids'])
      OR requirement_value->'applicability'->>'status' NOT IN ('required','optional','conditional','not_applicable') THEN
      RAISE EXCEPTION 'REQUIREMENT_COMPILE_REQUIREMENT_INVALID' USING ERRCODE='23514';
    END IF;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(requirement_value->'source_unit_revision_ids') source_id
      WHERE NOT EXISTS (SELECT 1 FROM bid_source_unit_disposition_set_items disposition
        WHERE disposition.disposition_set_id=typed.disposition_set_revision_id
          AND disposition.disposition='requirement'
          AND disposition.source_unit_revision_id=source_id::uuid)) THEN
      RAISE EXCEPTION 'REQUIREMENT_COMPILE_SOURCE_SCOPE_INVALID' USING ERRCODE='23514';
    END IF;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(requirement_value->'structured_form_revision_ids') form_id
      WHERE NOT EXISTS (SELECT 1 FROM bid_tender_structured_form_definition_artifacts form
        WHERE form.project_id=typed.project_id AND form.id=form_id::uuid
          AND form.source_unit_revision_id IN (SELECT source_id::uuid
            FROM jsonb_array_elements_text(requirement_value->'source_unit_revision_ids') source_id))) THEN
      RAISE EXCEPTION 'REQUIREMENT_COMPILE_FORM_SCOPE_INVALID' USING ERRCODE='23514';
    END IF;
    requirement_id:=gen_random_uuid();
    requirement_lineage:=kb_bid_v2_deterministic_uuid(
      typed.project_id::text||':requirement-v3:'||(requirement_value->>'requirement_ref'));
    SELECT coalesce(max(revision),0)+1 INTO requirement_revision
      FROM bid_requirement_revision_artifacts
      WHERE project_id=typed.project_id AND lineage_id=requirement_lineage;
    requirement_text:=convert_to(requirement_value->>'requirement_text','UTF8');
    requirement_text_sha:=kb_bid_v2_sha256_bytes(requirement_text);
    fulfillment:=jsonb_build_object('kind','need','need_occurrence_id',requirement_id,
      'channel',requirement_value->>'channel');
    SELECT jsonb_build_object('fragments',jsonb_agg('source_unit:'||(source_id#>>'{}') ORDER BY source_id#>>'{}'))
      INTO applicability_value
      FROM jsonb_array_elements(requirement_value->'source_unit_revision_ids') source_id;
    requirement_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
      'lineage_id',requirement_lineage,'revision',requirement_revision,
      'compiler_ref',requirement_value->>'requirement_ref',
      'requirement_kind',requirement_value->>'requirement_kind',
      'requiredness',requirement_value->>'requiredness',
      'compliance_policy',requirement_value->>'compliance_policy','lifecycle','current',
      'text',requirement_value->>'requirement_text','fulfillment_expr',fulfillment,
      'applicability',applicability_value,
      'compiled_applicability',requirement_value->'applicability',
      'structured_form_revision_ids',requirement_value->'structured_form_revision_ids'));
    requirement_sha:=kb_bid_v2_sha256_bytes(requirement_payload);
    INSERT INTO bid_requirement_revision_artifacts(id,project_id,lineage_id,revision,requirement_kind,
      requiredness,compliance_policy,lifecycle,text_utf8,text_sha256,fulfillment_expr,applicability,tombstone,
      canonical_payload,content_sha256,actor)
    VALUES(requirement_id,typed.project_id,requirement_lineage,requirement_revision,
      requirement_value->>'requirement_kind',requirement_value->>'requiredness',
      requirement_value->>'compliance_policy','current',requirement_text,requirement_text_sha,
      fulfillment,applicability_value,false,requirement_payload,requirement_sha,p_actor);
    FOR source_identity IN SELECT value FROM jsonb_array_elements(requirement_value->'source_unit_revision_ids') LOOP
      SELECT * INTO STRICT source_value FROM bid_source_unit_revision_artifacts
        WHERE project_id=typed.project_id AND id=(source_identity#>>'{}')::uuid;
      INSERT INTO bid_requirement_source_revision_artifacts(id,project_id,requirement_revision_id,
        source_unit_revision_id,quote_start_offset,quote_end_offset,quote_sha256)
      VALUES(gen_random_uuid(),typed.project_id,requirement_id,source_value.id,0,
        octet_length(source_value.text_utf8),source_value.text_sha256);
    END LOOP;
    requirement_items:=requirement_items||jsonb_build_array(jsonb_build_object(
      'requirement_revision_id',requirement_id,'content_sha256',requirement_sha,
      'effective_applicability',applicability_value,'ordinal',ordinal_value));
    ordinal_value:=ordinal_value+1;
  END LOOP;
  set_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',typed.project_id,
    'document_set_revision_id',typed.document_set_revision_id,
    'disposition_set_revision_id',typed.disposition_set_revision_id,'revision',set_revision,
    'compiler_version',3,'items',requirement_items));
  set_sha:=kb_bid_v2_sha256_bytes(set_payload);
  INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,
    disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256)
  SELECT set_id,typed.project_id,typed.document_set_revision_id,d.revision,
    typed.disposition_set_revision_id,s.revision,set_revision,set_payload,set_sha
    FROM bid_document_set_artifacts d,bid_source_unit_disposition_set_artifacts s
    WHERE d.id=typed.document_set_revision_id AND s.id=typed.disposition_set_revision_id;
  INSERT INTO bid_requirement_set_items(requirement_set_id,project_id,requirement_revision_id,effective_applicability,ordinal)
  SELECT set_id,typed.project_id,(value->>'requirement_revision_id')::uuid,value->'effective_applicability',
    (value->>'ordinal')::integer FROM jsonb_array_elements(requirement_items);
  publication_status:=kb_bid_v2_publish_requirement_set(set_id,set_sha);
  IF publication_status='obsolete' THEN
    result_value:=jsonb_build_object('requirement_set_id',set_id,'requirement_set_sha256',set_sha,
      'requirement_count',ordinal_value,'status','obsolete','compiler_version',3,'replayed',false);
    result_sha:=kb_bid_v2_sha256_bytes(convert_to(result_value::text,'UTF8'));
    INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
    VALUES(p_request_artifact_id,'requirement_compile',p_frozen_input_sha256,result_value,result_sha);
    UPDATE bid_async_request_snapshot_artifacts SET status='obsolete',result_identity=result_value,
      finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
    RETURN result_value;
  END IF;
  SELECT * INTO STRICT workspace_value FROM bid_submission_workspaces WHERE project_id=typed.project_id;
  SELECT * INTO STRICT projection_head FROM bid_workspace_requirement_projection_current
    WHERE scope_id=workspace_value.id FOR UPDATE;
  projection_revision:=projection_head.generation+1;
  projection_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
    'workspace_id',workspace_value.id,'requirement_set_id',set_id,
    'revision',projection_revision,'compiler_version',3,'items',requirement_items));
  projection_sha:=kb_bid_v2_sha256_bytes(projection_payload);
  INSERT INTO bid_workspace_requirement_projection_artifacts(id,project_id,workspace_id,requirement_set_id,
    revision,canonical_payload,content_sha256)
  VALUES(projection_id,typed.project_id,workspace_value.id,set_id,projection_revision,projection_payload,projection_sha);
  INSERT INTO bid_workspace_requirement_projection_items(projection_id,project_id,requirement_revision_id,effective_applicability,ordinal)
  SELECT projection_id,typed.project_id,(value->>'requirement_revision_id')::uuid,value->'effective_applicability',
    (value->>'ordinal')::integer FROM jsonb_array_elements(requirement_items);
  IF NOT kb_bid_v2_advance_requirement_projection(typed.project_id,workspace_value.id,
      projection_head.artifact_id,projection_head.artifact_sha256,projection_id,projection_sha) THEN
    RAISE EXCEPTION 'REQUIREMENT_PROJECTION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  PERFORM kb_bid_v2_advance_workspace_projection(workspace_value.id,
    projection_head.artifact_id,projection_head.artifact_sha256,projection_id,projection_sha);
  result_value:=jsonb_build_object('requirement_set_id',set_id,'requirement_set_sha256',set_sha,
    'requirement_count',ordinal_value,'requirement_projection_id',projection_id,
    'requirement_projection_sha256',projection_sha,'compiler_version',3,'replayed',false);
  result_sha:=kb_bid_v2_sha256_bytes(convert_to(result_value::text,'UTF8'));
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(p_request_artifact_id,'requirement_compile',p_frozen_input_sha256,result_value,result_sha);
  UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=result_value,
    finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
  RETURN result_value;
END $$;

CREATE OR REPLACE FUNCTION kb_bid_v2_create_outline_candidate(
  p_workspace_id uuid,p_expected_revision_id uuid,p_expected_sha256 kb_sha256,
  p_document_set_id uuid,p_document_set_sha256 kb_sha256,p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  workspace_rev bid_workspace_revision_artifacts%ROWTYPE;
  projection bid_workspace_requirement_projection_artifacts%ROWTYPE;
  requirement_set bid_requirement_set_artifacts%ROWTYPE;
  disposition bid_source_unit_disposition_set_artifacts%ROWTYPE;
  scope_rev bid_workspace_scope_revision_artifacts%ROWTYPE;
  request_id uuid:=gen_random_uuid(); frozen_payload bytea; frozen_sha kb_sha256;
  response jsonb; response_bytes bytea; replay bytea; form_identities jsonb;
  prompt_sha kb_sha256; template_sha kb_sha256; model_sha kb_sha256; agent_sha kb_sha256;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.outline.generate',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id FOR UPDATE;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR SHARE;
  IF head.artifact_id IS DISTINCT FROM p_expected_revision_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT workspace_rev FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id;
  SELECT * INTO STRICT projection FROM bid_workspace_requirement_projection_artifacts
    WHERE id=workspace_rev.requirement_projection_id AND content_sha256=workspace_rev.requirement_projection_sha256;
  SELECT * INTO STRICT requirement_set FROM bid_requirement_set_artifacts WHERE id=projection.requirement_set_id;
  SELECT * INTO STRICT disposition FROM bid_source_unit_disposition_set_artifacts WHERE id=requirement_set.disposition_set_id;
  SELECT * INTO STRICT scope_rev FROM bid_workspace_scope_revision_artifacts WHERE id=workspace_rev.scope_revision_id;
  IF requirement_set.document_set_id IS DISTINCT FROM p_document_set_id OR
     (SELECT content_sha256 FROM bid_document_set_artifacts WHERE id=p_document_set_id) IS DISTINCT FROM p_document_set_sha256 THEN
    RAISE EXCEPTION 'DOCUMENT_SET_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT content_sha256 INTO STRICT prompt_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000101';
  SELECT content_sha256 INTO STRICT template_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000102';
  SELECT content_sha256 INTO STRICT model_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000103';
  SELECT content_sha256 INTO STRICT agent_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000107';
  SELECT coalesce(jsonb_agg(jsonb_build_object('form_definition_revision_id',form.id,
      'form_definition_sha256',form.content_sha256,'source_unit_revision_id',form.source_unit_revision_id)
      ORDER BY form.source_unit_revision_id,form.id),'[]'::jsonb) INTO form_identities
    FROM bid_tender_structured_form_definition_artifacts form
    JOIN bid_source_unit_disposition_set_items disposition_item
      ON disposition_item.project_id=form.project_id AND disposition_item.source_unit_revision_id=form.source_unit_revision_id
    JOIN bid_source_unit_revision_artifacts source ON source.project_id=form.project_id AND source.id=form.source_unit_revision_id
    JOIN bid_document_set_items document_item ON document_item.project_id=source.project_id
      AND document_item.document_set_id=p_document_set_id AND document_item.document_id=source.document_id
    WHERE disposition_item.disposition_set_id=disposition.id;
  frozen_payload:=kb_bid_v2_json_payload(jsonb_build_object(
    'workspace_revision_id',workspace_rev.id,'workspace_sha256',workspace_rev.content_sha256,
    'document_set_revision_id',p_document_set_id,'document_set_sha256',p_document_set_sha256,
    'requirement_projection_revision_id',projection.id,'requirement_projection_sha256',projection.content_sha256,
    'scope_revision_id',scope_rev.id,'scope_revision_sha256',scope_rev.content_sha256,
    'prompt_contract_sha256',prompt_sha,'template_contract_sha256',template_sha,
    'model_contract_sha256',model_sha,'agent_contract_sha256',agent_sha,
    'structured_form_identities',form_identities));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  SELECT jsonb_build_object(
      'request_artifact_id',request_value.id,'kind','OutlineGenerate','status',request_value.status,
      'result_identity',request_value.result_identity,'error_code',request_value.error_code,
      'request_revision',identity_value.request_revision,'request_sha256',identity_value.request_sha256,
      'frozen_input_sha256',identity_value.frozen_input_sha256,'project_id',identity_value.project_id,
      'workspace_id',identity_value.workspace_id,'base_workspace_revision_id',identity_value.base_workspace_revision_id)
    INTO response
    FROM bid_async_request_snapshot_artifacts request_value
    JOIN bid_outline_generation_request_identities identity_value
      ON identity_value.request_artifact_id=request_value.id
    WHERE request_value.workspace_id=p_workspace_id
      AND request_value.request_kind='outline_generate'
      AND request_value.frozen_input_sha256=frozen_sha
      AND request_value.status='pending'
    ORDER BY request_value.created_at DESC
    LIMIT 1;
  IF response IS NOT NULL THEN
    response_bytes:=convert_to(response::text,'UTF8');
    PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.outline.generate',p_idempotency_key,202,response_bytes);
    RETURN response;
  END IF;
  INSERT INTO bid_async_request_snapshot_artifacts(
    id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(request_id,workspace.project_id,p_workspace_id,'outline_generate',1,frozen_sha,p_request_bytes,p_request_sha256,'pending');
  INSERT INTO bid_outline_generation_request_identities(
    request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,
    base_workspace_revision_id,base_workspace_sha256,document_set_revision_id,document_set_sha256,
    disposition_set_revision_id,disposition_set_sha256,requirement_set_revision_id,requirement_set_sha256,
    requirement_projection_id,requirement_projection_sha256,scope_revision_id,scope_revision_sha256,
    prompt_contract_id,prompt_contract_sha256,template_contract_id,template_contract_sha256,
    model_contract_id,model_contract_sha256,agent_contract_id,agent_contract_sha256,structured_form_identities)
  VALUES(request_id,workspace.project_id,p_workspace_id,1,p_request_sha256,frozen_sha,
    workspace_rev.id,workspace_rev.content_sha256,p_document_set_id,p_document_set_sha256,
    disposition.id,disposition.content_sha256,requirement_set.id,requirement_set.content_sha256,
    projection.id,projection.content_sha256,scope_rev.id,scope_rev.content_sha256,
    '00000000-0000-5000-8000-000000000101',prompt_sha,
    '00000000-0000-5000-8000-000000000102',template_sha,
    '00000000-0000-5000-8000-000000000103',model_sha,
    '00000000-0000-5000-8000-000000000107',agent_sha,form_identities);
  response:=jsonb_build_object('request_artifact_id',request_id,'kind','OutlineGenerate','status','pending',
    'result_identity',NULL,'error_code',NULL,'request_revision',1,'request_sha256',p_request_sha256,
    'frozen_input_sha256',frozen_sha,'project_id',workspace.project_id,'workspace_id',p_workspace_id,
    'base_workspace_revision_id',workspace_rev.id);
  response_bytes:=convert_to(response::text,'UTF8');
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.outline.generate',p_idempotency_key,202,response_bytes);
  RETURN response;
END $$;

CREATE OR REPLACE FUNCTION kb_bid_v2_outline_semantics_valid(
  p_candidate jsonb,p_nodes jsonb,p_reduce jsonb,p_projection_id uuid
) RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF jsonb_typeof(p_reduce)<>'object' OR p_reduce->'schema_version' IS DISTINCT FROM '2'::jsonb
    OR jsonb_typeof(p_reduce->'composition_spine')<>'object'
    OR jsonb_typeof(p_reduce#>'{composition_spine,sections}')<>'array'
    OR jsonb_array_length(p_reduce#>'{composition_spine,sections}')<2
    OR jsonb_typeof(p_reduce->'section_obligation_matrix')<>'object'
    OR jsonb_typeof(p_reduce#>'{section_obligation_matrix,sections}')<>'array'
    OR jsonb_typeof(p_candidate->'section_obligation_bindings')<>'array'
    OR jsonb_array_length(p_candidate->'section_obligation_bindings')>100000 THEN RETURN false; END IF;
  IF NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p_nodes) node
      WHERE node->>'client_node_ref'='root'
        AND jsonb_typeof(node->'parent_client_node_ref')='null'
        AND node->>'semantic_role'='cover' AND node->>'render_role'='front_matter'
        AND node->>'title'=p_reduce#>>'{composition_spine,root_title}') THEN RETURN false; END IF;
  IF (SELECT count(*) FROM jsonb_array_elements(p_nodes) node
      WHERE node->>'parent_client_node_ref'='root')
      <> jsonb_array_length(p_reduce#>'{composition_spine,sections}')+1 THEN RETURN false; END IF;
  IF NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p_nodes) node
      WHERE node->>'client_node_ref'='toc' AND node->>'parent_client_node_ref'='root'
        AND (node->>'ordinal')::integer=0 AND node->>'semantic_role'='toc'
        AND node->>'render_role'='toc') THEN RETURN false; END IF;
  IF EXISTS (WITH sections AS (
      SELECT section,ordinality FROM jsonb_array_elements(p_reduce#>'{composition_spine,sections}')
        WITH ORDINALITY value(section,ordinality))
    SELECT 1 FROM sections WHERE NOT EXISTS (
      SELECT 1 FROM jsonb_array_elements(p_nodes) node
      WHERE node->>'client_node_ref'='spine_'||left(section->>'section_ref',24)
        AND node->>'parent_client_node_ref'='root'
        AND (node->>'ordinal')::bigint=ordinality
        AND node->>'title'=section->>'title'
        AND node->>'semantic_role'=section->>'semantic_role'
        AND node->>'render_role'='section'
        AND EXISTS (SELECT 1 FROM jsonb_array_elements_text(node->'origin_source_unit_revision_ids') node_source
          JOIN jsonb_array_elements_text(section->'source_unit_revision_ids') section_source
            ON section_source=node_source))) THEN RETURN false; END IF;
  IF EXISTS (WITH RECURSIVE node_rows AS (
      SELECT node->>'client_node_ref' ref,node->>'parent_client_node_ref' parent
      FROM jsonb_array_elements(p_nodes) node),
    lineage(ref,ancestor) AS (
      SELECT ref,parent FROM node_rows WHERE parent IS NOT NULL
      UNION ALL SELECT lineage.ref,node_rows.parent FROM lineage
        JOIN node_rows ON node_rows.ref=lineage.ancestor WHERE node_rows.parent IS NOT NULL),
    sections AS (SELECT section->>'section_ref' section_ref
      FROM jsonb_array_elements(p_reduce#>'{composition_spine,sections}') section)
    SELECT 1 FROM sections WHERE NOT EXISTS (SELECT 1 FROM lineage
      WHERE ancestor='spine_'||left(section_ref,24))) THEN RETURN false; END IF;
  IF EXISTS (SELECT 1 FROM jsonb_array_elements(p_reduce->'structure_fragments') fragment,
      jsonb_array_elements(p_nodes) node
      WHERE fragment->>'outline_usage' IN ('requirement_context','reference_only')
        AND node->>'title'=fragment->>'title'
        AND EXISTS (SELECT 1 FROM jsonb_array_elements_text(node->'origin_source_unit_revision_ids') node_source
          JOIN jsonb_array_elements_text(fragment->'source_unit_revision_ids') fragment_source
            ON fragment_source=node_source)) THEN RETURN false; END IF;
  IF EXISTS (SELECT 1 FROM jsonb_array_elements(p_reduce->'structure_fragments') fragment,
      jsonb_array_elements(p_nodes) node
      WHERE btrim(coalesce(fragment->>'source_numbering',''))<>''
        AND left(ltrim(node->>'title'),char_length(fragment->>'source_numbering'))=fragment->>'source_numbering'
        AND substring(ltrim(node->>'title') FROM char_length(fragment->>'source_numbering')+1 FOR 1)
          ~ '^[[:space:]、.．:：]$') THEN RETURN false; END IF;
  IF EXISTS (SELECT 1 FROM jsonb_array_elements(p_candidate->'section_obligation_bindings') binding
      WHERE jsonb_typeof(binding)<>'object'
        OR NOT kb_bid_v2_json_keys_exact(binding,ARRAY['obligation_id','target_client_node_ref'])
        OR coalesce(binding->>'obligation_id','')!~'^[a-f0-9]{64}$'
        OR coalesce(binding->>'target_client_node_ref','')!~'^[A-Za-z0-9_-]{1,128}$'
        OR NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p_nodes) node
          WHERE node->>'client_node_ref'=binding->>'target_client_node_ref')) THEN RETURN false; END IF;
  IF EXISTS (SELECT 1 FROM jsonb_array_elements(p_candidate->'section_obligation_bindings') binding
      GROUP BY binding->>'obligation_id' HAVING count(*)<>1) THEN RETURN false; END IF;
  IF EXISTS (WITH matrix AS (
      SELECT section->>'section_ref' section_ref,section
      FROM jsonb_array_elements(p_reduce#>'{section_obligation_matrix,sections}') section),
    obligations AS (
      SELECT section_ref,obligation,'required' bucket FROM matrix
        CROSS JOIN LATERAL jsonb_array_elements(section->'required_children') obligation
      UNION ALL SELECT section_ref,obligation,'conditional' FROM matrix
        CROSS JOIN LATERAL jsonb_array_elements(section->'conditional_children') obligation
      UNION ALL SELECT section_ref,obligation,'excluded' FROM matrix
        CROSS JOIN LATERAL jsonb_array_elements(section->'excluded_children') obligation)
    SELECT 1 FROM jsonb_array_elements(p_candidate->'section_obligation_bindings') binding
      LEFT JOIN obligations ON obligations.obligation->>'obligation_id'=binding->>'obligation_id'
      WHERE obligations.obligation IS NULL OR obligations.bucket='excluded') THEN RETURN false; END IF;
  IF EXISTS (WITH matrix AS (SELECT section FROM jsonb_array_elements(p_reduce#>'{section_obligation_matrix,sections}') section),
    required AS (SELECT obligation->>'obligation_id' obligation_id FROM matrix
      CROSS JOIN LATERAL jsonb_array_elements(section->'required_children') obligation)
    SELECT 1 FROM required WHERE (SELECT count(*) FROM jsonb_array_elements(p_candidate->'section_obligation_bindings') binding
      WHERE binding->>'obligation_id'=required.obligation_id)<>1) THEN RETURN false; END IF;
  IF EXISTS (WITH RECURSIVE node_rows AS (
      SELECT node->>'client_node_ref' ref,node->>'parent_client_node_ref' parent,node
      FROM jsonb_array_elements(p_nodes) node),
    lineage(ref,ancestor) AS (
      SELECT ref,parent FROM node_rows WHERE parent IS NOT NULL
      UNION ALL SELECT lineage.ref,node_rows.parent FROM lineage
        JOIN node_rows ON node_rows.ref=lineage.ancestor WHERE node_rows.parent IS NOT NULL),
    matrix AS (SELECT section->>'section_ref' section_ref,section
      FROM jsonb_array_elements(p_reduce#>'{section_obligation_matrix,sections}') section),
    obligations AS (
      SELECT section_ref,obligation FROM matrix
        CROSS JOIN LATERAL jsonb_array_elements(section->'required_children') obligation
      UNION ALL SELECT section_ref,obligation FROM matrix
        CROSS JOIN LATERAL jsonb_array_elements(section->'conditional_children') obligation)
    SELECT 1 FROM jsonb_array_elements(p_candidate->'section_obligation_bindings') binding
      JOIN obligations ON obligations.obligation->>'obligation_id'=binding->>'obligation_id'
      JOIN node_rows target ON target.ref=binding->>'target_client_node_ref'
      WHERE target.ref='spine_'||left(obligations.section_ref,24)
        OR NOT EXISTS (SELECT 1 FROM lineage WHERE lineage.ref=target.ref
          AND lineage.ancestor='spine_'||left(obligations.section_ref,24))
        OR NOT EXISTS (SELECT 1 FROM jsonb_array_elements_text(target.node->'origin_source_unit_revision_ids') node_source
          JOIN jsonb_array_elements_text(obligations.obligation->'source_unit_revision_ids') obligation_source
            ON obligation_source=node_source)) THEN RETURN false; END IF;
  IF EXISTS (WITH matrix AS (SELECT section FROM jsonb_array_elements(p_reduce#>'{section_obligation_matrix,sections}') section),
    excluded AS (SELECT obligation FROM matrix
      CROSS JOIN LATERAL jsonb_array_elements(section->'excluded_children') obligation)
    SELECT 1 FROM excluded,jsonb_array_elements(p_nodes) node
      WHERE node->>'title'=excluded.obligation->>'title'
        AND EXISTS (SELECT 1 FROM jsonb_array_elements_text(node->'origin_source_unit_revision_ids') node_source
          JOIN jsonb_array_elements_text(excluded.obligation->'source_unit_revision_ids') excluded_source
            ON excluded_source=node_source)) THEN RETURN false; END IF;
  IF (WITH matrix AS (SELECT section FROM jsonb_array_elements(p_reduce#>'{section_obligation_matrix,sections}') section),
      excluded AS (SELECT obligation FROM matrix CROSS JOIN LATERAL jsonb_array_elements(section->'excluded_children') obligation)
      SELECT count(*) FROM excluded) >
     (SELECT count(*) FROM jsonb_array_elements(p_candidate->'notices') notice
      WHERE notice->>'code'='EXCLUDED_NOT_APPLICABLE') THEN RETURN false; END IF;
  IF EXISTS (WITH expected AS (
      SELECT (need->>'need_occurrence_id')::uuid need_id,need->>'channel' channel
      FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.id=item.requirement_revision_id
      CROSS JOIN LATERAL jsonb_array_elements(kb_bid_v2_fulfillment_needs(requirement.fulfillment_expr)) need
      WHERE item.projection_id=p_projection_id AND requirement.requiredness='mandatory'),
    matrix AS (SELECT section FROM jsonb_array_elements(p_reduce#>'{section_obligation_matrix,sections}') section),
    required AS (SELECT obligation FROM matrix CROSS JOIN LATERAL jsonb_array_elements(section->'required_children') obligation)
    SELECT 1 FROM expected WHERE NOT EXISTS (
      SELECT 1 FROM jsonb_array_elements(p_candidate->'bindings') route
      JOIN required ON EXISTS (SELECT 1 FROM jsonb_array_elements_text(required.obligation->'need_occurrence_ids') need_id
        WHERE need_id=expected.need_id::text)
      JOIN jsonb_array_elements(p_candidate->'section_obligation_bindings') obligation_binding
        ON obligation_binding->>'obligation_id'=required.obligation->>'obligation_id'
        AND obligation_binding->>'target_client_node_ref'=route->>'target_client_node_ref'
      WHERE route->>'need_occurrence_id'=expected.need_id::text
        AND route->>'channel'=expected.channel
        AND EXISTS (SELECT 1 FROM jsonb_array_elements_text(required.obligation->'allowed_channels') allowed
          WHERE allowed=expected.channel))) THEN RETURN false; END IF;
  RETURN true;
END $$;

CREATE OR REPLACE FUNCTION kb_bid_v2_publish_outline_generation(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_candidate_id uuid,p_candidate_payload bytea,p_candidate_sha256 kb_sha256,p_nodes jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_outline_generation_request_identities%ROWTYPE; request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  candidate_json jsonb; reduce_value jsonb; node_value jsonb; ordinal_value integer:=0; published_identity jsonb;
BEGIN
  SELECT * INTO STRICT typed FROM bid_outline_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256 FOR UPDATE;
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.status='succeeded' THEN RETURN request_value.result_identity; END IF;
  IF request_value.status<>'pending' OR p_candidate_id IS NULL OR p_candidate_payload IS NULL
     OR p_candidate_sha256 IS NULL OR p_candidate_sha256<>kb_bid_v2_sha256_bytes(p_candidate_payload)
     OR jsonb_typeof(p_nodes)<>'array' OR jsonb_array_length(p_nodes) NOT BETWEEN 1 AND 1000 THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID' USING ERRCODE='23514';
  END IF;
  candidate_json:=convert_from(p_candidate_payload,'UTF8')::jsonb;
  SELECT convert_from(artifact.canonical_payload,'UTF8')::jsonb INTO reduce_value
    FROM bid_outline_reduce_plan_artifacts artifact
    WHERE artifact.request_artifact_id=p_request_artifact_id
      AND artifact.frozen_input_sha256=p_frozen_input_sha256
    ORDER BY artifact.created_at DESC,artifact.id DESC LIMIT 1;
  IF reduce_value IS NULL
     OR NOT kb_bid_v2_json_keys_exact(candidate_json,ARRAY['schema_version','nodes','bindings','section_obligation_bindings','notices'])
     OR candidate_json->'schema_version' IS DISTINCT FROM '2'::jsonb
     OR candidate_json->'nodes' IS DISTINCT FROM p_nodes
     OR NOT kb_bid_v2_outline_tree_valid(p_nodes)
     OR NOT kb_bid_v2_outline_sources_valid(p_nodes,typed.disposition_set_revision_id)
     OR NOT kb_bid_v2_outline_requirement_closure_valid(candidate_json,p_nodes,typed.requirement_projection_id)
     OR NOT kb_bid_v2_outline_semantics_valid(candidate_json,p_nodes,reduce_value,typed.requirement_projection_id)
  THEN RAISE EXCEPTION 'AGENT_OUTPUT_INVALID' USING ERRCODE='23514'; END IF;
  INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,
    base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,
    request_operation,state,canonical_payload,content_sha256)
  VALUES(p_candidate_id,typed.project_id,typed.workspace_id,'outline',typed.base_workspace_revision_id,
    typed.base_workspace_sha256,p_request_artifact_id,'outline_generate',typed.request_revision,typed.request_sha256,
    'outline_generate','proposed',p_candidate_payload,p_candidate_sha256);
  FOR node_value IN SELECT value FROM jsonb_array_elements(p_nodes) LOOP
    INSERT INTO bid_candidate_operations(candidate_id,ordinal,operation,operation_sha256)
    VALUES(p_candidate_id,ordinal_value,node_value,kb_bid_v2_sha256_bytes(convert_to(node_value::text,'UTF8')));
    ordinal_value:=ordinal_value+1;
  END LOOP;
  UPDATE bid_candidate_artifacts SET state='obsolete',decided_at=clock_timestamp()
    WHERE workspace_id=typed.workspace_id AND candidate_kind='outline'
      AND state='proposed' AND id<>p_candidate_id;
  published_identity:=jsonb_build_object('artifact_id',p_candidate_id,'sha256',p_candidate_sha256);
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(p_request_artifact_id,'agent_generate',p_frozen_input_sha256,published_identity,p_candidate_sha256);
  UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=published_identity,
    finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
  RETURN published_identity;
END $$;

CREATE OR REPLACE FUNCTION kb_bid_v2_outline_reduce_put(
  p_request_artifact_id uuid,p_frozen_input_sha256 kb_sha256,
  p_map_evidence_set_sha256 kb_sha256,p_reduce_contract_sha256 kb_sha256,p_payload jsonb
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE payload bytea:=convert_to(p_payload::text,'UTF8');
BEGIN
  IF jsonb_typeof(p_payload)<>'object'
    OR NOT kb_bid_v2_json_keys_exact(p_payload,ARRAY['schema_version','coverage','composition_spine',
      'section_obligation_matrix','structure_fragments','priority_reads','requirement_routes',
      'unresolved_conflicts','vision_requests','notices'])
    OR p_payload->'schema_version' IS DISTINCT FROM '2'::jsonb
    OR jsonb_typeof(p_payload->'coverage')<>'object'
    OR jsonb_typeof(p_payload->'composition_spine')<>'object'
    OR jsonb_typeof(p_payload->'section_obligation_matrix')<>'object'
    OR jsonb_typeof(p_payload->'structure_fragments')<>'array'
    OR jsonb_typeof(p_payload->'priority_reads')<>'array'
    OR jsonb_typeof(p_payload->'requirement_routes')<>'array'
    OR jsonb_typeof(p_payload->'unresolved_conflicts')<>'array'
    OR jsonb_typeof(p_payload->'vision_requests')<>'array'
    OR jsonb_typeof(p_payload->'notices')<>'array' THEN
    RAISE EXCEPTION 'invalid outline Reduce plan' USING ERRCODE='23514';
  END IF;
  INSERT INTO bid_outline_reduce_plan_artifacts(
    request_artifact_id,frozen_input_sha256,map_evidence_set_sha256,reduce_contract_sha256,
    canonical_payload,content_sha256)
  VALUES(p_request_artifact_id,p_frozen_input_sha256,p_map_evidence_set_sha256,p_reduce_contract_sha256,
    payload,kb_bid_v2_sha256_bytes(payload))
  ON CONFLICT (request_artifact_id,frozen_input_sha256,map_evidence_set_sha256,reduce_contract_sha256)
  DO NOTHING;
  IF NOT EXISTS (SELECT 1 FROM bid_outline_reduce_plan_artifacts
      WHERE request_artifact_id=p_request_artifact_id AND frozen_input_sha256=p_frozen_input_sha256
        AND map_evidence_set_sha256=p_map_evidence_set_sha256
        AND reduce_contract_sha256=p_reduce_contract_sha256
        AND content_sha256=kb_bid_v2_sha256_bytes(payload)) THEN
    RAISE EXCEPTION 'divergent outline Reduce replay' USING ERRCODE='23514';
  END IF;
END $$;

CREATE OR REPLACE FUNCTION kb_bid_v2_outline_synthesis_packet_append(
  p_request_artifact_id uuid,p_frozen_input_sha256 kb_sha256,
  p_reduce_plan_sha256 kb_sha256,p_map_evidence_set_sha256 kb_sha256,p_payload jsonb
) RETURNS kb_sha256 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE payload bytea:=convert_to(p_payload::text,'UTF8'); sha kb_sha256:=kb_bid_v2_sha256_bytes(payload);
BEGIN
  IF jsonb_typeof(p_payload)<>'object'
    OR NOT kb_bid_v2_json_keys_exact(p_payload,ARRAY['schema_version','request_artifact_id','frozen_input_sha256',
      'reduce_plan_sha256','map_evidence_set_sha256','composition_spine','section_obligation_matrix',
      'deterministic_spine_nodes','manifest','structure_fragments','priority_reads','requirement_routes',
      'conflicts','notices','selected_evidence','selected_facts','draft'])
    OR p_payload->'schema_version' IS DISTINCT FROM '2'::jsonb
    OR (p_payload->>'request_artifact_id')::uuid<>p_request_artifact_id
    OR p_payload->>'frozen_input_sha256'<>p_frozen_input_sha256::text
    OR p_payload->>'reduce_plan_sha256'<>p_reduce_plan_sha256::text
    OR p_payload->>'map_evidence_set_sha256'<>p_map_evidence_set_sha256::text
    OR jsonb_typeof(p_payload->'composition_spine')<>'object'
    OR jsonb_typeof(p_payload->'section_obligation_matrix')<>'object'
    OR jsonb_typeof(p_payload->'deterministic_spine_nodes')<>'array'
    OR jsonb_typeof(p_payload->'manifest')<>'object'
    OR jsonb_typeof(p_payload->'structure_fragments')<>'array'
    OR jsonb_typeof(p_payload->'priority_reads')<>'array'
    OR jsonb_typeof(p_payload->'requirement_routes')<>'array'
    OR jsonb_typeof(p_payload->'conflicts')<>'array'
    OR jsonb_typeof(p_payload->'notices')<>'array'
    OR jsonb_typeof(p_payload->'selected_evidence')<>'array'
    OR jsonb_typeof(p_payload->'selected_facts')<>'array'
    OR jsonb_typeof(p_payload->'draft')<>'object' THEN
    RAISE EXCEPTION 'invalid outline synthesis packet' USING ERRCODE='23514';
  END IF;
  INSERT INTO bid_outline_synthesis_packet_artifacts(
    request_artifact_id,frozen_input_sha256,reduce_plan_sha256,map_evidence_set_sha256,
    canonical_payload,content_sha256)
  VALUES(p_request_artifact_id,p_frozen_input_sha256,p_reduce_plan_sha256,p_map_evidence_set_sha256,payload,sha)
  ON CONFLICT (request_artifact_id,content_sha256) DO NOTHING;
  RETURN sha;
END $$;

CREATE OR REPLACE FUNCTION kb_bid_v2_outline_checkpoint_append(
  p_request_artifact_id uuid,p_frozen_input_sha256 kb_sha256,p_attempt integer,
  p_checkpoint_ordinal integer,p_phase text,p_payload jsonb
) RETURNS kb_sha256 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE payload bytea:=convert_to(p_payload::text,'UTF8'); sha kb_sha256:=kb_bid_v2_sha256_bytes(payload);
  current_attempt integer;
BEGIN
  SELECT attempt INTO current_attempt FROM bid_outline_agent_run_artifacts
    WHERE request_artifact_id=p_request_artifact_id AND frozen_input_sha256=p_frozen_input_sha256
      AND status='running' FOR UPDATE;
  IF current_attempt IS NULL THEN RAISE EXCEPTION 'FROZEN_INPUT_MISSING' USING ERRCODE='P0002'; END IF;
  IF current_attempt<>p_attempt THEN RAISE EXCEPTION 'REQUEST_ATTEMPT_SUPERSEDED' USING ERRCODE='40001'; END IF;
  IF jsonb_typeof(p_payload)<>'object'
    OR NOT kb_bid_v2_json_keys_exact(p_payload,ARRAY['schema_version','attempt','phase','reduce_plan_sha256',
      'selected_evidence','selected_facts','accepted_node_chunks','accepted_routes','accepted_obligation_bindings',
      'unresolved_need_occurrence_ids','total_turns','total_tool_calls','text_bytes_read','images_read'])
    OR p_payload->'schema_version' IS DISTINCT FROM '2'::jsonb
    OR (p_payload->>'attempt')::integer<>p_attempt OR p_payload->>'phase'<>p_phase
    OR NOT kb_bid_v2_sha256_text(p_payload->>'reduce_plan_sha256')
    OR jsonb_typeof(p_payload->'selected_evidence')<>'array'
    OR jsonb_typeof(p_payload->'selected_facts')<>'array'
    OR jsonb_typeof(p_payload->'accepted_node_chunks')<>'array'
    OR jsonb_typeof(p_payload->'accepted_routes')<>'array'
    OR jsonb_typeof(p_payload->'accepted_obligation_bindings')<>'array'
    OR jsonb_typeof(p_payload->'unresolved_need_occurrence_ids')<>'array'
    OR coalesce(p_payload->>'total_turns','')!~'^(0|[1-9][0-9]*)$'
    OR coalesce(p_payload->>'total_tool_calls','')!~'^(0|[1-9][0-9]*)$'
    OR coalesce(p_payload->>'text_bytes_read','')!~'^(0|[1-9][0-9]*)$'
    OR coalesce(p_payload->>'images_read','')!~'^(0|[1-9][0-9]*)$' THEN
    RAISE EXCEPTION 'invalid outline checkpoint' USING ERRCODE='23514';
  END IF;
  INSERT INTO bid_outline_agent_checkpoint_artifacts(
    request_artifact_id,frozen_input_sha256,attempt,checkpoint_ordinal,phase,canonical_payload,content_sha256)
  VALUES(p_request_artifact_id,p_frozen_input_sha256,p_attempt,p_checkpoint_ordinal,p_phase,payload,sha)
  ON CONFLICT (request_artifact_id,checkpoint_ordinal) DO NOTHING;
  IF NOT EXISTS (SELECT 1 FROM bid_outline_agent_checkpoint_artifacts
      WHERE request_artifact_id=p_request_artifact_id AND checkpoint_ordinal=p_checkpoint_ordinal
        AND frozen_input_sha256=p_frozen_input_sha256 AND content_sha256=sha) THEN
    RAISE EXCEPTION 'divergent outline checkpoint replay' USING ERRCODE='23514';
  END IF;
  UPDATE bid_outline_agent_run_artifacts SET checkpoint_sha256=sha,updated_at=clock_timestamp()
    WHERE request_artifact_id=p_request_artifact_id;
  RETURN sha;
END $$;

CREATE OR REPLACE FUNCTION kb_bid_v2_outline_checkpoint_latest(
  p_request_artifact_id uuid,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT convert_from(canonical_payload,'UTF8')::jsonb
  FROM bid_outline_agent_checkpoint_artifacts
  WHERE request_artifact_id=p_request_artifact_id AND frozen_input_sha256=p_frozen_input_sha256
  ORDER BY created_at DESC,checkpoint_ordinal DESC LIMIT 1
$$;

INSERT INTO bid_authoring_contract_artifacts(
  id,contract_kind,schema_version,canonical_payload,content_sha256)
SELECT '00000000-0000-5000-8000-000000000107'::uuid,'agent',1,payload,
  kb_bid_v2_sha256_bytes(payload)
FROM (VALUES (convert_to(
  '{"kind":"outline_agent","version":7,"map_schema":3,"reduce_schema":2,"output_schema":2,"progress_control":"completion_and_stall","max_stalled_turns":2}',
  'UTF8'))) value(payload)
ON CONFLICT (id) DO NOTHING;

REVOKE ALL ON FUNCTION
  kb_bid_v2_load_requirement_set_compile_input_v3(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_requirement_set_v3(uuid,bigint,kb_sha256,jsonb,kb_actor_identity),
  kb_bid_v2_outline_semantics_valid(jsonb,jsonb,jsonb,uuid)
FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  kb_bid_v2_load_requirement_set_compile_input_v3(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_requirement_set_v3(uuid,bigint,kb_sha256,jsonb,kb_actor_identity)
TO kb_runtime_worker;

COMMIT;
