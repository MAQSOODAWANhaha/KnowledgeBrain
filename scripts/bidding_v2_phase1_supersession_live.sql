\set ON_ERROR_STOP on
BEGIN;
DO $$
DECLARE
  project_id uuid:='10000000-0000-4000-8000-000000000010';
  actor kb_actor_identity:='user:10000000-0000-4000-8000-000000000001';
  current_set bid_requirement_set_current%ROWTYPE;
  req1 bid_requirement_revision_artifacts%ROWTYPE; req2 bid_requirement_revision_artifacts%ROWTYPE;
  value jsonb; req2_target uuid; req2_later uuid; req1_old uuid; edge_id uuid;
  edge_lineage uuid:=gen_random_uuid(); edge_sha kb_sha256;
  body bytea; body_sha kb_sha256;
BEGIN
  SELECT * INTO STRICT current_set FROM bid_requirement_set_current WHERE scope_id=project_id;
  SELECT requirement.* INTO STRICT req1 FROM bid_requirement_set_items item
    JOIN bid_requirement_revision_artifacts requirement ON requirement.id=item.requirement_revision_id
    WHERE item.requirement_set_id=current_set.artifact_id ORDER BY item.ordinal LIMIT 1;
  SELECT requirement.* INTO STRICT req2 FROM bid_requirement_set_items item
    JOIN bid_requirement_revision_artifacts requirement ON requirement.id=item.requirement_revision_id
    WHERE item.requirement_set_id=current_set.artifact_id ORDER BY item.ordinal OFFSET 1 LIMIT 1;

  body:=convert_to('{"patch":"target-a"}','UTF8'); body_sha:=kb_bid_v2_sha256_bytes(body);
  value:=kb_bid_v2_patch_requirement(project_id,req2.id,current_set.artifact_id,current_set.artifact_sha256,
    req2.requirement_kind,req2.requiredness,req2.compliance_policy,req2.lifecycle,convert_from(req2.text_utf8,'UTF8'),
    req2.fulfillment_expr,'{"fragments":["a"]}',actor,'partial-target-a',body,body_sha);
  req2_target:=(value->>'requirement_revision_id')::uuid;

  SELECT * INTO STRICT current_set FROM bid_requirement_set_current WHERE scope_id=project_id;
  SELECT * INTO STRICT req2 FROM bid_requirement_revision_artifacts WHERE id=req2_target;
  body:=convert_to('{"patch":"later-b"}','UTF8'); body_sha:=kb_bid_v2_sha256_bytes(body);
  value:=kb_bid_v2_patch_requirement(project_id,req2.id,current_set.artifact_id,current_set.artifact_sha256,
    req2.requirement_kind,req2.requiredness,req2.compliance_policy,req2.lifecycle,convert_from(req2.text_utf8,'UTF8'),
    req2.fulfillment_expr,'{"fragments":["b"]}',actor,'partial-later-b',body,body_sha);
  req2_later:=(value->>'requirement_revision_id')::uuid;

  SELECT * INTO STRICT current_set FROM bid_requirement_set_current WHERE scope_id=project_id;
  body:=convert_to('{"patch":"old-ab"}','UTF8'); body_sha:=kb_bid_v2_sha256_bytes(body);
  value:=kb_bid_v2_patch_requirement(project_id,req1.id,current_set.artifact_id,current_set.artifact_sha256,
    req1.requirement_kind,req1.requiredness,req1.compliance_policy,req1.lifecycle,convert_from(req1.text_utf8,'UTF8'),
    req1.fulfillment_expr,'{"fragments":["a","b"]}',actor,'partial-old-ab',body,body_sha);
  req1_old:=(value->>'requirement_revision_id')::uuid;

  SELECT * INTO STRICT current_set FROM bid_requirement_set_current WHERE scope_id=project_id;
  body:=convert_to('{"supersede":"a"}','UTF8'); body_sha:=kb_bid_v2_sha256_bytes(body);
  value:=kb_bid_v2_publish_requirement_supersession(project_id,edge_lineage,req1_old,req2_target,
    '{"fragments":["a"]}',false,NULL,NULL,actor,'partial-supersede-a',body,body_sha);
  edge_id:=(value->>'artifact_id')::uuid; edge_sha:=(value->>'sha256')::kb_sha256;

  SELECT * INTO STRICT current_set FROM bid_requirement_set_current WHERE scope_id=project_id;
  IF NOT EXISTS (SELECT 1 FROM bid_requirement_set_items WHERE requirement_set_id=current_set.artifact_id
      AND requirement_revision_id=req1_old AND effective_applicability='{"fragments":["b"]}'::jsonb)
     OR NOT EXISTS (SELECT 1 FROM bid_requirement_set_items WHERE requirement_set_id=current_set.artifact_id
      AND requirement_revision_id=req2_target AND effective_applicability='{"fragments":["a"]}'::jsonb)
     OR NOT EXISTS (SELECT 1 FROM bid_requirement_set_items WHERE requirement_set_id=current_set.artifact_id
      AND requirement_revision_id=req2_later AND effective_applicability='{"fragments":["b"]}'::jsonb)
     OR NOT EXISTS (SELECT 1 FROM bid_requirement_supersession_revision_artifacts
      WHERE id=edge_id AND old_source_unit_revision_ids<>'{}'::uuid[]
        AND new_source_unit_revision_ids<>'{}'::uuid[]
        AND convert_from(canonical_payload,'UTF8')::jsonb ? 'old_source_unit_revision_ids'
        AND convert_from(canonical_payload,'UTF8')::jsonb ? 'new_source_unit_revision_ids') THEN
    RAISE EXCEPTION 'partial applicability supersession projection mismatch';
  END IF;

  body:=convert_to('{"supersede":"withdraw-a"}','UTF8'); body_sha:=kb_bid_v2_sha256_bytes(body);
  value:=kb_bid_v2_publish_requirement_supersession(project_id,edge_lineage,req1_old,req2_target,
    '{"fragments":["a"]}',true,edge_id,edge_sha,actor,'partial-withdraw-a',body,body_sha);
  edge_id:=(value->>'artifact_id')::uuid; edge_sha:=(value->>'sha256')::kb_sha256;
  SELECT * INTO STRICT current_set FROM bid_requirement_set_current WHERE scope_id=project_id;
  IF NOT EXISTS (SELECT 1 FROM bid_requirement_set_items WHERE requirement_set_id=current_set.artifact_id
      AND requirement_revision_id=req1_old AND effective_applicability='{"fragments":["a","b"]}'::jsonb)
     OR EXISTS (SELECT 1 FROM bid_requirement_set_items WHERE requirement_set_id=current_set.artifact_id
      AND requirement_revision_id=req2_target)
     OR (value->>'tombstone')::boolean IS DISTINCT FROM true THEN
    RAISE EXCEPTION 'supersession withdrawal did not restore the effective set';
  END IF;

  body:=convert_to('{"supersede":"reestablish-a"}','UTF8'); body_sha:=kb_bid_v2_sha256_bytes(body);
  value:=kb_bid_v2_publish_requirement_supersession(project_id,edge_lineage,req1_old,req2_target,
    '{"fragments":["a"]}',false,edge_id,edge_sha,actor,'partial-reestablish-a',body,body_sha);
  SELECT * INTO STRICT current_set FROM bid_requirement_set_current WHERE scope_id=project_id;
  IF NOT EXISTS (SELECT 1 FROM bid_requirement_set_items WHERE requirement_set_id=current_set.artifact_id
      AND requirement_revision_id=req1_old AND effective_applicability='{"fragments":["b"]}'::jsonb)
     OR NOT EXISTS (SELECT 1 FROM bid_requirement_set_items WHERE requirement_set_id=current_set.artifact_id
      AND requirement_revision_id=req2_target AND effective_applicability='{"fragments":["a"]}'::jsonb)
     OR (value->>'tombstone')::boolean IS DISTINCT FROM false THEN
    RAISE EXCEPTION 'supersession re-establishment did not reapply the effective set';
  END IF;
END $$;
ROLLBACK;
SELECT 'V2 Phase 1 partial applicability supersession: PASS';
