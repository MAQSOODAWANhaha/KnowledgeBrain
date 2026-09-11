-- Synthetic output of the shared Python docreader service, for Agent persistence tests.
DO $$
DECLARE
  owner_id uuid:=gen_random_uuid(); project_id_value uuid:=gen_random_uuid();
  actor kb_actor_identity:='user:'||owner_id::text;
  documents uuid[]:='{}';
  document_id_value uuid; staging_id uuid; request_id uuid;
  source_id uuid; lineage_id uuid; unit_id uuid;
  original_bytes bytea; source_bytes bytea; request_bytes bytea;
  original_sha kb_sha256; source_sha kb_sha256;
  typed bid_tender_document_process_request_identities%ROWTYPE;
  head bid_document_set_current%ROWTYPE; response jsonb;
  media text:=coalesce(nullif(current_setting('kb_test.media_type',true),''),'application/pdf');
BEGIN
  INSERT INTO users(id,email) VALUES(owner_id,owner_id::text || '@example.invalid');
  request_bytes := convert_to(jsonb_build_object('title',project_id_value)::text,'UTF8');
  PERFORM kb_bid_v2_create_project(project_id_value,'collection regression',owner_id,
    actor,gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  document_id_value := gen_random_uuid(); staging_id := gen_random_uuid();
  request_id := gen_random_uuid();
  original_bytes := coalesce(decode(nullif(current_setting('kb_test.original_hex',true),''),'hex'),convert_to(gen_random_uuid()::text,'UTF8'));
  original_sha := kb_bid_v2_sha256_bytes(original_bytes);
  request_bytes := convert_to(jsonb_build_object('file',document_id_value)::text,'UTF8');
  PERFORM kb_object_upload_stage(staging_id,'objects/' || original_sha,original_sha,
    media,octet_length(original_bytes),actor);
  PERFORM kb_bid_v2_upload_tender_document(staging_id,document_id_value,request_id,
    project_id_value,document_id_value::text || CASE WHEN media='image/png' THEN '.png' ELSE '.pdf' END,media,
    octet_length(original_bytes),'objects/' || original_sha,original_sha,actor,
    gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  documents := array_append(documents,document_id_value);
    -- Seed the output of a successful parse. This does not run a parser.
    SELECT * INTO STRICT typed FROM bid_tender_document_process_request_identities
      WHERE request_artifact_id=request_id;
    source_id := gen_random_uuid(); lineage_id := gen_random_uuid(); unit_id := gen_random_uuid();
    source_bytes := convert_to('提交响应表并附证明材料。','UTF8');
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
  SELECT * INTO STRICT head FROM bid_document_set_current WHERE scope_id=project_id_value;
  request_id:=gen_random_uuid();
  request_bytes:=convert_to(jsonb_build_object('documents',documents)::text,'UTF8');
  response:=kb_bid_v2_freeze_document_set(project_id_value,documents,head.artifact_id,
    head.artifact_sha256,request_id,actor,gen_random_uuid()::text,request_bytes,
    kb_bid_v2_sha256_bytes(request_bytes),current_setting('kb_test.runtime')::jsonb);
  PERFORM set_config('kb_test.request',response::text,true);
END $$;
