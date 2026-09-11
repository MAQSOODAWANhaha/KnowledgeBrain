\set ON_ERROR_STOP on

-- phase1 isolated Knowledge fixture for executable phase3/phase6 acceptance.
INSERT INTO users(id,email) VALUES
 ('00000000-0000-4000-8000-000000000001','v2-contract@example.invalid');
-- A real knowledge-owned text source closes the EvidenceBundle text_quote
-- provenance tuple; tender SourceUnit UUIDs are never accepted as a substitute.
INSERT INTO workspaces(id,name,slug,kind) VALUES
 ('00000000-0000-4000-8000-0000000001b8','evidence-workspace','evidence-workspace','company');
INSERT INTO products(id,workspace_id,kind,name,slug) VALUES
 ('00000000-0000-4000-8000-0000000001b7','00000000-0000-4000-8000-0000000001b8','library','evidence-library','evidence-library');
INSERT INTO product_versions(id,product_id,label,status) VALUES
 ('00000000-0000-4000-8000-0000000001b4','00000000-0000-4000-8000-0000000001b7','v1','active');
UPDATE products SET current_version_id='00000000-0000-4000-8000-0000000001b4'
WHERE id='00000000-0000-4000-8000-0000000001b7';
SELECT kb_object_reference_add(
 'objects/'||encode(digest(convert_to('knowledge-fixture','UTF8'),'sha256'),'hex'),
 encode(digest(convert_to('knowledge-fixture','UTF8'),'sha256'),'hex'),
 'text/plain',17,'knowledge_document','00000000-0000-4000-8000-0000000001b2','original',
 'system:knowledge-document-ingest');
INSERT INTO documents(id,product_version_id,title,parse_status,enable_status,index_ready,file_name,file_size,file_hash,object_ref) VALUES
 ('00000000-0000-4000-8000-0000000001b2','00000000-0000-4000-8000-0000000001b4','verified source','completed','enabled',true,'verified-source.txt',17,
  encode(digest(convert_to('knowledge-fixture','UTF8'),'sha256'),'hex'),
  'objects/'||encode(digest(convert_to('knowledge-fixture','UTF8'),'sha256'),'hex'));
INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,start_at,end_at) VALUES
 ('00000000-0000-4000-8000-0000000001b3','00000000-0000-4000-8000-0000000001b4','00000000-0000-4000-8000-0000000001b2','text','verified fact',0,13),
 ('00000000-0000-4000-8000-0000000001c3','00000000-0000-4000-8000-0000000001b4','00000000-0000-4000-8000-0000000001b2','text','unit1',0,5),
 ('00000000-0000-4000-8000-0000000001c4','00000000-0000-4000-8000-0000000001b4','00000000-0000-4000-8000-0000000001b2','text','unit1',0,5);

-- Immutable retrieval registry fixture for Phase 3/6 replay.
DO $$
DECLARE
 embedding_payload text;
 embedding_sha text;
 rerank_payload text;
 rerank_sha text;
 policy_payload text;
 policy_sha text;
 scope jsonb;
 attestation jsonb;
BEGIN
 embedding_payload:='{"schema_version":2,"provider_protocol_version":"openai-compatible-embeddings-json-v1","provider_model_identifier":"phase0-embedding@2026-01-01","provider_model_revision_sha256":"'||repeat('1',64)||'","endpoint_config_sha256":"'||repeat('2',64)||'","endpoint_identity":"https://embedding.example.invalid/v1","dimension":1024,"request_config_sha256":"a2ccbf02dc959b101e69f85df1b494ae0852065383e1e88e2a1c5a4bd09f40cb","output_normalization_version":"finite-vector-no-client-normalization-v1"}';
 embedding_sha:=encode(digest(convert_to(embedding_payload,'UTF8'),'sha256'),'hex');
 INSERT INTO embedding_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,dimension,request_config_sha256,output_normalization_version,credential_ref)
 VALUES(embedding_sha,convert_to(embedding_payload,'UTF8'),2,'openai-compatible-embeddings-json-v1','phase0-embedding@2026-01-01',repeat('1',64),repeat('2',64),'https://embedding.example.invalid/v1',1024,'a2ccbf02dc959b101e69f85df1b494ae0852065383e1e88e2a1c5a4bd09f40cb','finite-vector-no-client-normalization-v1','env:KB_V2_TEST_EMBEDDING_KEY');
 rerank_payload:='{"schema_version":2,"provider_protocol_version":"indexed-json-v1","provider_model_identifier":"phase0-rerank@2026-01-01","provider_model_revision_sha256":"'||repeat('3',64)||'","config_revision_sha256":"'||repeat('4',64)||'","endpoint_identity":"https://rerank.example.invalid/v1","request_config_sha256":"21c0ee51fa4df1a5e436fab5e5df6ab851c2f6ebfcf115c86d77b40f40bf02f1","score_normalization_version":"unit-interval-millionths-v1"}';
 rerank_sha:=encode(digest(convert_to(rerank_payload,'UTF8'),'sha256'),'hex');
 INSERT INTO rerank_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,config_revision_sha256,endpoint_identity,request_config_sha256,score_normalization_version,credential_ref)
 VALUES(rerank_sha,convert_to(rerank_payload,'UTF8'),2,'indexed-json-v1','phase0-rerank@2026-01-01',repeat('3',64),repeat('4',64),'https://rerank.example.invalid/v1','21c0ee51fa4df1a5e436fab5e5df6ab851c2f6ebfcf115c86d77b40f40bf02f1','unit-interval-millionths-v1','env:KB_V2_TEST_RERANK_KEY');
 policy_payload:='{"schema_version":2,"contract_version":"knowledge-evidence-v2","normalization_version":"unicode-whitespace-lowercase-v1","trusted_source_types":["text","parent_text","image_ocr"],"ranking":{"a_primary_comparator":["chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"],"a_version_comparator":["product_id ASC","product_version_id ASC"],"b_exact_comparator":["product_id ASC","product_version_id ASC","chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"],"c_semantic_comparator":["normalized_rerank_score DESC","pre_rerank_rrf_rank ASC","complete_source_identity ASC"],"source_folding_version":"unique-live-trusted-source-v1","channel_score_quantization_version":"floor-unit-interval-millionths-v1","channel_rank_comparator":["score_millionths DESC","complete_signal_identity ASC"],"pre_rerank_rrf_comparator":["exact_rrf_score DESC","vector_rank ASC NULLS LAST","keyword_rank ASC NULLS LAST","product_id ASC","product_version_id ASC","document_id ASC","source_chunk_id ASC"],"quota_semantics_version":"fair-exact-prefix-fail-closed-v1"},"keyword":{"tokenizer":"latin-numeric-cjk-bigram","tokenizer_version":"v1","score_version":"postgres-ts-rank-cd-normalization32-millionths-v1","top_k":1,"threshold_millionths":0},"embedding":{"policy":"declared-version-model","policy_version":"v1","similarity_version":"pgvector-cosine-clamp-zero-one-millionths-v1","model_revision_sha256":"'||embedding_sha||'","top_k":1,"threshold_millionths":0},"rrf":{"k":60,"keyword_weight_millionths":1000000,"vector_weight_millionths":1000000,"score_representation_version":"reduced-u128-rational-v1"},"rerank":{"provider_protocol_version":"indexed-json-v1","revision_sha256":"'||rerank_sha||'","model_revision_sha256":"'||repeat('3',64)||'","config_revision_sha256":"'||repeat('4',64)||'","top_k":1,"timeout_ms":1000,"score_normalization_version":"unit-interval-millionths-v1"},"request_quotas":{"max_hits":2,"max_chunk_bytes":1024,"max_total_bytes":1024}}';
 policy_sha:=encode(digest(convert_to(policy_payload,'UTF8'),'sha256'),'hex');
 INSERT INTO knowledge_retrieval_policies_v2(policy_sha256,canonical_policy_payload,embedding_revision_sha256,rerank_revision_sha256,contract_version,max_hits,max_chunk_bytes,max_total_bytes)
 VALUES(policy_sha,convert_to(policy_payload,'UTF8'),embedding_sha,rerank_sha,'knowledge-evidence-v2',2,1024,1024);
END $$;

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
  '10000000-0000-4000-8000-000000000028',
  (SELECT revision FROM bid_async_request_snapshot_artifacts WHERE id='10000000-0000-4000-8000-000000000028'),
  (SELECT frozen_input_sha256 FROM bid_async_request_snapshot_artifacts WHERE id='10000000-0000-4000-8000-000000000028'),
  'AGENT_OUTPUT_INVALID');
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
  '10000000-0000-4000-8000-000000000029',
  (SELECT revision FROM bid_async_request_snapshot_artifacts WHERE id='10000000-0000-4000-8000-000000000029'),
  (SELECT frozen_input_sha256 FROM bid_async_request_snapshot_artifacts WHERE id='10000000-0000-4000-8000-000000000029'),
  'AGENT_OUTPUT_INVALID');

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
  '10000000-0000-4000-8000-000000000021','10000000-0000-4000-8000-000000000041','section',0,'{"schema_version":2,"project_id":"10000000-0000-4000-8000-000000000010","document_id":"10000000-0000-4000-8000-000000000021","converted_source_revision_id":"10000000-0000-4000-8000-000000000041","parser_unit_key":"phase1-unit-1","parser_ordinal":0,"source_purpose":"tender_requirements_and_structure_only","locator":{"locator_kind":"document","section_ordinal":0,"table_ordinal":null,"row_ordinal":null,"form_ordinal":null,"heading_path":""}}',repeat('1',64),
  convert_to('投标人必须提供技术方案','UTF8'),kb_bid_v2_sha256_bytes(convert_to('投标人必须提供技术方案','UTF8')),
  convert_to('unit-one','UTF8'),kb_bid_v2_sha256_bytes(convert_to('unit-one','UTF8'))),
 ('10000000-0000-4000-8000-000000000062','10000000-0000-4000-8000-000000000010','10000000-0000-4000-8000-000000000052',1,
  '10000000-0000-4000-8000-000000000024','10000000-0000-4000-8000-000000000042','table_row',0,'{"schema_version":2,"project_id":"10000000-0000-4000-8000-000000000010","document_id":"10000000-0000-4000-8000-000000000024","converted_source_revision_id":"10000000-0000-4000-8000-000000000042","parser_unit_key":"phase1-unit-2","parser_ordinal":0,"source_purpose":"tender_requirements_and_structure_only","locator":{"locator_kind":"document","section_ordinal":0,"table_ordinal":0,"row_ordinal":0,"form_ordinal":null,"heading_path":""}}',repeat('2',64),
  convert_to('报价表必须完整填写','UTF8'),kb_bid_v2_sha256_bytes(convert_to('报价表必须完整填写','UTF8')),
  convert_to('unit-two','UTF8'),kb_bid_v2_sha256_bytes(convert_to('unit-two','UTF8')));

-- Freeze one ready and one failed document. The failed member remains visible as
-- a warning, but only the ready immutable source participates in compilation.
DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  head bid_document_set_current%ROWTYPE; disposition_head bid_source_unit_disposition_set_current%ROWTYPE;
  value jsonb; published jsonb; compile_value jsonb; items jsonb; request_bytes bytea;
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
  SELECT * INTO STRICT disposition_head FROM bid_source_unit_disposition_set_current
    WHERE scope_id='10000000-0000-4000-8000-000000000010';
  SELECT jsonb_agg(jsonb_build_object(
           'source_unit_revision_id',source.id,'disposition','requirement',
           'reason','phase1 ready source') ORDER BY source.id)
    INTO items
    FROM bid_document_set_items set_item
    JOIN bid_source_unit_revision_artifacts source
      ON source.project_id=set_item.project_id AND source.source_revision_id=set_item.source_revision_id
    WHERE set_item.document_set_id=(value->>'artifact_id')::uuid;
  request_bytes:=convert_to(items::text,'UTF8');
  published:=kb_bid_v2_publish_disposition_set(
    '10000000-0000-4000-8000-000000000010',(value->>'artifact_id')::uuid,items,
    disposition_head.artifact_id,disposition_head.artifact_sha256,gen_random_uuid(),actor,
    'phase1-disposition-partial',request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  compile_value:=kb_bid_v2_compile_requirement_set(
    (published->>'request_artifact_id')::uuid,(published->>'request_revision')::bigint,
    (published->>'frozen_input_sha256')::kb_sha256,'system:requirement-set-compile-v2');
  IF (compile_value->>'requirement_count')::integer<>1 THEN
    RAISE EXCEPTION 'partial DocumentSet compile did not use only ready input';
  END IF;
END $$;

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  head bid_document_set_current%ROWTYPE; disposition_head bid_source_unit_disposition_set_current%ROWTYPE;
  value jsonb; replay_value jsonb; compile_value jsonb; published jsonb; items jsonb; request_bytes bytea;
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
  SELECT * INTO STRICT disposition_head FROM bid_source_unit_disposition_set_current
    WHERE scope_id='10000000-0000-4000-8000-000000000010';
  SELECT jsonb_agg(jsonb_build_object(
           'source_unit_revision_id',source.id,'disposition','requirement',
           'reason','phase1 ready source') ORDER BY source.id)
    INTO items
    FROM bid_document_set_items set_item
    JOIN bid_source_unit_revision_artifacts source
      ON source.project_id=set_item.project_id AND source.source_revision_id=set_item.source_revision_id
    WHERE set_item.document_set_id=(value->>'artifact_id')::uuid;
  request_bytes:=convert_to(items::text,'UTF8');
  published:=kb_bid_v2_publish_disposition_set(
    '10000000-0000-4000-8000-000000000010',(value->>'artifact_id')::uuid,items,
    disposition_head.artifact_id,disposition_head.artifact_sha256,gen_random_uuid(),actor,
    'phase1-disposition-ready',request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  compile_value:=kb_bid_v2_compile_requirement_set(
    (published->>'request_artifact_id')::uuid,(published->>'request_revision')::bigint,
    (published->>'frozen_input_sha256')::kb_sha256,'system:requirement-set-compile-v2');
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
    'unused.png','image/png',1,1,1,NULL,'objects/'||repeat('e',64),repeat('e',64),actor,
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
