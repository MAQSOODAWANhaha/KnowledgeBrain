\set ON_ERROR_STOP on

-- Phase 1 user-visible vertical acceptance: owner/idempotency, role/relation,
-- DocumentSet freeze, exactly-one disposition and RequirementProjection compile.
INSERT INTO users(id,email) VALUES
 ('10000000-0000-4000-8000-000000000001','v2-phase1-owner@example.invalid'),
 ('10000000-0000-4000-8000-000000000002','v2-phase1-other@example.invalid');

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  request_bytes bytea:=convert_to('{"title":"phase1"}','UTF8');
  request_sha kb_sha256; first_value jsonb; replay_value jsonb;
BEGIN
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  first_value:=kb_bid_v2_create_project(
    '10000000-0000-4000-8000-000000000010','phase1-project',
    '10000000-0000-4000-8000-000000000001',actor,'phase1-project-create',request_bytes,request_sha);
  replay_value:=kb_bid_v2_create_project(
    '10000000-0000-4000-8000-000000000099','ignored-on-replay',
    '10000000-0000-4000-8000-000000000001',actor,'phase1-project-create',request_bytes,request_sha);
  IF first_value IS DISTINCT FROM replay_value OR (replay_value->>'id')::uuid<>'10000000-0000-4000-8000-000000000010' THEN
    RAISE EXCEPTION 'project idempotency replay changed identity';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_create_project('10000000-0000-4000-8000-000000000098','bad-replay',
      '10000000-0000-4000-8000-000000000001',actor,'phase1-project-create',
      convert_to('{"title":"different"}','UTF8'),kb_bid_v2_sha256_bytes(convert_to('{"title":"different"}','UTF8')));
    RAISE EXCEPTION 'idempotency payload mismatch accepted';
  EXCEPTION WHEN unique_violation THEN NULL; END;
END $$;

SELECT kb_object_upload_stage('10000000-0000-4000-8000-000000000020',
  'objects/'||repeat('a',64),repeat('a',64),'application/pdf',1,
  'user:10000000-0000-4000-8000-000000000001');
SELECT kb_bid_v2_upload_tender_document(
  '10000000-0000-4000-8000-000000000020','10000000-0000-4000-8000-000000000021',
  '10000000-0000-4000-8000-000000000022','10000000-0000-4000-8000-000000000010',
  '招标文件.pdf','application/pdf',1,'objects/'||repeat('a',64),repeat('a',64),
  'user:10000000-0000-4000-8000-000000000001','phase1-upload-one',
  convert_to('{"file":"one"}','UTF8'),kb_bid_v2_sha256_bytes(convert_to('{"file":"one"}','UTF8')));

SELECT kb_object_upload_stage('10000000-0000-4000-8000-000000000023',
  'objects/'||repeat('b',64),repeat('b',64),'application/pdf',1,
  'user:10000000-0000-4000-8000-000000000001');
SELECT kb_bid_v2_upload_tender_document(
  '10000000-0000-4000-8000-000000000023','10000000-0000-4000-8000-000000000024',
  '10000000-0000-4000-8000-000000000025','10000000-0000-4000-8000-000000000010',
  '澄清文件.pdf','application/pdf',1,'objects/'||repeat('b',64),repeat('b',64),
  'user:10000000-0000-4000-8000-000000000001','phase1-upload-two',
  convert_to('{"file":"two"}','UTF8'),kb_bid_v2_sha256_bytes(convert_to('{"file":"two"}','UTF8')));

SELECT kb_object_upload_stage('10000000-0000-4000-8000-000000000026',
  'objects/'||repeat('2',64),repeat('2',64),'application/pdf',1,
  'user:10000000-0000-4000-8000-000000000001');
SELECT kb_bid_v2_upload_tender_document(
  '10000000-0000-4000-8000-000000000026','10000000-0000-4000-8000-000000000027',
  '10000000-0000-4000-8000-000000000028','10000000-0000-4000-8000-000000000010',
  '失败重试文件.pdf','application/pdf',1,'objects/'||repeat('2',64),repeat('2',64),
  'user:10000000-0000-4000-8000-000000000001','phase1-upload-retry-fixture',
  convert_to('{"file":"retry"}','UTF8'),kb_bid_v2_sha256_bytes(convert_to('{"file":"retry"}','UTF8')));
SELECT kb_bid_v2_mark_tender_document_failed(
  '10000000-0000-4000-8000-000000000028','AGENT_OUTPUT_INVALID');
DO $$
DECLARE value jsonb;
BEGIN
  value:=kb_bid_v2_retry_tender_document(
    '10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000027',
    '10000000-0000-4000-8000-000000000029',1,
    'user:10000000-0000-4000-8000-000000000001','phase1-retry-document',
    convert_to('{"expected_generation":1}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"expected_generation":1}','UTF8')));
  IF value->>'parse_status'<>'pending' OR (value->>'conversion_generation')::bigint<>2 THEN
    RAISE EXCEPTION 'failed TenderDocument retry did not create generation two';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_retry_tender_document(
      '10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000027',
      gen_random_uuid(),1,'user:10000000-0000-4000-8000-000000000001','phase1-retry-stale',
      convert_to('{"expected_generation":1,"stale":true}','UTF8'),
      kb_bid_v2_sha256_bytes(convert_to('{"expected_generation":1,"stale":true}','UTF8')));
    RAISE EXCEPTION 'stale retry generation accepted';
  EXCEPTION WHEN serialization_failure THEN NULL; END;
END $$;
SELECT kb_bid_v2_mark_tender_document_failed(
  '10000000-0000-4000-8000-000000000029','AGENT_OUTPUT_INVALID');

DO $$ BEGIN
  BEGIN
    PERFORM kb_bid_v2_list_tender_documents('10000000-0000-4000-8000-000000000010',
      'user:10000000-0000-4000-8000-000000000002');
    RAISE EXCEPTION 'cross-owner tender read accepted';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  role_id uuid; role_sha kb_sha256; first_value jsonb; replay_value jsonb;
BEGIN
  SELECT role.id,role.content_sha256 INTO role_id,role_sha
  FROM bid_document_role_current head JOIN bid_document_role_revision_artifacts role ON role.id=head.artifact_id
  WHERE head.scope_id='10000000-0000-4000-8000-000000000021';
  first_value:=kb_bid_v2_patch_document_role(
    '10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000021',
    'technical_specification',role_id,role_sha,actor,'phase1-role-confirm',
    convert_to('{"role":"technical_specification"}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"role":"technical_specification"}','UTF8')));
  replay_value:=kb_bid_v2_patch_document_role(
    '10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000021',
    'technical_specification',role_id,role_sha,actor,'phase1-role-confirm',
    convert_to('{"role":"technical_specification"}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"role":"technical_specification"}','UTF8')));
  IF first_value IS DISTINCT FROM replay_value OR first_value->>'role_provenance'<>'human_modified' THEN
    RAISE EXCEPTION 'role replay/provenance invalid';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_patch_document_role(
      '10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000021',
      'not_a_role',(first_value->>'role_revision_id')::uuid,(first_value->>'role_revision_sha256')::kb_sha256,
      actor,'phase1-role-invalid',convert_to('{"role":"bad"}','UTF8'),
      kb_bid_v2_sha256_bytes(convert_to('{"role":"bad"}','UTF8')));
    RAISE EXCEPTION 'invalid role accepted';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  value jsonb;
BEGIN
  value:=kb_bid_v2_upsert_document_relation(
    '10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000030',
    '10000000-0000-4000-8000-000000000024','10000000-0000-4000-8000-000000000021',
    'clarifies','{}',NULL,NULL,actor,'phase1-relation',convert_to('{"relation":"clarifies"}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"relation":"clarifies"}','UTF8')));
  IF value->>'relation_kind'<>'clarifies' THEN RAISE EXCEPTION 'relation kind lost'; END IF;
  BEGIN
    PERFORM kb_bid_v2_upsert_document_relation(
      '10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000031',
      '10000000-0000-4000-8000-000000000021','10000000-0000-4000-8000-000000000021',
      'clarifies','{}',NULL,NULL,actor,'phase1-relation-invalid',convert_to('{"relation":"self"}','UTF8'),
      kb_bid_v2_sha256_bytes(convert_to('{"relation":"self"}','UTF8')));
    RAISE EXCEPTION 'self relation accepted';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;

-- Simulate the already separately accepted TenderDocumentProcess publication
-- so this script can focus on role/relation/DocumentSet/RequirementSet behavior.
UPDATE bid_documents SET parse_status='ready'
WHERE project_id='10000000-0000-4000-8000-000000000010'
  AND id IN ('10000000-0000-4000-8000-000000000021','10000000-0000-4000-8000-000000000024');
INSERT INTO bid_converted_source_artifacts(id,project_id,document_id,revision,source_object_ref,source_sha256,
  converter_contract_id,converter_contract_sha256,image_asset_set_sha256)
SELECT value.source_id,'10000000-0000-4000-8000-000000000010',value.document_id,1,
  'objects/'||value.source_sha,value.source_sha,'00000000-0000-5000-8000-000000000001',contract.content_sha256,repeat('0',64)
FROM (VALUES
  ('10000000-0000-4000-8000-000000000041'::uuid,'10000000-0000-4000-8000-000000000021'::uuid,repeat('c',64)::kb_sha256),
  ('10000000-0000-4000-8000-000000000042'::uuid,'10000000-0000-4000-8000-000000000024'::uuid,repeat('d',64)::kb_sha256)
) value(source_id,document_id,source_sha)
CROSS JOIN bid_authoring_contract_artifacts contract
WHERE contract.id='00000000-0000-5000-8000-000000000001';
INSERT INTO bid_source_unit_lineages(id,project_id,document_id) VALUES
 ('10000000-0000-4000-8000-000000000051','10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000021'),
 ('10000000-0000-4000-8000-000000000052','10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000024');
INSERT INTO bid_source_unit_revision_artifacts(id,project_id,lineage_id,revision,document_id,source_revision_id,
  unit_kind,ordinal,source_locator,source_span_sha256,text_utf8,text_sha256,canonical_payload,content_sha256) VALUES
 ('10000000-0000-4000-8000-000000000061','10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000051',1,
  '10000000-0000-4000-8000-000000000021','10000000-0000-4000-8000-000000000041','section',0,'{}',repeat('1',64),
  convert_to('投标人必须提供技术方案','UTF8'),kb_bid_v2_sha256_bytes(convert_to('投标人必须提供技术方案','UTF8')),
  convert_to('unit-one','UTF8'),kb_bid_v2_sha256_bytes(convert_to('unit-one','UTF8'))),
 ('10000000-0000-4000-8000-000000000062','10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000052',1,
  '10000000-0000-4000-8000-000000000024','10000000-0000-4000-8000-000000000042','table_row',0,'{}',repeat('2',64),
  convert_to('报价表必须完整填写','UTF8'),kb_bid_v2_sha256_bytes(convert_to('报价表必须完整填写','UTF8')),
  convert_to('unit-two','UTF8'),kb_bid_v2_sha256_bytes(convert_to('unit-two','UTF8')));

-- Freeze one ready and one failed document. The failed member remains visible as
-- a warning, but only the ready immutable source participates in compilation.
DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  head bid_document_set_current%ROWTYPE; value jsonb; compile_value jsonb;
BEGIN
  SELECT * INTO STRICT head FROM bid_document_set_current
    WHERE scope_id='10000000-0000-4000-8000-000000000010';
  value:=kb_bid_v2_freeze_document_set(
    '10000000-0000-4000-8000-000000000010',ARRAY[
      '10000000-0000-4000-8000-000000000021'::uuid,
      '10000000-0000-4000-8000-000000000027'::uuid],
    head.artifact_id,head.artifact_sha256,'10000000-0000-4000-8000-000000000068',actor,
    'phase1-freeze-partial',convert_to('{"documents":["ready","failed"]}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"documents":["ready","failed"]}','UTF8')));
  IF jsonb_array_length(value->'warnings')<>1
     OR value#>>'{warnings,0,disposition}'<>'failed' THEN
    RAISE EXCEPTION 'partial DocumentSet did not preserve failed-input warning';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_document_set_items
      WHERE document_set_id=(value->>'artifact_id')::uuid
        AND document_id='10000000-0000-4000-8000-000000000027'
        AND disposition='failed' AND source_revision_id IS NULL) THEN
    RAISE EXCEPTION 'partial DocumentSet froze a failed source as ready input';
  END IF;
  compile_value:=kb_bid_v2_compile_requirement_set(
    (value->>'request_artifact_id')::uuid,(value->>'request_revision')::bigint,
    (value->>'frozen_input_sha256')::kb_sha256,'system:requirement-set-compile-v2');
  IF (compile_value->>'requirement_count')::integer<>1 THEN
    RAISE EXCEPTION 'partial DocumentSet compile did not use only ready input';
  END IF;
END $$;

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  head bid_document_set_current%ROWTYPE; value jsonb; replay_value jsonb; compile_value jsonb;
BEGIN
  SELECT * INTO STRICT head FROM bid_document_set_current WHERE scope_id='10000000-0000-4000-8000-000000000010';
  value:=kb_bid_v2_freeze_document_set(
    '10000000-0000-4000-8000-000000000010',ARRAY[
      '10000000-0000-4000-8000-000000000021'::uuid,'10000000-0000-4000-8000-000000000024'::uuid],
    head.artifact_id,head.artifact_sha256,'10000000-0000-4000-8000-000000000070',actor,'phase1-freeze',
    convert_to('{"documents":["one","two"]}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"documents":["one","two"]}','UTF8')));
  replay_value:=kb_bid_v2_freeze_document_set(
    '10000000-0000-4000-8000-000000000010',ARRAY[
      '10000000-0000-4000-8000-000000000021'::uuid,'10000000-0000-4000-8000-000000000024'::uuid],
    head.artifact_id,head.artifact_sha256,'10000000-0000-4000-8000-000000000071',actor,'phase1-freeze',
    convert_to('{"documents":["one","two"]}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"documents":["one","two"]}','UTF8')));
  IF value IS DISTINCT FROM replay_value THEN RAISE EXCEPTION 'document set replay changed receipt'; END IF;
  compile_value:=kb_bid_v2_compile_requirement_set(
    (value->>'request_artifact_id')::uuid,(value->>'request_revision')::bigint,
    (value->>'frozen_input_sha256')::kb_sha256,'system:requirement-set-compile-v2');
  IF (compile_value->>'requirement_count')::integer<>2 THEN
    RAISE EXCEPTION 'requirement compile count mismatch';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_freeze_document_set(
      '10000000-0000-4000-8000-000000000010',ARRAY['10000000-0000-4000-8000-000000000021'::uuid],
      head.artifact_id,head.artifact_sha256,'10000000-0000-4000-8000-000000000072',actor,'phase1-freeze-stale',
      convert_to('{"documents":["one"]}','UTF8'),kb_bid_v2_sha256_bytes(convert_to('{"documents":["one"]}','UTF8')));
    RAISE EXCEPTION 'stale document set CAS accepted';
  EXCEPTION WHEN serialization_failure THEN NULL; END;
END $$;

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  documents jsonb; relations jsonb; units jsonb; requirements jsonb;
BEGIN
  documents:=kb_bid_v2_list_tender_documents('10000000-0000-4000-8000-000000000010',actor);
  relations:=kb_bid_v2_list_document_relations('10000000-0000-4000-8000-000000000010',actor);
  units:=kb_bid_v2_list_source_units('10000000-0000-4000-8000-000000000010',actor);
  requirements:=kb_bid_v2_list_requirements('10000000-0000-4000-8000-000000000010',actor);
  IF jsonb_array_length(documents)<>3 OR jsonb_array_length(relations)<>1
     OR jsonb_array_length(units)<>2 OR jsonb_array_length(requirements)<>2 THEN
    RAISE EXCEPTION 'phase1 list projection count mismatch';
  END IF;
  IF EXISTS (SELECT 1 FROM jsonb_array_elements(units) item WHERE item->>'disposition'<>'requirement') THEN
    RAISE EXCEPTION 'source unit lacks exactly one requirement disposition';
  END IF;
  IF (SELECT generation FROM bid_workspace_requirement_projection_current
      WHERE project_id='10000000-0000-4000-8000-000000000010')<>3 THEN
    RAISE EXCEPTION 'workspace requirement projection did not advance';
  END IF;
END $$;

-- Phase 2 entry slice: owner-scoped, idempotent workspace mutation with
-- If-Match-equivalent aggregate CAS and immutable node lineage/revision.
DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  workspace_id uuid; before_value jsonb; snapshot jsonb; first_value jsonb; replay_value jsonb;
  request_bytes bytea:=convert_to('{"operation":"insert_node"}','UTF8');
  request_sha kb_sha256;
BEGIN
  SELECT id INTO STRICT workspace_id FROM bid_submission_workspaces
    WHERE project_id='10000000-0000-4000-8000-000000000010';
  before_value:=kb_bid_v2_load_workspace_for_actor(workspace_id,actor);
  IF before_value->>'requirement_projection_revision_id' IS NULL THEN
    RAISE EXCEPTION 'workspace did not expose current requirement projection';
  END IF;
  snapshot:=jsonb_build_object(
    'schema_version',1,
    'document_settings',before_value->'document_settings',
    'nodes',jsonb_build_array(jsonb_build_object(
      'lineage_id','10000000-0000-4000-8000-000000000081',
      'revision_id','10000000-0000-4000-8000-000000000082',
      'parent_lineage_id',NULL,'ordinal',0,'depth',0,'title','技术方案',
      'semantic_role','technical','render_role','section','stale',false,
      'block_lineage_ids','[]'::jsonb)),
    'blocks','[]'::jsonb,'bindings','[]'::jsonb,'lineage_edges','[]'::jsonb);
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  first_value:=kb_bid_v2_commit_workspace_mutation_idempotent(
    workspace_id,(before_value->>'revision_id')::uuid,(before_value->>'sha256')::kb_sha256,
    snapshot,actor,'phase2-workspace-insert',request_bytes,request_sha);
  replay_value:=kb_bid_v2_commit_workspace_mutation_idempotent(
    workspace_id,(before_value->>'revision_id')::uuid,(before_value->>'sha256')::kb_sha256,
    snapshot,actor,'phase2-workspace-insert',request_bytes,request_sha);
  IF first_value IS DISTINCT FROM replay_value OR jsonb_array_length(first_value->'nodes')<>1 THEN
    RAISE EXCEPTION 'workspace mutation replay or materialization mismatch';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_commit_workspace_mutation_idempotent(
      workspace_id,(before_value->>'revision_id')::uuid,(before_value->>'sha256')::kb_sha256,
      snapshot,actor,'phase2-workspace-stale',request_bytes,request_sha);
    RAISE EXCEPTION 'stale workspace CAS accepted';
  EXCEPTION WHEN serialization_failure THEN NULL; END;
  BEGIN
    PERFORM kb_bid_v2_load_workspace_for_actor(
      workspace_id,'user:10000000-0000-4000-8000-000000000002');
    RAISE EXCEPTION 'cross-owner workspace read accepted';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

-- Phase 2 resource contract: requirement projection reads, immutable asset
-- retirement, and binding create/update/delete all preserve the Workspace CAS.
DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  workspace_id uuid; projection jsonb; upload_value jsonb; retired jsonb; replay jsonb;
  request_bytes bytea:=convert_to('{"asset":"retire"}','UTF8');
BEGIN
  SELECT id INTO STRICT workspace_id FROM bid_submission_workspaces
    WHERE project_id='10000000-0000-4000-8000-000000000010';
  projection:=kb_bid_v2_get_requirement_projection(workspace_id,actor);
  IF jsonb_array_length(projection->'items')<>2 THEN RAISE EXCEPTION 'requirement projection read mismatch'; END IF;
  PERFORM kb_object_upload_stage('10000000-0000-4000-8000-0000000000b0',
    'objects/'||repeat('e',64),repeat('e',64),'image/png',1,actor);
  upload_value:=kb_bid_v2_upload_workspace_asset(workspace_id,
    '10000000-0000-4000-8000-0000000000b1','10000000-0000-4000-8000-0000000000b0',
    'unused.png','image/png',1,'objects/'||repeat('e',64),repeat('e',64),actor,
    'phase2-asset-upload',convert_to('{"asset":"upload"}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"asset":"upload"}','UTF8')));
  retired:=kb_bid_v2_retire_workspace_asset(workspace_id,(upload_value->>'asset_revision_id')::uuid,
    'user_removed',actor,'phase2-asset-retire',request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  replay:=kb_bid_v2_retire_workspace_asset(workspace_id,(upload_value->>'asset_revision_id')::uuid,
    'user_removed',actor,'phase2-asset-retire',request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  IF retired IS DISTINCT FROM replay OR jsonb_array_length(kb_bid_v2_list_workspace_assets(workspace_id,actor))<>0 THEN
    RAISE EXCEPTION 'asset retirement/replay failed';
  END IF;
END $$;

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  workspace_id uuid; workspace_value jsonb; docset bid_document_set_current%ROWTYPE;
  request_value jsonb; replay_value jsonb; candidate_value jsonb; snapshot jsonb; accepted jsonb;
  nodes jsonb; outline_input jsonb; candidate_payload bytea; candidate_sha kb_sha256;
  request_bytes bytea:=convert_to('{"outline":"generate"}','UTF8'); request_sha kb_sha256;
  candidate_id uuid:='10000000-0000-4000-8000-000000000090';
BEGIN
  SELECT id INTO STRICT workspace_id FROM bid_submission_workspaces
    WHERE project_id='10000000-0000-4000-8000-000000000010';
  workspace_value:=kb_bid_v2_load_workspace_for_actor(workspace_id,actor);
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  request_value:=kb_bid_v2_create_outline_candidate(
    workspace_id,(workspace_value->>'revision_id')::uuid,(workspace_value->>'sha256')::kb_sha256,
    (workspace_value->>'document_set_revision_id')::uuid,(workspace_value->>'document_set_sha256')::kb_sha256,
    actor,'phase2-outline-generate',request_bytes,request_sha);
  replay_value:=kb_bid_v2_create_outline_candidate(
    workspace_id,(workspace_value->>'revision_id')::uuid,(workspace_value->>'sha256')::kb_sha256,
    (workspace_value->>'document_set_revision_id')::uuid,(workspace_value->>'document_set_sha256')::kb_sha256,
    actor,'phase2-outline-generate',request_bytes,request_sha);
  IF request_value IS DISTINCT FROM replay_value OR request_value->>'status'<>'pending' THEN
    RAISE EXCEPTION 'outline generation replay/status mismatch';
  END IF;
  outline_input:=kb_bid_v2_load_outline_generation_input(
    (request_value->>'request_artifact_id')::uuid,(request_value->>'request_revision')::bigint,
    (request_value->>'frozen_input_sha256')::kb_sha256);
  IF jsonb_array_length(outline_input->'requirements')<>2
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(outline_input->'requirements') requirement
       WHERE NOT requirement ? 'requiredness' OR requirement ? 'mandatory') THEN
    RAISE EXCEPTION 'outline frozen input contract mismatch';
  END IF;
  nodes:=jsonb_build_array(jsonb_build_object(
    'client_node_ref','outline-technical','parent_client_node_ref',NULL,'ordinal',0,
    'title','技术方案','semantic_role','technical','render_role','section',
    'origin_source_unit_revision_ids','[]'::jsonb));
  candidate_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'nodes',nodes,
    'bindings','[]'::jsonb,'notices','[]'::jsonb));
  candidate_sha:=kb_bid_v2_sha256_bytes(candidate_payload);
  PERFORM kb_bid_v2_publish_outline_generation(
    (request_value->>'request_artifact_id')::uuid,(request_value->>'request_revision')::bigint,
    (request_value->>'frozen_input_sha256')::kb_sha256,candidate_id,candidate_payload,candidate_sha,nodes);
  candidate_value:=kb_bid_v2_get_candidate(workspace_id,candidate_id,actor);
  IF candidate_value->>'status'<>'proposed' OR jsonb_array_length(candidate_value->'nodes')=0 THEN
    RAISE EXCEPTION 'outline candidate missing';
  END IF;
  snapshot:=jsonb_build_object(
    'schema_version',1,'document_settings',workspace_value->'document_settings',
    'nodes',(workspace_value->'nodes') || jsonb_build_array(jsonb_build_object(
      'lineage_id','10000000-0000-4000-8000-000000000091',
      'revision_id','10000000-0000-4000-8000-000000000092',
      'parent_lineage_id',NULL,'ordinal',1,'depth',0,
      'title',candidate_value#>>'{nodes,0,title}',
      'semantic_role',coalesce(candidate_value#>>'{nodes,0,semantic_role}','other'),
      'render_role','section','stale',false,'block_lineage_ids','[]'::jsonb)),
    'blocks',workspace_value->'blocks','bindings',workspace_value->'bindings','lineage_edges','[]'::jsonb);
  accepted:=kb_bid_v2_accept_candidate(
    workspace_id,candidate_id,(workspace_value->>'revision_id')::uuid,(workspace_value->>'sha256')::kb_sha256,
    snapshot,ARRAY[0],actor,'phase2-outline-accept',convert_to('{"accept":[0]}','UTF8'),
    kb_bid_v2_sha256_bytes(convert_to('{"accept":[0]}','UTF8')));
  IF jsonb_array_length(accepted->'nodes')<>2 OR
     (kb_bid_v2_get_candidate(workspace_id,candidate_id,actor)->>'status')<>'accepted' THEN
    RAISE EXCEPTION 'outline candidate acceptance did not commit workspace';
  END IF;
END $$;
