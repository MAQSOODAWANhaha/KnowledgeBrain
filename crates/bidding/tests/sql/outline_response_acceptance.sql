-- Fresh, isolated baseline only. Every identity is unique; no existing data is removed.
-- T20: stale/expired leases write nothing; T21: staged publication is durable.
DO $acceptance$
DECLARE
  v_user uuid := gen_random_uuid();
  v_project uuid := gen_random_uuid();
  v_other_project uuid := gen_random_uuid();
  v_run uuid := gen_random_uuid();
  v_other_run uuid := gen_random_uuid();
  token_a uuid := gen_random_uuid();
  token_b uuid := gen_random_uuid();
  epoch_a bigint;
  epoch_b bigint;
  v_frozen kb_sha256 := encode(public.digest(v_project::text, 'sha256'), 'hex')::kb_sha256;
  artifact jsonb;
  bindings jsonb := '[{"form_id":"form-1","chapter_id":"response"}]';
  published jsonb;
  replayed jsonb;
  response jsonb;
  docx jsonb;
  exported jsonb;
  outline_sha kb_sha256;
  v_document uuid := gen_random_uuid();
  stage_source uuid := gen_random_uuid();
  stage_docx uuid := gen_random_uuid();
  stage_export uuid := gen_random_uuid();
  stage_extra uuid := gen_random_uuid();
  stage_bad uuid := gen_random_uuid();
  v_source_sha kb_sha256 := encode(public.digest(v_document::text, 'sha256'), 'hex')::kb_sha256;
  v_docx_sha kb_sha256 := encode(public.digest(stage_docx::text, 'sha256'), 'hex')::kb_sha256;
  v_export_sha kb_sha256 := encode(public.digest(stage_export::text, 'sha256'), 'hex')::kb_sha256;
  v_bad_sha kb_sha256 := encode(public.digest(stage_bad::text, 'sha256'), 'hex')::kb_sha256;
  docx_media text := 'application/vnd.openxmlformats-officedocument.wordprocessingml.document';
  before_run jsonb;
  current_run jsonb;
  deletion jsonb;
  v_placeholder text;
BEGIN
  INSERT INTO users(id, email) VALUES (v_user, v_user::text || '@example.invalid');
  INSERT INTO bid_projects(id, owner_user_id, title, status)
  VALUES (v_project, v_user, 'storage acceptance', 'open'),
         (v_other_project, v_user, 'identity isolation', 'open');
  PERFORM kb_bid_v2_register_frozen_input(v_project, v_frozen, 'set');
  PERFORM kb_bid_v2_register_frozen_input(v_project, v_frozen, 'set');
  BEGIN
    PERFORM kb_bid_v2_register_frozen_input(v_project, v_frozen, 'wrong-set');
    RAISE EXCEPTION 'frozen identity changed';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    PERFORM kb_bid_v2_outline_claim(v_other_run, v_other_project, v_frozen, token_a, 120);
    RAISE EXCEPTION 'cross-project frozen input accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL;
  END;

  artifact := jsonb_build_object(
    'project_id', v_project, 'frozen_input_sha256', v_frozen,
    'required_requirement_ids',jsonb_build_array('req-1'),
    'fulfillments',jsonb_build_array(jsonb_build_object('requirement_id','req-1','target_refs',jsonb_build_array('blank'))),
    'semantic_review_complete',true,
    'chapters', jsonb_build_array(
      jsonb_build_object('id','group','parent_id','','order',0,'title','投标文件','purpose','group'),
      jsonb_build_object('id','response','parent_id','group','order',0,'title','投标函','purpose','response')),
    'templates', jsonb_build_array(jsonb_build_object('slot_id','blank','chapter_id','response',
      'kind','bidder_blank','text','','response_required',true,'match_query','投标函')));
  epoch_a := (kb_bid_v2_outline_claim(v_run, v_project, v_frozen, token_a, 120)->>'lease_epoch')::bigint;
  BEGIN
    PERFORM kb_bid_v2_outline_claim(v_run, v_project, v_frozen, token_b, 120);
    RAISE EXCEPTION 'second live lease accepted';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  UPDATE bid_outline_runs SET lease_until = clock_timestamp() - interval '1 second' WHERE id = v_run;
  SELECT to_jsonb(r) INTO before_run FROM bid_outline_runs r WHERE id = v_run;
  BEGIN
    PERFORM kb_bid_v2_publish_outline(v_run, v_project, v_frozen, token_a, epoch_a,
      convert_to(artifact::text, 'UTF8'), bindings, NULL);
    RAISE EXCEPTION 'expired worker published';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  SELECT to_jsonb(r) INTO current_run FROM bid_outline_runs r WHERE id = v_run;
  IF current_run IS DISTINCT FROM before_run
     OR EXISTS (SELECT 1 FROM bid_outline_artifacts WHERE run_id = v_run) THEN
    RAISE EXCEPTION 'expired publish changed the run or artifact';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_outline_renew(v_run, token_a, epoch_a, 120);
    RAISE EXCEPTION 'expired worker renewed';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  epoch_b := (kb_bid_v2_outline_claim(v_run, v_project, v_frozen, token_b, 120)->>'lease_epoch')::bigint;
  IF epoch_b <= epoch_a THEN RAISE EXCEPTION 'lease epoch did not advance'; END IF;
  SELECT to_jsonb(r) INTO before_run FROM bid_outline_runs r WHERE id = v_run;
  BEGIN
    PERFORM kb_bid_v2_publish_outline(v_run, v_project, v_frozen, token_a, epoch_a,
      convert_to(artifact::text, 'UTF8'), bindings, NULL);
    RAISE EXCEPTION 'superseded worker published';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  BEGIN
    PERFORM kb_bid_v2_outline_checkpoint(v_run, token_a, epoch_a, '{"stale":true}');
    RAISE EXCEPTION 'superseded worker wrote checkpoint';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  BEGIN
    PERFORM kb_bid_v2_outline_pause_budget(v_run, token_a, epoch_a);
    RAISE EXCEPTION 'superseded worker paused current worker';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  SELECT to_jsonb(r) INTO current_run FROM bid_outline_runs r WHERE id = v_run;
  IF current_run IS DISTINCT FROM before_run
     OR EXISTS (SELECT 1 FROM bid_outline_artifacts WHERE run_id = v_run) THEN
    RAISE EXCEPTION 'superseded worker changed storage';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_publish_outline(v_run, v_project, v_frozen, token_b, epoch_a,
      convert_to(artifact::text, 'UTF8'), bindings, NULL);
    RAISE EXCEPTION 'wrong epoch published';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  PERFORM kb_bid_v2_outline_checkpoint(v_run,token_b,epoch_b,jsonb_build_object(
    'done',true,'outline_run',jsonb_build_object('tool_draft',jsonb_build_object('finished',true)),
    'journal',jsonb_build_object('publication_receipt',jsonb_build_object(
      'artifact_sha256',kb_bid_v2_sha256_bytes(convert_to(artifact::text,'UTF8')),'bindings',bindings))));
  BEGIN
    PERFORM kb_bid_v2_publish_outline(v_run,v_project,v_frozen,token_b,epoch_b,
      convert_to((artifact || '{"required_requirement_ids":[],"fulfillments":[],"semantic_review_complete":true}')::text,'UTF8'),bindings,NULL);
    RAISE EXCEPTION 'forged self-reported semantic review published';
  EXCEPTION WHEN check_violation THEN
    IF SQLERRM NOT LIKE '%PUBLICATION_CANDIDATE_MISMATCH%' THEN RAISE; END IF;
  END;
  published := kb_bid_v2_publish_outline(v_run, v_project, v_frozen, token_b, epoch_b,
    convert_to(artifact::text, 'UTF8'), bindings, NULL);
  outline_sha := (published->>'outline_sha256')::kb_sha256;
  IF published->>'replayed' <> 'false' THEN RAISE EXCEPTION 'first publish was replay'; END IF;
  replayed := kb_bid_v2_publish_outline(v_run, v_project, v_frozen, token_b, epoch_b,
    convert_to(artifact::text, 'UTF8'), bindings, NULL);
  IF replayed->>'replayed' <> 'true' OR replayed->>'outline_sha256' <> outline_sha::text THEN
    RAISE EXCEPTION 'lost ACK replay failed';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_publish_outline(v_run, v_project, v_frozen, token_a, epoch_a,
      convert_to(artifact::text, 'UTF8'), bindings, NULL);
    RAISE EXCEPTION 'stale worker borrowed completed replay';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  BEGIN
    PERFORM kb_bid_v2_publish_outline(v_run, v_project, v_frozen, token_b, epoch_b,
      convert_to((artifact || '{"changed":true}')::text, 'UTF8'), bindings, NULL);
    RAISE EXCEPTION 'changed artifact borrowed completed replay';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    PERFORM kb_bid_v2_publish_outline(v_run, v_project, v_frozen, token_b, epoch_b,
      convert_to(artifact::text, 'UTF8'), '[]', NULL);
    RAISE EXCEPTION 'changed bindings borrowed completed replay';
  EXCEPTION WHEN check_violation THEN NULL;
  END;

  response := kb_bid_v2_publish_response(v_project, outline_sha, convert_to(jsonb_build_object(
    'outline_sha256', outline_sha, 'responses', jsonb_build_array(jsonb_build_object(
      'slot_id','blank','chapter_id','response','status','no_evidence','text','【待人工补充】','evidence_ids','[]'::jsonb)))::text, 'UTF8'), NULL);
  SELECT body INTO v_placeholder FROM bid_response_slots WHERE response_sha256 = (response->>'response_sha256')::kb_sha256;
  IF v_placeholder <> '【待人工补充】' THEN RAISE EXCEPTION 'response placeholder lost'; END IF;

  -- Each published domain object atomically gains a durable owner and consumes staging.
  PERFORM kb_object_upload_stage(stage_source, 'objects/' || v_source_sha, v_source_sha, 'application/pdf', 12, NULL);
  PERFORM kb_bid_v2_publish_document(v_document, v_project, 'source.pdf', 'application/pdf',
    'objects/' || v_source_sha, v_source_sha, 12, stage_source, NULL);
  PERFORM kb_object_upload_stage(stage_docx, 'objects/' || v_docx_sha, v_docx_sha, docx_media, 15, NULL);
  docx := kb_bid_v2_put_docx_version(v_project, outline_sha, 'objects/' || v_docx_sha,
    v_docx_sha, 15, 0, stage_docx, NULL);
  PERFORM kb_object_upload_stage(stage_export, 'objects/' || v_export_sha, v_export_sha, 'application/pdf', 18, NULL);
  exported := kb_bid_v2_publish_submission_export(v_project, outline_sha, (response->>'response_sha256')::kb_sha256,
    'objects/' || v_export_sha, v_export_sha, 'application/pdf', 18, stage_export, NULL);
  IF exported->>'docx_version_id' <> docx->>'version_id' THEN RAISE EXCEPTION 'export version mismatch'; END IF;
  PERFORM kb_object_upload_abandon(stage_source, NULL);
  PERFORM kb_object_upload_abandon(stage_docx, NULL);
  PERFORM kb_object_upload_expire_one(stage_export);
  IF EXISTS (SELECT 1 FROM object_upload_staging WHERE id IN (stage_source,stage_docx,stage_export))
     OR (SELECT count(*) FROM object_owner_references WHERE object_ref IN
       ('objects/' || v_source_sha,'objects/' || v_docx_sha,'objects/' || v_export_sha)) <> 3
     OR EXISTS (SELECT 1 FROM object_registry WHERE digest IN (v_source_sha,v_docx_sha,v_export_sha) AND state <> 'available')
     OR EXISTS (SELECT 1 FROM object_deletion_artifacts WHERE digest IN (v_source_sha,v_docx_sha,v_export_sha)) THEN
    RAISE EXCEPTION 'published objects lost durable ownership';
  END IF;

  -- An independent owner can disappear without deleting the published owner.
  PERFORM kb_object_upload_stage(stage_extra, 'objects/' || v_docx_sha, v_docx_sha, docx_media, 15, NULL);
  deletion := kb_object_upload_expire_one(stage_extra);
  IF deletion->>'state' <> 'not_current' OR NOT EXISTS (SELECT 1 FROM object_upload_staging WHERE id=stage_extra) THEN
    RAISE EXCEPTION 'unexpired staging was collected';
  END IF;
  UPDATE object_upload_staging SET created_at=clock_timestamp()-interval '2 days', expires_at=clock_timestamp()-interval '1 day' WHERE id=stage_extra;
  deletion := kb_object_upload_expire_one(stage_extra);
  IF deletion->'deletion' <> 'null'::jsonb
     OR NOT EXISTS (SELECT 1 FROM object_registry WHERE digest=v_docx_sha AND state='available') THEN
    RAISE EXCEPTION 'expiry of one owner deleted another owner';
  END IF;
  PERFORM kb_object_publish_reference(NULL, 'objects/' || v_docx_sha, v_docx_sha, docx_media, 15,
    'fixture_domain', stage_extra, 'payload', NULL);
  deletion := kb_object_reference_remove('objects/' || v_docx_sha,'fixture_domain',stage_extra,'payload',stage_extra);
  IF deletion IS NOT NULL THEN RAISE EXCEPTION 'domain removal deleted shared DOCX'; END IF;

  -- Missing, unavailable, wrong metadata, expired staging, and failed CAS never move the head or leak a durable owner.
  BEGIN
    PERFORM kb_bid_v2_put_docx_version(v_project, outline_sha, 'objects/' || v_bad_sha, v_bad_sha, 16, 1, NULL, NULL);
    RAISE EXCEPTION 'bare missing digest accepted';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  PERFORM kb_object_upload_stage(stage_bad, 'objects/' || v_bad_sha, v_bad_sha, docx_media, 16, NULL);
  BEGIN
    PERFORM kb_bid_v2_put_docx_version(v_project, outline_sha, 'objects/' || v_bad_sha, v_bad_sha, 16, 0, stage_bad, NULL);
    RAISE EXCEPTION 'stale DOCX revision accepted';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  BEGIN
    PERFORM kb_bid_v2_put_docx_version(v_project, outline_sha, 'objects/' || v_bad_sha, v_bad_sha, 99, 1, stage_bad, NULL);
    RAISE EXCEPTION 'wrong object length accepted';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  UPDATE object_upload_staging SET created_at=clock_timestamp()-interval '2 days', expires_at=clock_timestamp()-interval '1 day' WHERE id=stage_bad;
  BEGIN
    PERFORM kb_bid_v2_put_docx_version(v_project, outline_sha, 'objects/' || v_bad_sha, v_bad_sha, 16, 1, stage_bad, NULL);
    RAISE EXCEPTION 'expired staging publication accepted';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  IF (SELECT version_id FROM bid_docx_current WHERE project_id=v_project AND outline_sha256=outline_sha) <> (docx->>'version_id')::uuid
     OR (SELECT count(*) FROM bid_docx_versions WHERE project_id=v_project) <> 1
     OR (SELECT count(*) FROM object_owner_references WHERE object_ref='objects/' || v_bad_sha) <> 1
     OR NOT EXISTS (SELECT 1 FROM object_upload_staging WHERE id=stage_bad) THEN
    RAISE EXCEPTION 'failed publication was not atomic';
  END IF;
  deletion := kb_object_upload_expire_one(stage_bad)->'deletion';
  IF deletion IS NULL OR deletion = 'null'::jsonb THEN RAISE EXCEPTION 'abandoned bytes not collectible'; END IF;
  IF kb_retention_preflight(stage_bad,'objects/' || v_bad_sha,v_bad_sha,16)->>'state' <> 'current' THEN
    RAISE EXCEPTION 'unowned expired bytes did not pass GC fence';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_put_docx_version(v_project, outline_sha, 'objects/' || v_bad_sha, v_bad_sha, 16, 1, NULL, NULL);
    RAISE EXCEPTION 'deleting object revived';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  -- This fixture checks database GC authorization, not a fabricated physical delete receipt.
  IF has_table_privilege('kb_runtime_worker','bid_outline_runs','UPDATE')
     OR has_table_privilege('kb_runtime_worker','bid_outline_artifacts','INSERT')
     OR has_table_privilege('kb_runtime_api','bid_documents','INSERT')
     OR has_function_privilege('kb_runtime_worker','kb_object_reference_remove(kb_object_ref,text,uuid,text,uuid)','EXECUTE') THEN
    RAISE EXCEPTION 'runtime can bypass publication fence';
  END IF;
END
$acceptance$;

DO $frozen_manifest$
DECLARE
  v_user uuid := gen_random_uuid();
  v_project uuid := gen_random_uuid();
  v_publication uuid := gen_random_uuid();
  v_stage uuid := gen_random_uuid();
  v_source_stage uuid := gen_random_uuid();
  v_image_stage uuid := gen_random_uuid();
  v_sha kb_sha256;
  v_source_sha kb_sha256 := encode(public.digest('raw tender','sha256'),'hex')::kb_sha256;
  v_image_sha kb_sha256 := encode(public.digest(v_image_stage::text,'sha256'),'hex')::kb_sha256;
  v_bytes bytea;
  source_key text := 'source:' || encode(public.digest('doc1','sha256'),'hex');
  image_key text := 'image:' || encode(public.digest('4:doc16:image1','sha256'),'hex');
  manifest jsonb;
  wrong_source jsonb;
  result kb_sha256;
  expiry_result jsonb;
  v_cancel uuid := gen_random_uuid();
  v_cancel_sha kb_sha256 := encode(public.digest(v_cancel::text,'sha256'),'hex')::kb_sha256;
BEGIN
  INSERT INTO users(id,email) VALUES(v_user,v_user::text||'@example.invalid');
  INSERT INTO bid_projects(id,owner_user_id,title,status) VALUES(v_project,v_user,'frozen object manifest','open');
  v_bytes := convert_to(jsonb_build_object('schema_version',2,'project_id',v_project,'document_set_id','set',
    'documents',jsonb_build_array(jsonb_build_object('document_id','doc1','document_revision',v_source_sha,
      'source_contract',jsonb_build_object('document_revision',v_source_sha))),
    'source_units',jsonb_build_array(jsonb_build_object('document_id','doc1',
      'locator',jsonb_build_object('unit_id','image1','image_ref','objects/'||v_image_sha))))::text,'UTF8');
  v_sha:=kb_bid_v2_sha256_bytes(v_bytes);
  PERFORM kb_object_upload_stage(v_stage,'objects/'||v_sha,v_sha,'application/json',octet_length(v_bytes),NULL);
  PERFORM kb_object_upload_stage(v_source_stage,'objects/'||v_source_sha,v_source_sha,'application/pdf',10,NULL);
  PERFORM kb_object_upload_stage(v_image_stage,'objects/'||v_image_sha,v_image_sha,'image/png',200,NULL);
  manifest:=jsonb_build_array(
    jsonb_build_object('staging_id',v_stage,'occurrence','frozen-json','object_ref','objects/'||v_sha,'digest',v_sha,'media_type','application/json','byte_length',octet_length(v_bytes)),
    jsonb_build_object('staging_id',v_source_stage,'occurrence',source_key,'object_ref','objects/'||v_source_sha,'digest',v_source_sha,'media_type','application/pdf','byte_length',10),
    jsonb_build_object('staging_id',v_image_stage,'occurrence',image_key,'object_ref','objects/'||v_image_sha,'digest',v_image_sha,'media_type','image/png','byte_length',200));
  -- Both objects have valid independent metadata, but the wrong original cannot
  -- be substituted for the parser's frozen document revision.
  wrong_source := jsonb_set(manifest,'{1}',(manifest->2)||jsonb_build_object('occurrence',source_key));
  BEGIN
    PERFORM kb_bid_v2_publish_frozen_input(v_project,v_sha,v_bytes,'set',v_publication,wrong_source,NULL);
    RAISE EXCEPTION 'different valid original bytes accepted';
  EXCEPTION WHEN check_violation THEN
    IF SQLERRM NOT LIKE '%staged source bytes%' THEN RAISE; END IF;
  END;
  -- A late manifest failure must roll back earlier owner transfers and registration.
  BEGIN
    PERFORM kb_bid_v2_publish_frozen_input(v_project,v_sha,v_bytes,'set',v_publication,
      jsonb_set(manifest,'{2,byte_length}','201'),NULL);
    RAISE EXCEPTION 'wrong frozen manifest metadata accepted';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  IF EXISTS(SELECT 1 FROM bid_frozen_inputs WHERE project_id=v_project)
    OR (SELECT count(*) FROM object_upload_staging WHERE id IN(v_stage,v_source_stage,v_image_stage))<>3
    OR EXISTS(SELECT 1 FROM object_owner_references WHERE owner_kind='bid_frozen_input' AND owner_id=v_publication) THEN
    RAISE EXCEPTION 'failed frozen manifest left a partial snapshot';
  END IF;
  result:=kb_bid_v2_publish_frozen_input(v_project,v_sha,v_bytes,'set',v_publication,manifest,NULL);
  IF result IS DISTINCT FROM v_sha
    OR EXISTS(SELECT 1 FROM object_upload_staging WHERE id IN(v_stage,v_source_stage,v_image_stage))
    OR (SELECT count(*) FROM bid_frozen_input_objects WHERE project_id=v_project)<>3
    OR (SELECT count(*) FROM object_owner_references WHERE owner_kind='bid_frozen_input' AND owner_id=v_publication)<>3 THEN
    RAISE EXCEPTION 'frozen manifest did not atomically acquire all owners';
  END IF;
  result:=kb_bid_v2_publish_frozen_input(v_project,v_sha,v_bytes,'set',v_publication,manifest,NULL);
  IF result IS DISTINCT FROM v_sha THEN RAISE EXCEPTION 'frozen ACK replay changed identity'; END IF;
  BEGIN
    PERFORM kb_bid_v2_publish_frozen_input(v_project,v_sha,v_bytes,'set',v_publication,
      jsonb_build_array(manifest->0),NULL);
    RAISE EXCEPTION 'frozen replay dropped image owner';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  PERFORM kb_object_upload_request_expiry(v_stage,NULL);
  PERFORM kb_object_upload_expire_one(v_image_stage);
  IF EXISTS(SELECT 1 FROM object_registry WHERE digest IN(v_sha,v_source_sha,v_image_sha) AND state<>'available') THEN
    RAISE EXCEPTION 'post-publication cleanup deleted frozen evidence';
  END IF;
  PERFORM kb_object_upload_stage(v_cancel,'objects/'||v_cancel_sha,v_cancel_sha,'image/png',10,NULL);
  PERFORM kb_object_upload_request_expiry(v_cancel,NULL);
  IF NOT EXISTS(SELECT 1 FROM kb_object_upload_expiry_candidates() id WHERE id=v_cancel)
      OR NOT EXISTS(SELECT 1 FROM object_owner_references WHERE owner_kind='object_upload_staging' AND owner_id=v_cancel) THEN
    RAISE EXCEPTION 'cancel did not retain durable expiry work';
  END IF;
  -- This SQL fixture has no retention process. Exercise its exact database
  -- consumer after proving the producer left durable work, rather than leaving
  -- an intentional cancelled upload behind in the enclosing stack test.
  expiry_result:=kb_object_upload_expire_one(v_cancel);
  IF expiry_result->>'state'<>'expired' OR expiry_result->'deletion'='null'::jsonb
      OR EXISTS(SELECT 1 FROM object_upload_staging WHERE id=v_cancel)
      OR EXISTS(SELECT 1 FROM object_owner_references WHERE owner_kind='object_upload_staging' AND owner_id=v_cancel) THEN
    RAISE EXCEPTION 'retention did not consume cancelled staging';
  END IF;
END
$frozen_manifest$;

DO $budget_resume$
DECLARE
  v_user uuid := gen_random_uuid();
  v_project uuid := gen_random_uuid();
  v_run uuid := gen_random_uuid();
  v_token uuid := gen_random_uuid();
  v_sha kb_sha256 := encode(public.digest(v_run::text,'sha256'),'hex')::kb_sha256;
  v_epoch bigint;
  v_checkpoint jsonb := '{"config_sha256":"old-config","done":false,"journal":{"budget":{"physical_calls":5,"reserved_input_tokens":100,"reserved_output_tokens":30,"paused_reason":"limit"},"pending":{"physical_attempts":2}},"turn":3}';
  grant_checkpoint jsonb;
  receipt jsonb;
BEGIN
  INSERT INTO users(id,email) VALUES(v_user,v_user::text||'@example.invalid');
  INSERT INTO bid_projects(id,owner_user_id,title,status) VALUES(v_project,v_user,'paused budget','open');
  PERFORM kb_bid_v2_register_frozen_input(v_project,v_sha,'set');
  v_epoch:=(kb_bid_v2_outline_claim(v_run,v_project,v_sha,v_token,120)->>'lease_epoch')::bigint;
  PERFORM kb_bid_v2_outline_checkpoint(v_run,v_token,v_epoch,v_checkpoint);
  PERFORM kb_bid_v2_outline_pause_budget(v_run,v_token,v_epoch);
  receipt:=kb_bid_v2_outline_paused_checkpoint(v_run);
  grant_checkpoint:=jsonb_set(jsonb_set(v_checkpoint,'{config_sha256}','"granted-config"'),'{journal,budget,paused_reason}','null');
  BEGIN
    PERFORM kb_bid_v2_outline_resume_budget(v_run,(receipt->>'checkpoint_sha256')::kb_sha256,
      jsonb_set(grant_checkpoint,'{journal,budget,physical_calls}','0'));
    RAISE EXCEPTION 'budget grant erased physical accounting';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
  BEGIN
    PERFORM kb_bid_v2_outline_resume_budget(v_run,repeat('f',64)::kb_sha256,grant_checkpoint);
    RAISE EXCEPTION 'stale budget grant succeeded';
  EXCEPTION WHEN serialization_failure THEN NULL;
  END;
  PERFORM kb_bid_v2_outline_resume_budget(v_run,(receipt->>'checkpoint_sha256')::kb_sha256,grant_checkpoint);
  IF NOT EXISTS(SELECT 1 FROM bid_outline_runs WHERE id=v_run AND status='pending'
    AND lease_token IS NULL AND lease_until IS NULL AND checkpoint=grant_checkpoint) THEN
    RAISE EXCEPTION 'budget grant did not preserve pending work';
  END IF;
  IF (kb_bid_v2_outline_claim(v_run,v_project,v_sha,gen_random_uuid(),120)->>'lease_epoch')::bigint <= v_epoch THEN
    RAISE EXCEPTION 'resumed budget reused acquisition epoch';
  END IF;
END
$budget_resume$;
