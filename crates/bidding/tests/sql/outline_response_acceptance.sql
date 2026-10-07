-- Fresh-baseline acceptance for outline, response, DOCX, and export.
-- Re-runnable: rows for this project are removed first.
DO $acceptance$
DECLARE
  v_user_id uuid := '10000000-0000-4000-8000-00000000aa01';
  v_project_id uuid := '10000000-0000-4000-8000-00000000aa10';
  v_document_id uuid := '10000000-0000-4000-8000-00000000aa11';
  v_run_id uuid := '10000000-0000-4000-8000-00000000aa20';
  v_bad_run_id uuid := '10000000-0000-4000-8000-00000000aa22';
  lease_token uuid := '10000000-0000-4000-8000-00000000aa21';
  other_token uuid := '10000000-0000-4000-8000-00000000aa23';
  editor_key uuid := '10000000-0000-4000-8000-00000000aa24';
  doc_bytes bytea := convert_to('tender-bytes', 'UTF8');
  doc_sha kb_sha256 := encode(public.digest(doc_bytes, 'sha256'), 'hex')::kb_sha256;
  frozen_sha kb_sha256 := repeat('ab', 32)::kb_sha256;
  docx_sha kb_sha256 := repeat('cd', 32)::kb_sha256;
  export_sha kb_sha256 := repeat('ef', 32)::kb_sha256;
  artifact jsonb;
  bad_artifact jsonb;
  published jsonb;
  replayed jsonb;
  response jsonb;
  response_replay jsonb;
  docx jsonb;
  opened jsonb;
  exported jsonb;
  outline_sha kb_sha256;
  chapter_count integer;
  v_placeholder text;
  v_evidence jsonb;
BEGIN
  UPDATE bid_outline_runs SET outline_sha256 = NULL
  WHERE project_id = v_project_id;
  DELETE FROM bid_submission_exports WHERE project_id = v_project_id;
  DELETE FROM bid_docx_current WHERE project_id = v_project_id;
  DELETE FROM bid_docx_versions WHERE project_id = v_project_id AND revision > 1;
  DELETE FROM bid_docx_versions WHERE project_id = v_project_id;
  DELETE FROM bid_response_slots
  WHERE response_sha256 IN (SELECT sha256 FROM bid_response_sets WHERE project_id = v_project_id);
  DELETE FROM bid_response_sets WHERE project_id = v_project_id;
  DELETE FROM bid_outline_attachment_bindings
  WHERE outline_sha256 IN (SELECT sha256 FROM bid_outline_artifacts WHERE project_id = v_project_id);
  DELETE FROM bid_outline_template_slots
  WHERE outline_sha256 IN (SELECT sha256 FROM bid_outline_artifacts WHERE project_id = v_project_id);
  DELETE FROM bid_outline_chapters
  WHERE outline_sha256 IN (SELECT sha256 FROM bid_outline_artifacts WHERE project_id = v_project_id)
    AND parent_id IS NOT NULL;
  DELETE FROM bid_outline_chapters
  WHERE outline_sha256 IN (SELECT sha256 FROM bid_outline_artifacts WHERE project_id = v_project_id);
  DELETE FROM bid_outline_artifacts WHERE project_id = v_project_id;
  DELETE FROM bid_outline_runs WHERE project_id = v_project_id OR id IN (v_run_id, v_bad_run_id);
  DELETE FROM bid_documents WHERE project_id = v_project_id;
  DELETE FROM bid_projects WHERE id = v_project_id;

  INSERT INTO users(id, email)
  VALUES (v_user_id, 'outline-acceptance@example.invalid')
  ON CONFLICT (id) DO NOTHING;
  INSERT INTO bid_projects(id, owner_user_id, title, status)
  VALUES (v_project_id, v_user_id, 'outline acceptance', 'open');
  INSERT INTO bid_documents(
    id, project_id, file_name, media_type, object_ref, content_sha256, byte_length
  ) VALUES (
    v_document_id, v_project_id, 'tender.pdf', 'application/pdf',
    ('objects/' || doc_sha)::kb_object_ref, doc_sha, octet_length(doc_bytes)
  );

  bad_artifact := jsonb_build_object(
    'chapters', jsonb_build_array(jsonb_build_object(
      'id', 'ch-1', 'parent_id', '', 'order', 0, 'title', '组章', 'purpose', 'group'
    )),
    'templates', jsonb_build_array(jsonb_build_object(
      'slot_id', 'slot-bad', 'chapter_id', 'ch-1', 'kind', 'bidder_blank',
      'text', '', 'response_required', true, 'match_query', '不应出现'
    ))
  );
  BEGIN
    PERFORM kb_bid_v2_publish_outline(
      v_bad_run_id, v_project_id, frozen_sha, convert_to(bad_artifact::text, 'UTF8'), '[]'::jsonb, NULL
    );
    RAISE EXCEPTION 'group chapter response was accepted';
  EXCEPTION
    WHEN SQLSTATE '23514' THEN
      IF SQLERRM NOT LIKE '%group chapter cannot carry a knowledge response%' THEN
        RAISE;
      END IF;
  END;
  IF EXISTS (SELECT 1 FROM bid_outline_artifacts WHERE run_id = v_bad_run_id) THEN
    RAISE EXCEPTION 'rejected outline left an artifact';
  END IF;

  artifact := jsonb_build_object(
    'chapters', jsonb_build_array(
      jsonb_build_object('id', 'ch-1', 'parent_id', '', 'order', 0, 'title', '投标文件', 'purpose', 'group'),
      jsonb_build_object('id', 'ch-2', 'parent_id', 'ch-1', 'order', 0, 'title', '投标函', 'purpose', 'response')
    ),
    'templates', jsonb_build_array(
      jsonb_build_object(
        'slot_id', 'slot-fixed', 'chapter_id', 'ch-1', 'kind', 'fixed_text',
        'text', '须知', 'response_required', false, 'match_query', ''
      ),
      jsonb_build_object(
        'slot_id', 'slot-blank', 'chapter_id', 'ch-2', 'kind', 'bidder_blank',
        'text', '', 'response_required', true, 'match_query', '投标函'
      )
    )
  );
  INSERT INTO bid_outline_runs(id, project_id, status, checkpoint)
  VALUES (v_run_id, v_project_id, 'pending', '{}'::jsonb);
  PERFORM kb_bid_v2_outline_claim(v_run_id, lease_token, 120);
  BEGIN
    PERFORM kb_bid_v2_outline_claim(v_run_id, other_token, 120);
    RAISE EXCEPTION 'second lease was accepted';
  EXCEPTION
    WHEN SQLSTATE '40001' THEN
      IF SQLERRM NOT LIKE '%published or leased%' THEN
        RAISE;
      END IF;
  END;

  published := kb_bid_v2_publish_outline(
    v_run_id, v_project_id, frozen_sha, convert_to(artifact::text, 'UTF8'),
    jsonb_build_array(jsonb_build_object('form_id', 'form-1', 'chapter_id', 'ch-2')),
    NULL
  );
  IF published->>'replayed' IS DISTINCT FROM 'false' THEN
    RAISE EXCEPTION 'first publish was a replay: %', published;
  END IF;
  outline_sha := (published->>'outline_sha256')::kb_sha256;
  replayed := kb_bid_v2_publish_outline(
    v_run_id, v_project_id, frozen_sha, convert_to(artifact::text, 'UTF8'),
    jsonb_build_array(jsonb_build_object('form_id', 'form-1', 'chapter_id', 'ch-2')),
    NULL
  );
  IF replayed->>'replayed' IS DISTINCT FROM 'true'
     OR (replayed->>'outline_sha256')::kb_sha256 IS DISTINCT FROM outline_sha THEN
    RAISE EXCEPTION 'replay did not return the same outline: %', replayed;
  END IF;
  SELECT count(*) INTO chapter_count
  FROM bid_outline_chapters WHERE outline_sha256 = outline_sha;
  IF chapter_count <> 2 THEN
    RAISE EXCEPTION 'chapter count %', chapter_count;
  END IF;
  BEGIN
    PERFORM kb_bid_v2_outline_claim(v_run_id, lease_token, 120);
    RAISE EXCEPTION 'published run was claimed again';
  EXCEPTION
    WHEN SQLSTATE '40001' THEN
      NULL;
  END;

  response := kb_bid_v2_publish_response(
    v_project_id, outline_sha, convert_to(jsonb_build_object(
      'outline_sha256', outline_sha,
      'responses', jsonb_build_array(jsonb_build_object(
        'slot_id', 'slot-blank', 'chapter_id', 'ch-2', 'status', 'no_evidence',
        'text', '【待人工补充】', 'evidence_ids', '[]'::jsonb
      ))
    )::text, 'UTF8'), NULL
  );
  IF response->>'replayed' IS DISTINCT FROM 'false' THEN
    RAISE EXCEPTION 'first response was a replay: %', response;
  END IF;
  SELECT slot.body, slot.evidence INTO v_placeholder, v_evidence
  FROM bid_response_slots slot
  WHERE slot.response_sha256 = (response->>'response_sha256')::kb_sha256
    AND slot.slot_id = 'slot-blank';
  IF v_placeholder IS DISTINCT FROM '【待人工补充】' OR v_evidence <> '[]'::jsonb THEN
    RAISE EXCEPTION 'unmatched slot is not the manual placeholder';
  END IF;
  response_replay := kb_bid_v2_publish_response(
    v_project_id, outline_sha, convert_to(jsonb_build_object(
      'outline_sha256', outline_sha,
      'responses', jsonb_build_array(jsonb_build_object(
        'slot_id', 'slot-blank', 'chapter_id', 'ch-2', 'status', 'no_evidence',
        'text', '【待人工补充】', 'evidence_ids', '[]'::jsonb
      ))
    )::text, 'UTF8'), NULL
  );
  IF response_replay->>'replayed' IS DISTINCT FROM 'true' THEN
    RAISE EXCEPTION 'response replay was not recognized: %', response_replay;
  END IF;

  docx := kb_bid_v2_put_docx_version(
    v_project_id, outline_sha, ('objects/' || docx_sha)::kb_object_ref, docx_sha, 12, 0, NULL
  );
  IF (docx->>'revision')::bigint <> 1 THEN
    RAISE EXCEPTION 'first docx revision %', docx;
  END IF;
  BEGIN
    PERFORM kb_bid_v2_put_docx_version(
      v_project_id, outline_sha, ('objects/' || docx_sha)::kb_object_ref, docx_sha, 12, 0, NULL
    );
    RAISE EXCEPTION 'stale docx revision was accepted';
  EXCEPTION
    WHEN SQLSTATE '40001' THEN
      IF SQLERRM NOT LIKE '%DOCX_VERSION_CAS_MISMATCH%' THEN
        RAISE;
      END IF;
  END;
  opened := kb_bid_v2_open_docx_editor(v_project_id, outline_sha, editor_key, NULL);
  IF (opened->>'editor_key')::uuid IS DISTINCT FROM editor_key THEN
    RAISE EXCEPTION 'editor lock %', opened;
  END IF;
  exported := kb_bid_v2_publish_submission_export(
    v_project_id, outline_sha, (response->>'response_sha256')::kb_sha256,
    ('objects/' || export_sha)::kb_object_ref, export_sha, NULL
  );
  IF exported->>'docx_version_id' IS DISTINCT FROM docx->>'version_id' THEN
    RAISE EXCEPTION 'export is not bound to the current docx: %', exported;
  END IF;
END
$acceptance$;
