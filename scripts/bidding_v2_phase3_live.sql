\set ON_ERROR_STOP on

-- Phase 3 live publication: a user-triggered ContentGenerate freezes the current
-- checkpoint, worker publication creates explicit no-evidence bundles and a
-- candidate, and match_only reuses the same coarse job without a Candidate.
-- First prove OutlineGenerate resolves every authoring input through its frozen
-- identities rather than current pointers.
DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  v_workspace_id uuid; head bid_workspace_heads%ROWTYPE; current_set bid_requirement_set_artifacts%ROWTYPE;
  document_set bid_document_set_artifacts%ROWTYPE; request_value jsonb; frozen_input jsonb;
  request_bytes bytea:=convert_to('{"operation":"outline-generate-frozen-input"}','UTF8');
BEGIN
  SELECT id INTO STRICT v_workspace_id FROM bid_submission_workspaces
    WHERE project_id='10000000-0000-4000-8000-000000000010';
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=v_workspace_id;
  SELECT requirement_set.* INTO STRICT current_set FROM bid_workspace_requirement_projection_current projection_head
    JOIN bid_workspace_requirement_projection_artifacts projection ON projection.id=projection_head.artifact_id
    JOIN bid_requirement_set_artifacts requirement_set ON requirement_set.id=projection.requirement_set_id
    WHERE projection_head.scope_id=v_workspace_id;
  SELECT * INTO STRICT document_set FROM bid_document_set_artifacts WHERE id=current_set.document_set_id;
  request_value:=kb_bid_v2_create_outline_candidate(v_workspace_id,head.artifact_id,head.artifact_sha256,
    document_set.id,document_set.content_sha256,actor,'phase3-outline-frozen-input',request_bytes,
    kb_bid_v2_sha256_bytes(request_bytes));
  frozen_input:=kb_bid_v2_load_outline_generation_input(
    (request_value->>'request_artifact_id')::uuid,(request_value->>'request_revision')::bigint,
    (request_value->>'frozen_input_sha256')::kb_sha256);
  IF frozen_input->'workspace_scope' IS NULL
     OR frozen_input#>'{document_set,items}' IS DISTINCT FROM convert_from(document_set.canonical_payload,'UTF8')::jsonb->'items'
     OR frozen_input#>'{document_set,relations}' IS DISTINCT FROM convert_from(document_set.canonical_payload,'UTF8')::jsonb->'relations'
     OR jsonb_array_length(frozen_input->'source_units')<>(SELECT count(*) FROM bid_source_unit_disposition_set_items item
       WHERE item.disposition_set_id=current_set.disposition_set_id)
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(frozen_input->'source_units') source
       WHERE NOT EXISTS (SELECT 1 FROM bid_source_unit_disposition_set_items item
         WHERE item.disposition_set_id=current_set.disposition_set_id
           AND item.source_unit_revision_id=(source->>'source_unit_revision_id')::uuid
           AND item.disposition=source->>'disposition'))
     OR frozen_input->'structured_forms' IS NULL OR frozen_input->'requirements' IS NULL
     OR frozen_input->'current_outline' IS NULL THEN
    RAISE EXCEPTION 'outline frozen disposition/document/form/scope input mismatch'
      USING DETAIL=jsonb_build_object('workspace_scope',frozen_input->'workspace_scope',
        'document_items',frozen_input#>'{document_set,items}',
        'expected_document_items',convert_from(document_set.canonical_payload,'UTF8')::jsonb->'items',
        'document_relations',frozen_input#>'{document_set,relations}',
        'expected_document_relations',convert_from(document_set.canonical_payload,'UTF8')::jsonb->'relations',
        'source_units',frozen_input->'source_units','structured_forms',frozen_input->'structured_forms',
        'requirements',frozen_input->'requirements','current_outline',frozen_input->'current_outline')::text;
  END IF;
END $$;

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  workspace_id uuid; workspace_value jsonb; checkpoint jsonb; request_value jsonb;
  frozen_input jsonb; candidate_payload bytea; candidate_sha kb_sha256;
  candidate_id uuid:='10000000-0000-4000-8000-0000000000a1'; result_value jsonb;
  operation jsonb; block_content jsonb; block_value jsonb;
  policy jsonb; scope jsonb; attestation jsonb; matches jsonb;
  request_bytes bytea:=convert_to('{"target":"node","operation":"generate"}','UTF8');
  request_sha kb_sha256;
BEGIN
  SELECT id INTO STRICT workspace_id FROM bid_submission_workspaces
    WHERE project_id='10000000-0000-4000-8000-000000000010';
  workspace_value:=kb_bid_v2_load_workspace_for_actor(workspace_id,actor);
  checkpoint:=kb_bid_v2_create_outline_checkpoint(
    workspace_id,(workspace_value->>'revision_id')::uuid,(workspace_value->>'sha256')::kb_sha256,
    '10000000-0000-4000-8000-0000000000a0',actor,'phase3-checkpoint',
    convert_to('{"checkpoint":1}','UTF8'),kb_bid_v2_sha256_bytes(convert_to('{"checkpoint":1}','UTF8')));
  IF checkpoint->>'artifact_id'<>'10000000-0000-4000-8000-0000000000a0' THEN
    RAISE EXCEPTION 'outline checkpoint was not frozen';
  END IF;
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  request_value:=kb_bid_v2_create_content_request(
    workspace_id,(workspace_value->>'revision_id')::uuid,(workspace_value->>'sha256')::kb_sha256,
    'generate','node','10000000-0000-4000-8000-000000000081','empty_only',NULL,
    'system_proposed',NULL,actor,'phase3-content-generate',request_bytes,request_sha);
  IF request_value->>'status'<>'pending' OR request_value->>'operation'<>'generate' THEN
    RAISE EXCEPTION 'content request was not persisted pending';
  END IF;
  frozen_input:=kb_bid_v2_load_content_generation_input(
    (request_value->>'request_artifact_id')::uuid,(request_value->>'request_revision')::bigint,
    (request_value->>'frozen_input_sha256')::kb_sha256);
  IF jsonb_array_length(frozen_input->'target_nodes')<>1 OR jsonb_array_length(frozen_input->'requirements')<>2 THEN
    RAISE EXCEPTION 'content frozen input did not bind target and requirements';
  END IF;
  SELECT jsonb_build_object('contract_version',contract_version,'policy_sha256',policy_sha256,
    'max_hits',max_hits,'max_chunk_bytes',max_chunk_bytes,'max_total_bytes',max_total_bytes)
    INTO STRICT policy FROM knowledge_retrieval_policies_v2 WHERE support_state='supported'
    ORDER BY created_at DESC,policy_sha256 LIMIT 1;
  scope:=jsonb_build_object('schema_version',2,'workspace_kinds','[]'::jsonb,
    'version_selections',jsonb_build_object('product_line','[]'::jsonb,'company','[]'::jsonb),
    'products','[]'::jsonb,'retrieval_requirements',(SELECT jsonb_agg(jsonb_build_object(
      'route_id',requirement->>'requirement_revision_id','requirement_artifact_id',requirement->>'requirement_revision_id',
      'requirement_identity_sha256',requirement->>'requirement_identity_sha256','requirement_text',requirement->>'requirement_text',
      'exact_prefix_hit_count',0)) FROM jsonb_array_elements(frozen_input->'requirements') requirement),
    'frozen_hits','[]'::jsonb,'retrieval_policy',policy);
  attestation:=kb_knowledge_attest_matching_scope_v2(scope);
  SELECT jsonb_agg(jsonb_build_object('requirement_revision_id',requirement->>'requirement_revision_id',
    'evidence_bundle_id',gen_random_uuid(),
    'items','[]'::jsonb)) INTO matches FROM jsonb_array_elements(frozen_input->'requirements') requirement;
  block_content:=jsonb_build_object('type','rich_text','nodes',jsonb_build_array(jsonb_build_object(
    'kind','paragraph','content',jsonb_build_array(jsonb_build_object('kind','text','text','【待人工补充】候选响应内容','marks','[]'::jsonb)))));
  block_value:=jsonb_build_object('schema_version',1,'block_revision_id','10000000-0000-4000-8000-0000000000a2',
    'lineage_id','10000000-0000-4000-8000-0000000000a3','revision',1,'kind','rich_text',
    'content',block_content,'origin','agent_candidate','dependency_sha256',request_value->>'frozen_input_sha256',
    'stale',false,'content_sha256',kb_bid_v2_sha256_bytes(convert_to(block_content::text,'UTF8')));
  operation:=jsonb_build_object('kind','insert_block','client_operation_ref','phase3-op-0',
    'target_node_lineage_id','10000000-0000-4000-8000-000000000081','ordinal',0,'block',block_value);
  candidate_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
    'operations',jsonb_build_array(operation),'factual_claims','[]'::jsonb,
    'notices',jsonb_build_array(jsonb_build_object('code','NO_EVIDENCE','message','未检索到可用企业证据','requirement_revision_id',NULL))));
  candidate_sha:=kb_bid_v2_sha256_bytes(candidate_payload);
  result_value:=kb_bid_v2_publish_content_generation(
    (request_value->>'request_artifact_id')::uuid,(request_value->>'request_revision')::bigint,
    (request_value->>'frozen_input_sha256')::kb_sha256,(attestation->>'id')::uuid,
    (attestation->>'content_sha256')::kb_sha256,matches,candidate_id,candidate_payload,candidate_sha,
    jsonb_build_array(operation));
  IF (result_value->>'artifact_id')::uuid<>candidate_id
     OR (kb_bid_v2_get_candidate(workspace_id,candidate_id,actor)->>'status')<>'proposed' THEN
    RAISE EXCEPTION 'content candidate publication failed';
  END IF;
  IF (SELECT count(*) FROM bid_content_generation_request_evidence_bundles
      WHERE request_artifact_id=(request_value->>'request_artifact_id')::uuid)<>2
     OR EXISTS (SELECT 1 FROM bid_content_generation_request_evidence_bundles link
       JOIN bid_evidence_bundle_items item ON item.evidence_bundle_id=link.evidence_bundle_id
       WHERE link.request_artifact_id=(request_value->>'request_artifact_id')::uuid
         AND item.item_kind<>'no_evidence') THEN
    RAISE EXCEPTION 'explicit no-evidence bundle publication failed';
  END IF;
END $$;

DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  workspace_id uuid; workspace_value jsonb; request_value jsonb; result_value jsonb; frozen_input jsonb;
  policy jsonb; scope jsonb; attestation jsonb; matches jsonb; first_requirement_id uuid;
  evidence_item_id constant uuid:='10000000-0000-4000-8000-0000000000b5';
  request_bytes bytea:=convert_to('{"target":"node","operation":"match_only"}','UTF8');
BEGIN
  SELECT id INTO STRICT workspace_id FROM bid_submission_workspaces
    WHERE project_id='10000000-0000-4000-8000-000000000010';
  workspace_value:=kb_bid_v2_load_workspace_for_actor(workspace_id,actor);
  request_value:=kb_bid_v2_create_content_request(
    workspace_id,(workspace_value->>'revision_id')::uuid,(workspace_value->>'sha256')::kb_sha256,
    'match_only','node','10000000-0000-4000-8000-000000000081','missing_requirements_only',NULL,
    'system_proposed',NULL,actor,'phase3-evidence-match',request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  frozen_input:=kb_bid_v2_load_content_generation_input(
    (request_value->>'request_artifact_id')::uuid,(request_value->>'request_revision')::bigint,
    (request_value->>'frozen_input_sha256')::kb_sha256);
  SELECT jsonb_build_object('contract_version',contract_version,'policy_sha256',policy_sha256,
    'max_hits',max_hits,'max_chunk_bytes',max_chunk_bytes,'max_total_bytes',max_total_bytes)
    INTO STRICT policy FROM knowledge_retrieval_policies_v2 WHERE support_state='supported'
    ORDER BY created_at DESC,policy_sha256 LIMIT 1;
  first_requirement_id:=(frozen_input#>>'{requirements,0,requirement_revision_id}')::uuid;
  scope:=jsonb_build_object('schema_version',2,'workspace_kinds',jsonb_build_array('company'),
    'version_selections',jsonb_build_object('product_line','[]'::jsonb,'company','[]'::jsonb),
    'products',jsonb_build_array(jsonb_build_object('id','00000000-0000-4000-8000-0000000001b4',
      'product_id','00000000-0000-4000-8000-0000000001b7','product_version_id','00000000-0000-4000-8000-0000000001b4',
      'workspace_kind','company','frozen_display_name','00000000-0000-4000-8000-0000000001b4',
      'identity_sha256',encode(digest(convert_to('ProductVersionEvidenceV1:00000000-0000-4000-8000-0000000001b7:00000000-0000-4000-8000-0000000001b4:company','UTF8'),'sha256'),'hex'))),
    'retrieval_requirements',(SELECT jsonb_agg(jsonb_build_object(
      'route_id',requirement->>'requirement_revision_id','requirement_artifact_id',requirement->>'requirement_revision_id',
      'requirement_identity_sha256',requirement->>'requirement_identity_sha256','requirement_text',requirement->>'requirement_text',
      'exact_prefix_hit_count',0))
      FROM jsonb_array_elements(frozen_input->'requirements') requirement),
    'frozen_hits',jsonb_build_array(jsonb_build_object('id','10000000-0000-4000-8000-0000000000b4',
      'route_id',first_requirement_id,'requirement_artifact_id',first_requirement_id,
      'product_version_artifact_id','00000000-0000-4000-8000-0000000001b4',
      'document_id','00000000-0000-4000-8000-0000000001b2','source_chunk_id','00000000-0000-4000-8000-0000000001b3',
      'frozen_document_display_name','verified-source.txt','chunk_utf8','verified fact',
      'chunk_sha256',encode(digest(convert_to('verified fact','UTF8'),'sha256'),'hex'),'chunk_byte_length',13,
      'source_type','text','media',NULL,'retrieval_rank',1,'retrieval_raw_score','0.500000',
      'pre_rerank_rrf_rank',1,'quote_start_offset',0,'quote_end_offset',13,'offset_unit','utf8_byte',
      'retrieval_contract_version','knowledge-evidence-v2')),'retrieval_policy',policy);
  attestation:=kb_knowledge_attest_matching_scope_v2(scope);
  SELECT jsonb_agg(jsonb_build_object('requirement_revision_id',requirement->>'requirement_revision_id',
    'evidence_bundle_id',gen_random_uuid(),'items',CASE
      WHEN (requirement->>'requirement_revision_id')::uuid=first_requirement_id THEN jsonb_build_array(jsonb_build_object(
        'kind','text_quote','evidence_item_id',evidence_item_id,
        'document_id','00000000-0000-4000-8000-0000000001b2','source_chunk_id','00000000-0000-4000-8000-0000000001b3',
        'product_version_id','00000000-0000-4000-8000-0000000001b4','workspace_kind','company',
        'frozen_document_display_name','verified-source.txt','quote_utf8','verified fact',
        'quote_sha256',encode(digest(convert_to('verified fact','UTF8'),'sha256'),'hex'),
        'quote_start_offset',0,'quote_end_offset',13,'retrieval_rank',1,
        'retrieval_contract_version','knowledge-evidence-v2')) ELSE '[]'::jsonb END))
    INTO matches FROM jsonb_array_elements(frozen_input->'requirements') requirement;
  result_value:=kb_bid_v2_publish_content_generation(
    (request_value->>'request_artifact_id')::uuid,(request_value->>'request_revision')::bigint,
    (request_value->>'frozen_input_sha256')::kb_sha256,(attestation->>'id')::uuid,
    (attestation->>'content_sha256')::kb_sha256,matches,NULL,NULL,NULL,'[]'::jsonb);
  IF result_value->>'artifact_id'<>request_value->>'request_artifact_id'
     OR (kb_bid_v2_get_async_request(workspace_id,(request_value->>'request_artifact_id')::uuid,actor)->>'status')<>'succeeded'
     OR EXISTS (SELECT 1 FROM bid_candidate_artifacts WHERE request_artifact_id=(request_value->>'request_artifact_id')::uuid) THEN
    RAISE EXCEPTION 'match_only did not complete without a candidate';
  END IF;
END $$;

-- Node-scoped Evidence is a frozen user selection over a MatchReport and is
-- replayable without creating a second PickSet artifact.
DO $$
DECLARE actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  v_workspace_id uuid; report_id uuid; first_value jsonb; replay_value jsonb; node_evidence jsonb;
  request_bytes bytea:=convert_to('{"pick":["10000000-0000-4000-8000-0000000000b5"]}','UTF8');
BEGIN
  SELECT id INTO STRICT v_workspace_id FROM bid_submission_workspaces
    WHERE project_id='10000000-0000-4000-8000-000000000010';
  SELECT report.id INTO STRICT report_id FROM bid_evidence_match_reports report
    JOIN bid_evidence_bundle_artifacts bundle ON bundle.matching_report_id=report.id
    JOIN bid_evidence_bundle_items item ON item.evidence_bundle_id=bundle.id
    WHERE report.workspace_id=v_workspace_id AND report.node_lineage_id='10000000-0000-4000-8000-000000000081'
      AND item.id='10000000-0000-4000-8000-0000000000b5';
  first_value:=kb_bid_v2_create_node_evidence_pick_set(v_workspace_id,
    '10000000-0000-4000-8000-000000000081',report_id,ARRAY['10000000-0000-4000-8000-0000000000b5'::uuid],actor,
    'phase3-node-pick',request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  replay_value:=kb_bid_v2_create_node_evidence_pick_set(v_workspace_id,
    '10000000-0000-4000-8000-000000000081',report_id,ARRAY['10000000-0000-4000-8000-0000000000b5'::uuid],actor,
    'phase3-node-pick',request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  node_evidence:=kb_bid_v2_get_node_evidence(v_workspace_id,'10000000-0000-4000-8000-000000000081',actor);
  IF first_value IS DISTINCT FROM replay_value OR jsonb_array_length(node_evidence->'bundles')<2
     OR jsonb_array_length(node_evidence->'pick_sets')<>1 THEN
    RAISE EXCEPTION 'node evidence projection or PickSet replay failed';
  END IF;
END $$;
