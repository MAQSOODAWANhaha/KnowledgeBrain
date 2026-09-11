\set ON_ERROR_STOP on

-- Run with psql -X --dbname="$KNOWLEDGEBRAIN_TEST_DATABASE_URL" --file=this-file
-- against an isolated test database with all three baselines installed.
-- Uses synthetic already-parsed sources, not real DocReader/OCR publication.
-- UUIDs and hashes are generated; no project, sample or converter identity is pinned.
BEGIN;
DO $$ BEGIN
  IF current_database() NOT LIKE 'knowledgebrain\_test\_%' THEN
    RAISE EXCEPTION 'collection fixture requires a dedicated knowledgebrain_test_* database';
  END IF;
END $$;
SET LOCAL ROLE kb_app_owner;

-- Test-only protocol input: no provider is contacted. The publication below is
-- deliberately needs_review with a finding, never a fabricated semantic pass.
SELECT set_config('kb_fixture.runtime',jsonb_build_object(
  'provider',jsonb_build_object('model_id','sql-boundary-fixture','max_tokens',1024),
  'limits',jsonb_build_object('max_turns',10,'max_tool_calls',100,'max_read_bytes',1000000,'max_context_bytes',1000000),
  'main_prompt_sha256',kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs('"SQL boundary fixture only"'::jsonb),'UTF8')),
  'review_prompt_sha256',kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs('"SQL boundary fixture only"'::jsonb),'UTF8')),
  'tools_sha256',kb_bid_v2_sha256_bytes(convert_to('[]','UTF8')),
  'review_tools_sha256',kb_bid_v2_sha256_bytes(convert_to('[]','UTF8')))::text,true);

CREATE FUNCTION kb_fixture_publish_collection_v4(
  p_id uuid,p_revision bigint,p_sha kb_sha256,p_compiled jsonb,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE
  runtime jsonb; frozen jsonb; claim jsonb; body bytea; checkpoint jsonb;
  analysis jsonb; review jsonb; analysis_result jsonb; disposition jsonb;
  result_value jsonb; input_sha kb_sha256; empty_coverage jsonb;
BEGIN
  IF current_database() NOT LIKE 'knowledgebrain\_test\_%' THEN
    RAISE EXCEPTION 'test fixture database required';
  END IF;
  frozen:=kb_bid_v2_load_tender_analysis_input(p_id,p_revision,p_sha);
  runtime:=frozen->'runtime'; frozen:=frozen->'input';
  claim:=kb_bid_v2_tender_agent_claim(p_id,p_revision,p_sha);
  IF claim->>'disposition'='obsolete' THEN
    -- A terminal request is a read-only receipt replay, not a second publish
    -- through an expired owner. The production claim must refuse reacquisition.
    SELECT result_identity INTO STRICT result_value FROM bid_async_request_snapshot_artifacts
      WHERE id=p_id AND revision=p_revision AND frozen_input_sha256=p_sha AND status='succeeded';
    RETURN result_value||jsonb_build_object('replayed',true);
  END IF;
  IF claim->>'disposition'<>'claimed' THEN RAISE EXCEPTION 'fixture could not claim request'; END IF;
  input_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(frozen),'UTF8'));
  SELECT jsonb_object_agg(value->>'source_unit_revision_id',jsonb_build_object(
    'state','unresolved','reason','SQL boundary fixture; no semantic classification'))
    INTO disposition FROM jsonb_array_elements(frozen->'source_units');
  empty_coverage:='{"metadata":{},"text":{},"form_cells":{},"candidate":{},"views":{},"view_failures":{}}'::jsonb;
  analysis:=jsonb_build_object('records','{}'::jsonb,'relations','{}'::jsonb,
    'dispositions',coalesce(disposition,'{}'::jsonb),'coverage',empty_coverage);
  review:=jsonb_build_object('analysis_sha256',kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(analysis),'UTF8')),
    'coverage',empty_coverage,'findings',jsonb_build_array(jsonb_build_object(
      'code','SQL_BOUNDARY_FIXTURE','message','No model or semantic review was run by this fixture',
      'affected','[]'::jsonb,'sources','[]'::jsonb)));
  analysis_result:=jsonb_build_object('schema_version',1,'frozen_input_sha256',input_sha,
    'analysis',analysis,'review',review,'quality','needs_review','source_views','{}'::jsonb);
  body:=convert_to(kb_bid_v2_jcs(jsonb_build_object('model',runtime#>>'{provider,model_id}',
    'max_tokens',runtime#>'{provider,max_tokens}','stream',true,'tool_choice','required','tools','[]'::jsonb,
    'messages',jsonb_build_array(jsonb_build_object('role','system','content','SQL boundary fixture only')))),'UTF8');
  PERFORM kb_bid_v2_tender_agent_reserve(p_id,p_sha,(claim->>'attempt')::integer,
    (claim->>'execution_owner_token')::uuid,0,'main',body);
  checkpoint:=jsonb_build_object('input_sha256',input_sha,
    'config_sha256',kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(runtime),'UTF8')),
    'turn',1,'role','main','tool_calls',0,'read_bytes',0,'done',true,
    'analysis',analysis,'review',review,'source_views','{}'::jsonb);
  PERFORM kb_bid_v2_tender_agent_checkpoint_put(p_id,p_sha,(claim->>'attempt')::integer,
    (claim->>'execution_owner_token')::uuid,checkpoint,'{"phase":"sql_boundary_fixture"}'::jsonb);
  RETURN kb_bid_v2_publish_requirement_set_v4(p_id,p_revision,p_sha,
    p_compiled||jsonb_build_object('analysis_result',analysis_result),p_actor,
    (claim->>'attempt')::integer,(claim->>'execution_owner_token')::uuid);
END $$;

DO $$
DECLARE
  runtime jsonb:=current_setting('kb_fixture.runtime')::jsonb;
  owner_id uuid := gen_random_uuid();
  project_id_value uuid := gen_random_uuid();
  actor kb_actor_identity := 'user:' || owner_id::text;
  documents uuid[] := ARRAY[]::uuid[];
  sources uuid[] := ARRAY[]::uuid[];
  units uuid[] := ARRAY[]::uuid[];
  source_hashes text[] := ARRAY[]::text[];
  snapshots jsonb := '[]';
  decision_requests jsonb := '[]';
  disposition_count_before bigint;
  source_id uuid; document_id_value uuid; unit_id uuid; lineage_id uuid;
  staging_id uuid; request_id uuid; original_bytes bytea; source_bytes bytea;
  request_bytes bytea; original_sha kb_sha256; source_sha kb_sha256;
  head bid_document_set_current%ROWTYPE;
  disposition_head bid_source_unit_disposition_set_current%ROWTYPE;
  typed bid_tender_document_process_request_identities%ROWTYPE;
  response jsonb; replay jsonb; compile_input jsonb; snapshot jsonb;
  selected uuid[]; expected_units uuid[]; actual_units uuid[];
  request_key text; expected_status text;
  disposition_items jsonb; invalid_items jsonb; expected_error text;
  initial_set_count bigint; request_count bigint; set_count bigint;
  round integer; member_count integer; ready_count integer;
  workspace_id_value uuid; workspace_before jsonb; workspace_snapshot jsonb;
  compiled jsonb; publication jsonb; latest_publication jsonb;
  requirement_count_before bigint; projection_count_before bigint;
  snapshot_index integer; source_item jsonb; compiled_items jsonb;
  latest_compiled jsonb; round_input jsonb; first_round_input jsonb; docx_receipt jsonb;
  first_docx_receipt jsonb; first_docx_metadata jsonb; first_docx_current jsonb; initial_sha kb_sha256;
  docx_staging uuid; replay_staging uuid; docx_key text; first_docx_key text;
  wrong_actor kb_actor_identity := 'user:' || gen_random_uuid()::text;
BEGIN
  INSERT INTO users(id,email) VALUES(owner_id,owner_id::text || '@example.invalid');
  request_bytes := convert_to(jsonb_build_object('title',project_id_value)::text,'UTF8');
  PERFORM kb_bid_v2_create_project(project_id_value,'collection regression',owner_id,
    actor,gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  SELECT count(*) INTO initial_set_count FROM bid_document_set_artifacts
    WHERE project_id=project_id_value;

  -- A -> A+B -> A+B+supplement, then an unavailable fourth member.
  FOR round IN 1..5 LOOP
    IF round <= 4 THEN
      document_id_value := gen_random_uuid(); staging_id := gen_random_uuid();
      request_id := gen_random_uuid();
      original_bytes := convert_to(gen_random_uuid()::text,'UTF8');
      original_sha := kb_bid_v2_sha256_bytes(original_bytes);
      request_bytes := convert_to(jsonb_build_object('file',document_id_value)::text,'UTF8');
      PERFORM kb_object_upload_stage(staging_id,'objects/' || original_sha,original_sha,
        'application/pdf',octet_length(original_bytes),actor);
      PERFORM kb_bid_v2_upload_tender_document(staging_id,document_id_value,request_id,
        project_id_value,document_id_value::text || '.pdf','application/pdf',
        octet_length(original_bytes),'objects/' || original_sha,original_sha,actor,
        gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
      documents := array_append(documents,document_id_value);
      IF round <= 3 THEN
        -- Seed the output of a successful parse. This does not run a parser.
        SELECT * INTO STRICT typed FROM bid_tender_document_process_request_identities
          WHERE request_artifact_id=request_id;
        source_id := gen_random_uuid(); lineage_id := gen_random_uuid(); unit_id := gen_random_uuid();
        source_bytes := convert_to(gen_random_uuid()::text,'UTF8');
        source_sha := kb_bid_v2_sha256_bytes(source_bytes);
        INSERT INTO bid_converted_source_artifacts(id,project_id,document_id,revision,
          source_object_ref,source_sha256,converter_contract_id,converter_contract_sha256,image_asset_set_sha256)
        VALUES(source_id,project_id_value,document_id_value,1,'objects/' || source_sha,
          source_sha,typed.converter_contract_id,typed.converter_contract_sha256,
          kb_bid_v2_sha256_bytes(convert_to('[]','UTF8')));
        INSERT INTO bid_source_unit_lineages(id,project_id,document_id)
          VALUES(lineage_id,project_id_value,document_id_value);
        INSERT INTO bid_source_unit_revision_artifacts(id,project_id,lineage_id,revision,
          document_id,source_revision_id,unit_kind,ordinal,source_locator,
          source_span_sha256,text_utf8,text_sha256,canonical_payload,content_sha256)
        VALUES(unit_id,project_id_value,lineage_id,1,document_id_value,source_id,'table_row',0,
          jsonb_build_object('schema_version',2,'project_id',project_id_value,
            'document_id',document_id_value,'converted_source_revision_id',source_id,
            'parser_unit_key',unit_id,'parser_ordinal',0,
            'source_purpose','tender_requirements_and_structure_only',
            'locator',jsonb_build_object('locator_kind','document','section_ordinal',0,
              'table_ordinal',0,'row_ordinal',0,'form_ordinal',NULL,'heading_path','')),
          source_sha,source_bytes,source_sha,source_bytes,source_sha);
        UPDATE bid_documents SET parse_status='ready' WHERE id=document_id_value;
        sources := array_append(sources,source_id); units := array_append(units,unit_id);
        source_hashes := array_append(source_hashes,source_sha::text);
      END IF;
    ELSE
      -- Model the same unavailable member moving from pending to failed.
      UPDATE bid_documents SET parse_status='failed' WHERE id=documents[4];
    END IF;
    selected := documents; member_count := cardinality(selected); ready_count := cardinality(sources);
    SELECT * INTO STRICT head FROM bid_document_set_current WHERE scope_id=project_id_value;
    request_id := gen_random_uuid(); request_key := gen_random_uuid()::text;
    request_bytes := convert_to(jsonb_build_object('documents',selected,'expected',head.artifact_id)::text,'UTF8');
    response := kb_bid_v2_freeze_document_set(project_id_value,selected,head.artifact_id,
      head.artifact_sha256,request_id,actor,request_key,request_bytes,kb_bid_v2_sha256_bytes(request_bytes),runtime);
    replay := kb_bid_v2_freeze_document_set(project_id_value,selected,head.artifact_id,
      head.artifact_sha256,request_id,actor,request_key,request_bytes,kb_bid_v2_sha256_bytes(request_bytes),runtime);
    IF replay IS DISTINCT FROM response THEN RAISE EXCEPTION 'round % replay changed identity',round; END IF;
    IF (SELECT count(*) FROM bid_document_set_items WHERE document_set_id=(response->>'artifact_id')::uuid) <> member_count
      OR EXISTS (SELECT 1 FROM unnest(selected) WITH ORDINALITY expected(id,position)
        LEFT JOIN bid_document_set_items item ON item.document_set_id=(response->>'artifact_id')::uuid
          AND item.document_id=expected.id
        WHERE item.document_id IS NULL OR item.ordinal<>expected.position-1
          OR item.source_revision_id IS DISTINCT FROM sources[expected.position::integer]) THEN
      RAISE EXCEPTION 'round % lost full membership or reused source identity',round;
    END IF;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements((SELECT convert_from(canonical_payload,'UTF8')::jsonb->'items'
        FROM bid_document_set_artifacts WHERE id=(response->>'artifact_id')::uuid)) item
      WHERE item->>'source_revision_sha256' IS DISTINCT FROM source_hashes[(item->>'ordinal')::integer+1]) THEN
      RAISE EXCEPTION 'round % changed frozen source hashes',round;
    END IF;
    SELECT array_agg(id ORDER BY id) INTO expected_units FROM unnest(units) id;
    SELECT array_agg(source_unit_revision_id ORDER BY source_unit_revision_id) INTO actual_units
      FROM bid_source_unit_disposition_set_items
      WHERE disposition_set_id=(response->>'disposition_set_artifact_id')::uuid;
    IF actual_units IS DISTINCT FROM expected_units THEN RAISE EXCEPTION 'disposition closure changed'; END IF;
    compile_input := kb_bid_v2_load_tender_analysis_input(request_id,
      (response->>'request_revision')::bigint,(response->>'frozen_input_sha256')::kb_sha256)->'input';
    SELECT array_agg((item->>'source_unit_revision_id')::uuid ORDER BY (item->>'source_unit_revision_id')::uuid)
      INTO actual_units FROM jsonb_array_elements(compile_input->'source_units') item;
    IF actual_units IS DISTINCT FROM expected_units THEN RAISE EXCEPTION 'compiler received only a delta'; END IF;
    IF jsonb_array_length(response->'warnings')<>member_count-ready_count THEN RAISE EXCEPTION 'warnings missing'; END IF;
    IF round >= 4 THEN
      expected_status := CASE WHEN round=4 THEN 'pending' ELSE 'failed' END;
      IF response->'warnings'->0->>'document_id' IS DISTINCT FROM documents[4]::text
        OR response->'warnings'->0->>'disposition' IS DISTINCT FROM expected_status
        OR (SELECT disposition FROM bid_document_set_items
          WHERE document_set_id=(response->>'artifact_id')::uuid AND document_id=documents[4]) IS DISTINCT FROM expected_status THEN
        RAISE EXCEPTION 'unavailable member not visible';
      END IF;
    END IF;
    IF (SELECT count(*) FROM bid_async_request_snapshot_artifacts
        WHERE project_id=project_id_value AND request_kind='tender_document_process')<>member_count
      OR (SELECT count(*) FROM bid_async_request_snapshot_artifacts
        WHERE project_id=project_id_value AND request_kind='requirement_set_compile')<>round
      OR (SELECT count(*) FROM bid_document_set_artifacts WHERE project_id=project_id_value)<>initial_set_count+round THEN
      RAISE EXCEPTION 'freeze/replay scheduled extra work or duplicated snapshots';
    END IF;
    snapshots := snapshots || jsonb_build_array(jsonb_build_object('response',response,'input',compile_input,
      'payload',(SELECT encode(canonical_payload,'hex') FROM bid_document_set_artifacts
        WHERE id=(response->>'artifact_id')::uuid)));
    FOR snapshot IN SELECT value FROM jsonb_array_elements(snapshots) LOOP
      IF kb_bid_v2_load_tender_analysis_input((snapshot->'response'->>'request_artifact_id')::uuid,
          (snapshot->'response'->>'request_revision')::bigint,
          (snapshot->'response'->>'frozen_input_sha256')::kb_sha256)->'input' IS DISTINCT FROM snapshot->'input'
        OR (SELECT encode(canonical_payload,'hex') FROM bid_document_set_artifacts
          WHERE id=(snapshot->'response'->>'artifact_id')::uuid) IS DISTINCT FROM snapshot->>'payload' THEN
        RAISE EXCEPTION 'new round changed old snapshot';
      END IF;
    END LOOP;
    RAISE NOTICE 'collection round %: members=%, ready=%, reuse/closure/replay/history verified',round,member_count,ready_count;
  END LOOP;

  SELECT * INTO STRICT head FROM bid_document_set_current WHERE scope_id=project_id_value;
  SELECT count(*) INTO request_count FROM bid_async_request_snapshot_artifacts WHERE project_id=project_id_value;
  SELECT count(*) INTO set_count FROM bid_document_set_artifacts WHERE project_id=project_id_value;
  BEGIN
    PERFORM kb_bid_v2_freeze_document_set(project_id_value,ARRAY[documents[1],documents[1]],
      head.artifact_id,head.artifact_sha256,gen_random_uuid(),actor,gen_random_uuid()::text,
      request_bytes,kb_bid_v2_sha256_bytes(request_bytes),runtime);
    RAISE EXCEPTION 'duplicate member accepted';
  EXCEPTION WHEN check_violation THEN
    IF SQLERRM <> 'DOCUMENT_SET_MEMBERS_INVALID' THEN RAISE; END IF;
  END;
  BEGIN
    PERFORM kb_bid_v2_freeze_document_set(project_id_value,selected,gen_random_uuid(),
      head.artifact_sha256,gen_random_uuid(),actor,gen_random_uuid()::text,
      request_bytes,kb_bid_v2_sha256_bytes(request_bytes),runtime);
    RAISE EXCEPTION 'stale CAS accepted';
  EXCEPTION WHEN serialization_failure THEN
    IF SQLERRM <> 'DOCUMENT_SET_CAS_MISMATCH' THEN RAISE; END IF;
  END;
  SELECT * INTO STRICT disposition_head FROM bid_source_unit_disposition_set_current
    WHERE scope_id=project_id_value;
  SELECT jsonb_agg(jsonb_build_object('source_unit_revision_id',id,'disposition','requirement'))
    INTO disposition_items FROM unnest(units) id;
  FOR round IN 1..3 LOOP
    invalid_items := CASE round
      WHEN 1 THEN disposition_items - 0
      WHEN 2 THEN disposition_items || jsonb_build_array(disposition_items->0)
      ELSE jsonb_set(disposition_items,'{0,source_unit_revision_id}',to_jsonb(gen_random_uuid())) END;
    expected_error := CASE WHEN round=2 THEN 'DISPOSITION_SET_ITEMS_INVALID'
      ELSE 'DISPOSITION_SET_COVERAGE_INVALID' END;
    request_bytes := convert_to(invalid_items::text,'UTF8');
    BEGIN
      PERFORM kb_bid_v2_publish_disposition_set(project_id_value,head.artifact_id,invalid_items,
        disposition_head.artifact_id,disposition_head.artifact_sha256,gen_random_uuid(),
        actor,gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
      RAISE EXCEPTION 'invalid disposition coverage accepted in case %',round;
    EXCEPTION WHEN check_violation THEN
      IF SQLERRM <> expected_error THEN RAISE; END IF;
    END;
  END LOOP;
  IF (SELECT artifact_id FROM bid_document_set_current WHERE scope_id=project_id_value)<>head.artifact_id
    OR (SELECT artifact_id FROM bid_source_unit_disposition_set_current
      WHERE scope_id=project_id_value)<>disposition_head.artifact_id
    OR (SELECT count(*) FROM bid_async_request_snapshot_artifacts WHERE project_id=project_id_value)<>request_count
    OR (SELECT count(*) FROM bid_document_set_artifacts WHERE project_id=project_id_value)<>set_count THEN
    RAISE EXCEPTION 'invalid request changed current snapshot or scheduled work';
  END IF;
  RAISE NOTICE 'duplicate member/stale CAS/missing/duplicate/foreign source rejections verified';

  -- Publish the newest round before older in-flight requests finish. Preserve
  -- a nonempty manual draft so an accidental reset/copy cannot pass vacuously.
  SELECT id INTO STRICT workspace_id_value FROM bid_submission_workspaces WHERE project_id=project_id_value;
  workspace_before := kb_bid_v2_load_workspace_for_actor(workspace_id_value,actor);
  workspace_snapshot := jsonb_build_object('schema_version',1,
    'document_settings',workspace_before->'document_settings',
    'nodes',jsonb_build_array(jsonb_build_object('lineage_id',gen_random_uuid(),
      'revision_id',gen_random_uuid(),'parent_lineage_id',NULL,'ordinal',0,'depth',0,
      'title',gen_random_uuid()::text,'semantic_role','technical','render_role','section',
      'stale',false,'block_lineage_ids','[]'::jsonb)),
    'blocks','[]'::jsonb,'bindings','[]'::jsonb,'lineage_edges','[]'::jsonb);
  request_bytes := convert_to(workspace_snapshot::text,'UTF8');
  workspace_before := kb_bid_v2_commit_workspace_mutation_idempotent(workspace_id_value,
    (workspace_before->>'revision_id')::uuid,(workspace_before->>'sha256')::kb_sha256,
    workspace_snapshot,actor,gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  FOR snapshot_index IN REVERSE jsonb_array_length(snapshots)-1..0 LOOP
    snapshot := snapshots->snapshot_index;
    compiled_items := '[]';
    -- Synthetic compiler output exercises the real SQL publication boundary;
    -- it does not claim to execute the Rust compiler or classify source text.
    FOR source_item IN SELECT value FROM jsonb_array_elements(snapshot->'input'->'source_units') LOOP
      compiled_items := compiled_items || jsonb_build_array(jsonb_build_object(
        'requirement_ref',kb_bid_v2_sha256_bytes(convert_to(source_item::text,'UTF8')),
        'requirement_kind','other','requiredness','unknown','compliance_policy','unknown',
        'requirement_text',source_item->>'text','response_needs',jsonb_build_array(jsonb_build_object('channel','narrative_content')),
        'source_spans',jsonb_build_array(jsonb_build_object('source_id',source_item->>'source_unit_revision_id',
          'start',0,'end',octet_length(source_item->>'text'))),
        'applicability',jsonb_build_object('status','unknown','reason','SQL boundary fixture only',
          'source_unit_revision_ids',jsonb_build_array(source_item->>'source_unit_revision_id')),
        'source_unit_revision_ids',jsonb_build_array(source_item->>'source_unit_revision_id'),
        'structured_form_revision_ids','[]'::jsonb));
    END LOOP;
    compiled := jsonb_build_object('schema_version',4,'requirements',compiled_items,'notices','[]'::jsonb,
      'source_unit_revision_ids',(SELECT jsonb_agg(value->>'source_unit_revision_id')
        FROM jsonb_array_elements(snapshot->'input'->'source_units')));
    publication := kb_fixture_publish_collection_v4(
      (snapshot->'response'->>'request_artifact_id')::uuid,
      (snapshot->'response'->>'request_revision')::bigint,
      (snapshot->'response'->>'frozen_input_sha256')::kb_sha256,
      compiled,'system:requirement-set-compile-v4');
    IF snapshot_index=jsonb_array_length(snapshots)-1 THEN
      IF publication->'published_current' IS DISTINCT FROM 'true'::jsonb
        OR publication->'workspace_apply_required' IS DISTINCT FROM 'false'::jsonb THEN
        RAISE EXCEPTION 'latest analysis did not publish independently of the retired workspace apply flow';
      END IF;
      latest_publication := publication;
      latest_compiled := compiled;
    ELSE
      IF publication->'published_current' IS DISTINCT FROM 'false'::jsonb
        OR publication->'workspace_apply_required' IS DISTINCT FROM 'false'::jsonb THEN
        RAISE EXCEPTION 'late compilation replaced the latest round';
      END IF;
    END IF;
    IF (SELECT artifact_id FROM bid_requirement_set_current WHERE scope_id=project_id_value)
        IS DISTINCT FROM (latest_publication->>'requirement_set_id')::uuid
      OR (SELECT artifact_id FROM bid_workspace_requirement_projection_current WHERE scope_id=workspace_id_value)
        IS DISTINCT FROM (latest_publication->>'requirement_projection_id')::uuid THEN
      RAISE EXCEPTION 'late publication regressed current requirement/projection';
    END IF;
    IF (SELECT document_set_id FROM bid_requirement_set_artifacts
        WHERE id=(publication->>'requirement_set_id')::uuid) IS DISTINCT FROM (snapshot->'response'->>'artifact_id')::uuid
      OR (publication->>'requirement_count')::integer<>jsonb_array_length(compiled_items) THEN
      RAISE EXCEPTION 'published requirement set lost its frozen round identity or requirements';
    END IF;
    SELECT count(*) INTO requirement_count_before FROM bid_requirement_revision_artifacts WHERE project_id=project_id_value;
    SELECT count(*) INTO projection_count_before FROM bid_workspace_requirement_projection_artifacts WHERE project_id=project_id_value;
    replay := kb_fixture_publish_collection_v4(
      (snapshot->'response'->>'request_artifact_id')::uuid,
      (snapshot->'response'->>'request_revision')::bigint,
      (snapshot->'response'->>'frozen_input_sha256')::kb_sha256,
      compiled,'system:requirement-set-compile-v4');
    IF replay IS DISTINCT FROM publication || jsonb_build_object('replayed',true)
      OR (SELECT count(*) FROM bid_requirement_revision_artifacts WHERE project_id=project_id_value)<>requirement_count_before
      OR (SELECT count(*) FROM bid_workspace_requirement_projection_artifacts WHERE project_id=project_id_value)<>projection_count_before THEN
      RAISE EXCEPTION 'publication replay duplicated artifacts or changed receipt';
    END IF;
    IF kb_bid_v2_load_workspace_for_actor(workspace_id_value,actor) IS DISTINCT FROM workspace_before THEN
      RAISE EXCEPTION 'requirement publication rewrote the manual draft before explicit apply';
    END IF;
  END LOOP;
  RAISE NOTICE 'latest publication/four late publications/replay/manual draft preservation verified';

  -- Two decisions for the same collection: an older analysis completing before
  -- the latest one must stay historical even if no newer result exists yet.
  FOR round IN 1..2 LOOP
    SELECT * INTO STRICT disposition_head FROM bid_source_unit_disposition_set_current
      WHERE scope_id=project_id_value;
    request_bytes:=convert_to(gen_random_uuid()::text,'UTF8');
    response:=kb_bid_v2_publish_disposition_set(project_id_value,head.artifact_id,disposition_items,
      disposition_head.artifact_id,disposition_head.artifact_sha256,gen_random_uuid(),actor,
      gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes),runtime);
    decision_requests:=decision_requests||jsonb_build_array(response);
  END LOOP;
  SELECT * INTO STRICT disposition_head FROM bid_source_unit_disposition_set_current
    WHERE scope_id=project_id_value;
  SELECT count(*) INTO disposition_count_before FROM bid_source_unit_disposition_set_artifacts
    WHERE project_id=project_id_value;
  response:=decision_requests->0;
  publication:=kb_fixture_publish_collection_v4((response->>'request_artifact_id')::uuid,
    (response->>'request_revision')::bigint,(response->>'frozen_input_sha256')::kb_sha256,
    latest_compiled,'system:requirement-set-compile-v4');
  IF publication->'published_current' IS DISTINCT FROM 'false'::jsonb
    OR (SELECT artifact_id FROM bid_requirement_set_current WHERE scope_id=project_id_value)
      IS DISTINCT FROM (latest_publication->>'requirement_set_id')::uuid
    OR (SELECT artifact_id FROM bid_source_unit_disposition_set_current WHERE scope_id=project_id_value)
      IS DISTINCT FROM disposition_head.artifact_id
    OR (SELECT count(*) FROM bid_source_unit_disposition_set_artifacts WHERE project_id=project_id_value)
      <>disposition_count_before THEN
    RAISE EXCEPTION 'superseded same-collection analysis changed current basis or consumed a decision generation';
  END IF;
  response:=decision_requests->1;
  latest_publication:=kb_fixture_publish_collection_v4((response->>'request_artifact_id')::uuid,
    (response->>'request_revision')::bigint,(response->>'frozen_input_sha256')::kb_sha256,
    latest_compiled,'system:requirement-set-compile-v4');
  IF latest_publication->'published_current' IS DISTINCT FROM 'true'::jsonb
    OR (SELECT generation FROM bid_source_unit_disposition_set_current WHERE scope_id=project_id_value)
      <>disposition_head.generation+1
    OR kb_bid_v2_load_workspace_for_actor(workspace_id_value,actor) IS DISTINCT FROM workspace_before THEN
    RAISE EXCEPTION 'latest same-collection analysis failed to advance exactly once or rewrote the draft';
  END IF;
  RAISE NOTICE 'same-collection supersession/current generation/manual draft preservation verified';

  IF kb_bid_v2_get_current_docx(workspace_id_value,actor) IS NOT NULL THEN
    RAISE EXCEPTION 'legacy Workspace was implicitly treated as a DOCX round';
  END IF;
  -- SQL tests object metadata and atomic ownership transfer. Real Office byte
  -- validation is exercised separately by docx_round Rust tests.
  FOR round IN 1..2 LOOP
    source_bytes := convert_to(gen_random_uuid()::text,'UTF8');
    initial_sha := kb_bid_v2_sha256_bytes(source_bytes);
    docx_staging := gen_random_uuid(); docx_key := gen_random_uuid()::text;
    PERFORM kb_object_upload_stage(docx_staging,'objects/'||initial_sha,initial_sha,
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document',octet_length(source_bytes),actor);
    SELECT * INTO STRICT head FROM bid_document_set_current WHERE scope_id=project_id_value;
    round_input := jsonb_build_object('document_set_id',head.artifact_id,'document_set_sha256',head.artifact_sha256,
      'requirement_set_id',latest_publication->>'requirement_set_id',
      'requirement_set_sha256',latest_publication->>'requirement_set_sha256',
      'expected_version_id',docx_receipt->>'version_id','expected_docx_sha256',docx_receipt->>'docx_sha256',
      'docx_sha256',initial_sha,'byte_length',octet_length(source_bytes));
    IF round=2 THEN
      -- Changing the source collection cannot silently switch the saved DOCX.
      request_id := gen_random_uuid(); request_bytes := convert_to(gen_random_uuid()::text,'UTF8');
      response := kb_bid_v2_freeze_document_set(project_id_value,selected,head.artifact_id,
        head.artifact_sha256,request_id,actor,gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes),runtime);
      BEGIN
        PERFORM kb_bid_v2_create_docx_round(workspace_id_value,docx_staging,round_input,actor,gen_random_uuid()::text);
        RAISE EXCEPTION 'stale frozen basis accepted';
      EXCEPTION WHEN serialization_failure THEN
        IF SQLERRM<>'DOCX_ROUND_BASIS_CHANGED' THEN RAISE; END IF;
      END;
      round_input := round_input || jsonb_build_object('document_set_id',response->>'artifact_id',
        'document_set_sha256',response->>'sha256');
      BEGIN
        PERFORM kb_bid_v2_create_docx_round(workspace_id_value,docx_staging,round_input,actor,gen_random_uuid()::text);
        RAISE EXCEPTION 'old requirements applied to a new collection';
      EXCEPTION WHEN serialization_failure THEN
        IF SQLERRM<>'DOCX_ROUND_REQUIREMENTS_NOT_CURRENT' THEN RAISE; END IF;
      END;
      latest_publication := kb_fixture_publish_collection_v4(request_id,
        (response->>'request_revision')::bigint,(response->>'frozen_input_sha256')::kb_sha256,
        latest_compiled,'system:requirement-set-compile-v4');
      round_input := round_input || jsonb_build_object('requirement_set_id',latest_publication->>'requirement_set_id',
        'requirement_set_sha256',latest_publication->>'requirement_set_sha256');
      IF kb_bid_v2_get_current_docx(workspace_id_value,actor) IS DISTINCT FROM first_docx_current THEN
        RAISE EXCEPTION 'source publication implicitly replaced the saved DOCX';
      END IF;
    END IF;
    BEGIN
      PERFORM kb_bid_v2_create_docx_round(workspace_id_value,docx_staging,
        round_input || jsonb_build_object('byte_length',octet_length(source_bytes)+1),actor,gen_random_uuid()::text);
      RAISE EXCEPTION 'staged object size mismatch accepted';
    EXCEPTION WHEN check_violation THEN
      IF SQLERRM<>'object upload staging content identity mismatch' THEN RAISE; END IF;
    END;
    BEGIN
      PERFORM kb_bid_v2_create_docx_round(workspace_id_value,docx_staging,round_input,wrong_actor,gen_random_uuid()::text);
      RAISE EXCEPTION 'cross-owner round creation accepted';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    PERFORM set_config('role','kb_runtime_api',true);
    docx_receipt := kb_bid_v2_create_docx_round(workspace_id_value,docx_staging,round_input,actor,docx_key);
    PERFORM set_config('role','kb_app_owner',true);
    IF (docx_receipt->>'round_revision')::integer<>round
      OR (SELECT count(*) FROM bid_docx_round_artifacts WHERE workspace_id=workspace_id_value)<>round
      OR (SELECT count(*) FROM bid_docx_version_artifacts WHERE workspace_id=workspace_id_value)<>round
      OR EXISTS (SELECT 1 FROM object_upload_staging WHERE id=docx_staging)
      OR NOT EXISTS (SELECT 1 FROM object_owner_references reference WHERE reference.owner_kind='bid_docx_version'
        AND reference.owner_id=(docx_receipt->>'version_id')::uuid AND reference.object_ref='objects/'||initial_sha) THEN
      RAISE EXCEPTION 'round/version/current/object transfer not atomic';
    END IF;
    replay_staging := gen_random_uuid();
    PERFORM kb_object_upload_stage(replay_staging,'objects/'||initial_sha,initial_sha,
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document',octet_length(source_bytes),actor);
    replay := kb_bid_v2_create_docx_round(workspace_id_value,replay_staging,round_input,actor,docx_key);
    IF replay IS DISTINCT FROM docx_receipt OR EXISTS (SELECT 1 FROM object_upload_staging WHERE id=replay_staging) THEN
      RAISE EXCEPTION 'fresh staging replay changed version or leaked staging';
    END IF;
    BEGIN
      PERFORM kb_bid_v2_create_docx_round(workspace_id_value,gen_random_uuid(),
        round_input || jsonb_build_object('byte_length',octet_length(source_bytes)+1),actor,docx_key);
      RAISE EXCEPTION 'idempotency key accepted changed DOCX input';
    EXCEPTION WHEN unique_violation THEN NULL; END;
    BEGIN
      PERFORM kb_bid_v2_create_docx_round(workspace_id_value,gen_random_uuid(),round_input,actor,gen_random_uuid()::text);
      RAISE EXCEPTION 'stale version CAS accepted';
    EXCEPTION WHEN serialization_failure THEN
      IF SQLERRM<>'DOCX_VERSION_CAS_MISMATCH' THEN RAISE; END IF;
    END;
    IF round=1 THEN
      first_docx_receipt := docx_receipt; first_round_input := round_input; first_docx_key := docx_key;
      first_docx_metadata := kb_bid_v2_get_docx_version(workspace_id_value,(docx_receipt->>'version_id')::uuid,actor);
      first_docx_current := kb_bid_v2_get_current_docx(workspace_id_value,actor);
    END IF;
    IF kb_bid_v2_load_workspace_for_actor(workspace_id_value,actor) IS DISTINCT FROM workspace_before THEN
      RAISE EXCEPTION 'new DOCX round rewrote the historical legacy draft';
    END IF;
  END LOOP;
  replay := kb_bid_v2_create_docx_round(workspace_id_value,gen_random_uuid(),first_round_input,actor,first_docx_key);
  IF replay IS DISTINCT FROM first_docx_receipt
    OR kb_bid_v2_get_docx_version(workspace_id_value,(first_docx_receipt->>'version_id')::uuid,actor) IS DISTINCT FROM first_docx_metadata
    OR kb_bid_v2_get_current_docx(workspace_id_value,actor)->>'version_id' IS DISTINCT FROM docx_receipt->>'version_id'
    OR (SELECT count(*) FROM bid_docx_version_artifacts WHERE workspace_id=workspace_id_value)<>2
    OR EXISTS (SELECT 1 FROM bid_docx_version_artifacts WHERE workspace_id=workspace_id_value AND parent_version_id IS NOT NULL) THEN
    RAISE EXCEPTION 'new round inherited a parent version, lost history or old replay regressed current';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_get_docx_version(workspace_id_value,(first_docx_receipt->>'version_id')::uuid,wrong_actor);
    RAISE EXCEPTION 'cross-owner DOCX read accepted';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  PERFORM set_config('role','kb_runtime_api',true);
  BEGIN
    DELETE FROM bid_docx_current WHERE scope_id=workspace_id_value;
    RAISE EXCEPTION 'runtime API bypassed the domain writer';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  PERFORM set_config('role','kb_runtime_worker',true);
  BEGIN
    PERFORM kb_bid_v2_get_current_docx(workspace_id_value,actor);
    RAISE EXCEPTION 'worker inherited the API document read grant';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  PERFORM set_config('role','kb_app_owner',true);
  RAISE NOTICE 'DOCX rounds: current basis/CAS/ownership/replay/history/role boundaries verified';
  RAISE NOTICE 'document-collection-acceptance-ok';
END $$;
ROLLBACK;
