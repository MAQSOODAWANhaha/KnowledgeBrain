\set ON_ERROR_STOP on

-- Phase 0 V2 live contract: all identities are deterministic test fixtures.
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

INSERT INTO bid_projects(id,owner_user_id,title,status) VALUES
 ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000001','contract-one','open'),
 ('00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000001','contract-two','open');
INSERT INTO bid_documents(id,project_id,file_name,media_type,byte_length,original_object_ref,original_sha256,parse_status) VALUES
 ('00000000-0000-4000-8000-000000000011','00000000-0000-4000-8000-000000000010','a.pdf','application/pdf',1,'objects/'||repeat('a',64),repeat('a',64),'ready'),
 ('00000000-0000-4000-8000-000000000012','00000000-0000-4000-8000-000000000010','b.pdf','application/pdf',1,'objects/'||repeat('b',64),repeat('b',64),'ready'),
 ('00000000-0000-4000-8000-000000000013','00000000-0000-4000-8000-000000000019','c.pdf','application/pdf',1,'objects/'||repeat('c',64),repeat('c',64),'ready');
INSERT INTO bid_document_role_revision_artifacts(id,project_id,document_id,revision,role,provenance,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-000000000021','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000011',1,'primary_tender','human_confirmed',convert_to('role1','UTF8'),encode(digest(convert_to('role1','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001'),
 ('00000000-0000-4000-8000-000000000029','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000013',1,'primary_tender','human_confirmed',convert_to('role2','UTF8'),encode(digest(convert_to('role2','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
INSERT INTO bid_authoring_contract_artifacts(id,contract_kind,schema_version,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-000000000190','converter',1,convert_to('converter1','UTF8'),encode(digest(convert_to('converter1','UTF8'),'sha256'),'hex'));
INSERT INTO bid_converted_source_artifacts(id,project_id,document_id,revision,source_object_ref,source_sha256,converter_contract_id,converter_contract_sha256,image_asset_set_sha256) VALUES
 ('00000000-0000-4000-8000-000000000031','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000011',1,'objects/'||repeat('d',64),repeat('d',64),'00000000-0000-4000-8000-000000000190',encode(digest(convert_to('converter1','UTF8'),'sha256'),'hex'),repeat('f',64)),
 ('00000000-0000-4000-8000-000000000032','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000012',1,'objects/'||repeat('1',64),repeat('1',64),'00000000-0000-4000-8000-000000000190',encode(digest(convert_to('converter1','UTF8'),'sha256'),'hex'),repeat('f',64)),
 ('00000000-0000-4000-8000-000000000039','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000013',1,'objects/'||repeat('2',64),repeat('2',64),'00000000-0000-4000-8000-000000000190',encode(digest(convert_to('converter1','UTF8'),'sha256'),'hex'),repeat('f',64));

-- DocumentSet: advance, replay, stale CAS, composite identity and append-only.
INSERT INTO bid_document_set_artifacts(id,project_id,revision,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-000000000041','00000000-0000-4000-8000-000000000010',1,convert_to('set1','UTF8'),encode(digest(convert_to('set1','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001'),
 ('00000000-0000-4000-8000-000000000042','00000000-0000-4000-8000-000000000010',2,convert_to('set2','UTF8'),encode(digest(convert_to('set2','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001'),
 ('00000000-0000-4000-8000-000000000049','00000000-0000-4000-8000-000000000019',1,convert_to('set9','UTF8'),encode(digest(convert_to('set9','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_document_set_items(document_set_id,project_id,document_id,ordinal,role_revision_id,source_revision_id,disposition)
    VALUES('00000000-0000-4000-8000-000000000041','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000012',0,'00000000-0000-4000-8000-000000000021','00000000-0000-4000-8000-000000000032','ready');
    RAISE EXCEPTION 'cross-document role unexpectedly accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
INSERT INTO bid_document_set_items(document_set_id,project_id,document_id,ordinal,role_revision_id,source_revision_id,disposition) VALUES
 ('00000000-0000-4000-8000-000000000041','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000011',0,'00000000-0000-4000-8000-000000000021','00000000-0000-4000-8000-000000000031','ready'),
 ('00000000-0000-4000-8000-000000000042','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000011',0,'00000000-0000-4000-8000-000000000021','00000000-0000-4000-8000-000000000031','ready');
DO $$ DECLARE h1 kb_sha256:=encode(digest(convert_to('set1','UTF8'),'sha256'),'hex'); DECLARE h2 kb_sha256:=encode(digest(convert_to('set2','UTF8'),'sha256'),'hex'); BEGIN
  IF NOT kb_bid_v2_advance_document_set('00000000-0000-4000-8000-000000000010',NULL,NULL,'00000000-0000-4000-8000-000000000041',h1) THEN RAISE EXCEPTION 'document initial advance failed'; END IF;
  IF NOT kb_bid_v2_advance_document_set('00000000-0000-4000-8000-000000000010',NULL,NULL,'00000000-0000-4000-8000-000000000041',h1) THEN RAISE EXCEPTION 'document replay failed'; END IF;
  IF kb_bid_v2_advance_document_set('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000099',repeat('9',64),'00000000-0000-4000-8000-000000000042',h2) THEN RAISE EXCEPTION 'document stale CAS accepted'; END IF;
  IF NOT kb_bid_v2_advance_document_set('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000041',h1,'00000000-0000-4000-8000-000000000042',h2) THEN RAISE EXCEPTION 'document advance failed'; END IF;
  BEGIN
    UPDATE bid_document_set_current SET artifact_id='00000000-0000-4000-8000-000000000041',artifact_sha256=h1,generation=3 WHERE scope_id='00000000-0000-4000-8000-000000000010';
    RAISE EXCEPTION 'document incoherent pointer accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
  BEGIN UPDATE bid_document_set_artifacts SET actor=actor WHERE id='00000000-0000-4000-8000-000000000041'; RAISE EXCEPTION 'document append-only update accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

-- SourceUnit and StructuredForm identities used by dispositions and typed bindings.
INSERT INTO bid_source_unit_lineages(id,project_id,document_id) VALUES
 ('00000000-0000-4000-8000-000000000051','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000011'),
 ('00000000-0000-4000-8000-000000000059','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000013');
INSERT INTO bid_source_unit_revision_artifacts(id,project_id,lineage_id,revision,document_id,source_revision_id,unit_kind,ordinal,source_locator,source_span_sha256,text_utf8,text_sha256,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-000000000052','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000051',1,'00000000-0000-4000-8000-000000000011','00000000-0000-4000-8000-000000000031','section',0,'{}',repeat('1',64),convert_to('unit1','UTF8'),encode(digest(convert_to('unit1','UTF8'),'sha256'),'hex'),convert_to('unit1','UTF8'),encode(digest(convert_to('unit1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-00000000005a','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000059',1,'00000000-0000-4000-8000-000000000013','00000000-0000-4000-8000-000000000039','form_region',0,'{}',repeat('2',64),convert_to('unit2','UTF8'),encode(digest(convert_to('unit2','UTF8'),'sha256'),'hex'),convert_to('unit2','UTF8'),encode(digest(convert_to('unit2','UTF8'),'sha256'),'hex'));
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_source_unit_revision_artifacts(id,project_id,lineage_id,revision,document_id,source_revision_id,unit_kind,ordinal,source_locator,source_span_sha256,text_utf8,text_sha256,canonical_payload,content_sha256)
    VALUES('00000000-0000-4000-8000-00000000005b','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000051',2,'00000000-0000-4000-8000-000000000012','00000000-0000-4000-8000-000000000032','section',0,'{}',repeat('3',64),convert_to('bad','UTF8'),encode(digest(convert_to('bad','UTF8'),'sha256'),'hex'),convert_to('bad','UTF8'),encode(digest(convert_to('bad','UTF8'),'sha256'),'hex'));
    RAISE EXCEPTION 'source composite identity unexpectedly accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
INSERT INTO bid_tender_structured_form_definition_artifacts(id,project_id,source_unit_revision_id,schema_version,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-000000000053','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000052',1,convert_to('form1','UTF8'),encode(digest(convert_to('form1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000058','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-00000000005a',1,convert_to('form2','UTF8'),encode(digest(convert_to('form2','UTF8'),'sha256'),'hex'));

-- DispositionSet: all CAS outcomes, composite identity and append-only.
INSERT INTO bid_source_unit_disposition_set_artifacts(id,project_id,document_set_id,document_set_sequence,revision,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-000000000061','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000041',1,1,convert_to('disp1','UTF8'),encode(digest(convert_to('disp1','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001'),
 ('00000000-0000-4000-8000-000000000062','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000042',2,2,convert_to('disp2','UTF8'),encode(digest(convert_to('disp2','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001'),
 ('00000000-0000-4000-8000-000000000063','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000042',2,3,convert_to('disp3','UTF8'),encode(digest(convert_to('disp3','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001'),
 ('00000000-0000-4000-8000-000000000069','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000049',1,1,convert_to('disp9','UTF8'),encode(digest(convert_to('disp9','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
INSERT INTO bid_source_unit_disposition_set_items(disposition_set_id,project_id,source_unit_revision_id,disposition) VALUES
 ('00000000-0000-4000-8000-000000000061','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000052','requirement'),
 ('00000000-0000-4000-8000-000000000062','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000052','requirement'),
 ('00000000-0000-4000-8000-000000000063','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000052','requirement');
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_source_unit_disposition_set_artifacts(id,project_id,document_set_id,document_set_sequence,revision,canonical_payload,content_sha256,actor)
    VALUES('00000000-0000-4000-8000-00000000006a','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000042',1,4,convert_to('bad-disp','UTF8'),encode(digest(convert_to('bad-disp','UTF8'),'sha256'),'hex'),'system:bid-extraction-worker');
    RAISE EXCEPTION 'disposition composite identity unexpectedly accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
DO $$ DECLARE h1 kb_sha256:=encode(digest(convert_to('disp1','UTF8'),'sha256'),'hex'); DECLARE h2 kb_sha256:=encode(digest(convert_to('disp2','UTF8'),'sha256'),'hex'); DECLARE h3 kb_sha256:=encode(digest(convert_to('disp3','UTF8'),'sha256'),'hex'); BEGIN
  IF NOT kb_bid_v2_advance_disposition_set('00000000-0000-4000-8000-000000000010',NULL,NULL,'00000000-0000-4000-8000-000000000061',h1) THEN RAISE EXCEPTION 'disposition initial advance failed'; END IF;
  IF NOT kb_bid_v2_advance_disposition_set('00000000-0000-4000-8000-000000000010',NULL,NULL,'00000000-0000-4000-8000-000000000061',h1) THEN RAISE EXCEPTION 'disposition replay failed'; END IF;
  IF kb_bid_v2_advance_disposition_set('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000099',repeat('9',64),'00000000-0000-4000-8000-000000000062',h2) THEN RAISE EXCEPTION 'disposition stale CAS accepted'; END IF;
  IF NOT kb_bid_v2_advance_disposition_set('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000061',h1,'00000000-0000-4000-8000-000000000062',h2) THEN RAISE EXCEPTION 'disposition advance two failed'; END IF;
  IF NOT kb_bid_v2_advance_disposition_set('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000062',h2,'00000000-0000-4000-8000-000000000063',h3) THEN RAISE EXCEPTION 'disposition advance three failed'; END IF;
  BEGIN UPDATE bid_source_unit_disposition_set_current SET artifact_id='00000000-0000-4000-8000-000000000061',artifact_sha256=h1,generation=4 WHERE scope_id='00000000-0000-4000-8000-000000000010'; RAISE EXCEPTION 'disposition incoherent pointer accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
  BEGIN UPDATE bid_source_unit_disposition_set_artifacts SET actor=actor WHERE id='00000000-0000-4000-8000-000000000061'; RAISE EXCEPTION 'disposition append-only update accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

-- RequirementSet monotonic publication explicitly starts with revision 7,
-- then receives revision 3 late, and finally advances to revision 11.
INSERT INTO bid_requirement_revision_artifacts(id,project_id,lineage_id,revision,requirement_kind,requiredness,compliance_policy,lifecycle,text_utf8,text_sha256,fulfillment_expr,applicability,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a1',1,'technical','mandatory','must_comply','current',convert_to('req1','UTF8'),encode(digest(convert_to('req1','UTF8'),'sha256'),'hex'),'{"kind":"need","need_occurrence_id":"00000000-0000-4000-8000-000000000201","channel":"narrative_content"}','{}',convert_to('req1','UTF8'),encode(digest(convert_to('req1','UTF8'),'sha256'),'hex'),'system:bid-extraction-worker'),
 ('00000000-0000-4000-8000-000000000072','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a1',2,'technical','mandatory','must_comply','current',convert_to('req2','UTF8'),encode(digest(convert_to('req2','UTF8'),'sha256'),'hex'),'{"kind":"need","need_occurrence_id":"00000000-0000-4000-8000-000000000201","channel":"narrative_content"}','{}',convert_to('req2','UTF8'),encode(digest(convert_to('req2','UTF8'),'sha256'),'hex'),'system:bid-extraction-worker'),
 ('00000000-0000-4000-8000-000000000079','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-0000000000a9',1,'technical','mandatory','must_comply','current',convert_to('req9','UTF8'),encode(digest(convert_to('req9','UTF8'),'sha256'),'hex'),'{"kind":"need","need_occurrence_id":"00000000-0000-4000-8000-000000000209","channel":"narrative_content"}','{}',convert_to('req9','UTF8'),encode(digest(convert_to('req9','UTF8'),'sha256'),'hex'),'system:bid-extraction-worker');
INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-000000000081','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000041',1,'00000000-0000-4000-8000-000000000061',1,3,convert_to('rset-old','UTF8'),encode(digest(convert_to('rset-old','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000082','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000042',2,'00000000-0000-4000-8000-000000000062',2,7,convert_to('rset-high','UTF8'),encode(digest(convert_to('rset-high','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000083','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000042',2,'00000000-0000-4000-8000-000000000063',3,11,convert_to('rset-new','UTF8'),encode(digest(convert_to('rset-new','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000089','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000049',1,'00000000-0000-4000-8000-000000000069',1,1,convert_to('rset9','UTF8'),encode(digest(convert_to('rset9','UTF8'),'sha256'),'hex'));
INSERT INTO bid_requirement_set_items(requirement_set_id,project_id,requirement_revision_id,ordinal) VALUES
 ('00000000-0000-4000-8000-000000000081','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000071',0),
 ('00000000-0000-4000-8000-000000000082','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000071',0),
 ('00000000-0000-4000-8000-000000000083','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000072',0);
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256)
    VALUES('00000000-0000-4000-8000-00000000008a','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000041',1,'00000000-0000-4000-8000-000000000069',1,12,convert_to('bad-rset','UTF8'),encode(digest(convert_to('bad-rset','UTF8'),'sha256'),'hex'));
    RAISE EXCEPTION 'requirement composite identity unexpectedly accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
DO $$ DECLARE h_old kb_sha256:=encode(digest(convert_to('rset-old','UTF8'),'sha256'),'hex'); DECLARE h_high kb_sha256:=encode(digest(convert_to('rset-high','UTF8'),'sha256'),'hex'); DECLARE h_new kb_sha256:=encode(digest(convert_to('rset-new','UTF8'),'sha256'),'hex'); BEGIN
  IF kb_bid_v2_publish_requirement_set('00000000-0000-4000-8000-000000000082',h_high)<>'published' THEN RAISE EXCEPTION 'requirement high initial publish failed'; END IF;
  IF kb_bid_v2_publish_requirement_set('00000000-0000-4000-8000-000000000082',h_high)<>'replayed' THEN RAISE EXCEPTION 'requirement identical replay failed'; END IF;
  IF kb_bid_v2_publish_requirement_set('00000000-0000-4000-8000-000000000081',h_old)<>'superseded' THEN RAISE EXCEPTION 'late older requirement was not superseded'; END IF;
  IF kb_bid_v2_publish_requirement_set('00000000-0000-4000-8000-000000000083',h_new)<>'published' THEN RAISE EXCEPTION 'nonconsecutive newer requirement publish failed'; END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_requirement_set_current WHERE scope_id='00000000-0000-4000-8000-000000000010' AND artifact_id='00000000-0000-4000-8000-000000000083' AND generation=2 AND document_set_sequence=2 AND disposition_set_sequence=3) THEN RAISE EXCEPTION 'requirement current pointer mismatch'; END IF;
  BEGIN UPDATE bid_requirement_set_current SET artifact_id='00000000-0000-4000-8000-000000000082',artifact_sha256=h_high,generation=3 WHERE scope_id='00000000-0000-4000-8000-000000000010'; RAISE EXCEPTION 'requirement incoherent tuple accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
  BEGIN UPDATE bid_requirement_set_artifacts SET canonical_payload=canonical_payload WHERE id='00000000-0000-4000-8000-000000000081'; RAISE EXCEPTION 'requirement append-only update accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

-- Requirement supersession aggregate.
INSERT INTO bid_requirement_supersession_revision_artifacts(id,project_id,lineage_id,revision,old_requirement_revision_id,new_requirement_revision_id,applicability,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-000000000091','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000b1',1,'00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000072','{}',convert_to('sup1','UTF8'),encode(digest(convert_to('sup1','UTF8'),'sha256'),'hex'),'system:bid-extraction-worker'),
 ('00000000-0000-4000-8000-000000000092','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000b1',2,'00000000-0000-4000-8000-000000000072','00000000-0000-4000-8000-000000000071','{}',convert_to('sup2','UTF8'),encode(digest(convert_to('sup2','UTF8'),'sha256'),'hex'),'system:bid-extraction-worker');
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_requirement_supersession_revision_artifacts(id,project_id,lineage_id,revision,old_requirement_revision_id,new_requirement_revision_id,applicability,canonical_payload,content_sha256,actor)
    VALUES('00000000-0000-4000-8000-000000000099','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000b9',1,'00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000079','{}',convert_to('bad-sup','UTF8'),encode(digest(convert_to('bad-sup','UTF8'),'sha256'),'hex'),'system:bid-extraction-worker');
    RAISE EXCEPTION 'supersession cross-project identity unexpectedly accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
DO $$ DECLARE h1 kb_sha256:=encode(digest(convert_to('sup1','UTF8'),'sha256'),'hex'); DECLARE h2 kb_sha256:=encode(digest(convert_to('sup2','UTF8'),'sha256'),'hex'); BEGIN
  IF NOT kb_bid_v2_advance_requirement_supersession('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000b1',NULL,NULL,'00000000-0000-4000-8000-000000000091',h1) THEN RAISE EXCEPTION 'supersession initial failed'; END IF;
  IF NOT kb_bid_v2_advance_requirement_supersession('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000b1',NULL,NULL,'00000000-0000-4000-8000-000000000091',h1) THEN RAISE EXCEPTION 'supersession replay failed'; END IF;
  IF kb_bid_v2_advance_requirement_supersession('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000b1','00000000-0000-4000-8000-000000000099',repeat('9',64),'00000000-0000-4000-8000-000000000092',h2) THEN RAISE EXCEPTION 'supersession stale CAS accepted'; END IF;
  IF NOT kb_bid_v2_advance_requirement_supersession('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000b1','00000000-0000-4000-8000-000000000091',h1,'00000000-0000-4000-8000-000000000092',h2) THEN RAISE EXCEPTION 'supersession advance failed'; END IF;
  BEGIN UPDATE bid_requirement_supersession_current SET artifact_id='00000000-0000-4000-8000-000000000091',artifact_sha256=h1,generation=3 WHERE scope_id='00000000-0000-4000-8000-0000000000b1'; RAISE EXCEPTION 'supersession incoherent pointer accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
  BEGIN UPDATE bid_requirement_supersession_revision_artifacts SET actor=actor WHERE id='00000000-0000-4000-8000-000000000091'; RAISE EXCEPTION 'supersession append-only update accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

-- WorkspaceRequirementProjection aggregate.
INSERT INTO bid_submission_workspaces(id,project_id) VALUES
 ('00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000010'),
 ('00000000-0000-4000-8000-0000000000a9','00000000-0000-4000-8000-000000000019');
INSERT INTO bid_workspace_requirement_projection_artifacts(id,project_id,workspace_id,requirement_set_id,revision,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-0000000000b1','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000082',1,convert_to('proj1','UTF8'),encode(digest(convert_to('proj1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-0000000000b2','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000083',2,convert_to('proj2','UTF8'),encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-0000000000b9','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-0000000000a9','00000000-0000-4000-8000-000000000089',1,convert_to('proj9','UTF8'),encode(digest(convert_to('proj9','UTF8'),'sha256'),'hex'));
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_workspace_requirement_projection_artifacts(id,project_id,workspace_id,requirement_set_id,revision,canonical_payload,content_sha256)
    VALUES('00000000-0000-4000-8000-0000000000ba','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000089',3,convert_to('bad-proj','UTF8'),encode(digest(convert_to('bad-proj','UTF8'),'sha256'),'hex'));
    RAISE EXCEPTION 'projection cross-project identity unexpectedly accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
DO $$ DECLARE h1 kb_sha256:=encode(digest(convert_to('proj1','UTF8'),'sha256'),'hex'); DECLARE h2 kb_sha256:=encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'); BEGIN
  IF NOT kb_bid_v2_advance_requirement_projection('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',NULL,NULL,'00000000-0000-4000-8000-0000000000b1',h1) THEN RAISE EXCEPTION 'projection initial failed'; END IF;
  IF NOT kb_bid_v2_advance_requirement_projection('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',NULL,NULL,'00000000-0000-4000-8000-0000000000b1',h1) THEN RAISE EXCEPTION 'projection replay failed'; END IF;
  IF kb_bid_v2_advance_requirement_projection('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000099',repeat('9',64),'00000000-0000-4000-8000-0000000000b2',h2) THEN RAISE EXCEPTION 'projection stale CAS accepted'; END IF;
  IF NOT kb_bid_v2_advance_requirement_projection('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-0000000000b1',h1,'00000000-0000-4000-8000-0000000000b2',h2) THEN RAISE EXCEPTION 'projection advance failed'; END IF;
  IF kb_bid_v2_publish_requirement_set('00000000-0000-4000-8000-000000000081',
       encode(digest(convert_to('rset-old','UTF8'),'sha256'),'hex'))<>'superseded' THEN
    RAISE EXCEPTION 'late RequirementSet redelivery was not superseded';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_requirement_set_current
       WHERE scope_id='00000000-0000-4000-8000-000000000010'
         AND artifact_id='00000000-0000-4000-8000-000000000083')
     OR NOT EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_current
       WHERE scope_id='00000000-0000-4000-8000-0000000000a0'
         AND artifact_id='00000000-0000-4000-8000-0000000000b2' AND generation=2) THEN
    RAISE EXCEPTION 'stale RequirementSet redelivery rolled back Workspace projection';
  END IF;
  BEGIN UPDATE bid_workspace_requirement_projection_current SET artifact_id='00000000-0000-4000-8000-0000000000b1',artifact_sha256=h1,generation=3 WHERE scope_id='00000000-0000-4000-8000-0000000000a0'; RAISE EXCEPTION 'projection incoherent pointer accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
  BEGIN UPDATE bid_workspace_requirement_projection_artifacts SET canonical_payload=canonical_payload WHERE id='00000000-0000-4000-8000-0000000000b1'; RAISE EXCEPTION 'projection append-only update accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

-- Typed fulfillment binding targets: four valid and four explicit negatives.
INSERT INTO bid_outline_node_lineages(id,project_id,workspace_id) VALUES
 ('00000000-0000-4000-8000-0000000000c1','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0'),
 ('00000000-0000-4000-8000-0000000001d2','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0'),
 ('00000000-0000-4000-8000-0000000000c9','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-0000000000a9');
INSERT INTO bid_content_block_lineages(id,project_id,workspace_id) VALUES
 ('00000000-0000-4000-8000-0000000000d1','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0'),
 ('00000000-0000-4000-8000-0000000000d2','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0'),
 ('00000000-0000-4000-8000-0000000000d9','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-0000000000a9');
INSERT INTO bid_content_block_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,schema_version,block_kind,block_payload,origin,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-0000000000e1','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-0000000000d1',1,1,'table','{}','human',convert_to('table1','UTF8'),encode(digest(convert_to('table1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-0000000000e2','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-0000000000d2',1,1,'rich_text','{"type":"rich_text","nodes":[{"kind":"paragraph","content":[]}]}','human',convert_to('text1','UTF8'),encode(digest(convert_to('text1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-0000000000e9','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-0000000000a9','00000000-0000-4000-8000-0000000000d9',1,1,'table','{}','human',convert_to('table9','UTF8'),encode(digest(convert_to('table9','UTF8'),'sha256'),'hex'));
INSERT INTO bid_quote_snapshot_artifacts(id,project_id,revision,currency,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-0000000000f1','00000000-0000-4000-8000-000000000010',1,'CNY',convert_to('{}','UTF8'),encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001'),
 ('00000000-0000-4000-8000-0000000000f9','00000000-0000-4000-8000-000000000019',1,'CNY',convert_to('quote9','UTF8'),encode(digest(convert_to('quote9','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
INSERT INTO bid_outline_fulfillment_binding_lineages(id,project_id,workspace_id)
SELECT ('00000000-0000-4000-8000-'||lpad(to_hex(n),12,'0'))::uuid,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0' FROM generate_series(257,264) n;
INSERT INTO bid_outline_fulfillment_binding_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,need_occurrence_id,requirement_projection_id,channel,target_kind,target_id,state,reason,actor,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-000000000111','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000101',1,'00000000-0000-4000-8000-000000000201','00000000-0000-4000-8000-0000000000b2','narrative_content','outline_node','00000000-0000-4000-8000-0000000000c1','bound','valid node','system:bid-extraction-worker',convert_to('bind1','UTF8'),encode(digest(convert_to('bind1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000112','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000102',1,'00000000-0000-4000-8000-000000000202','00000000-0000-4000-8000-0000000000b2','response_table','response_table','00000000-0000-4000-8000-0000000000d1','bound','valid table','system:bid-extraction-worker',convert_to('bind2','UTF8'),encode(digest(convert_to('bind2','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000113','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000103',1,'00000000-0000-4000-8000-000000000203','00000000-0000-4000-8000-0000000000b2','structured_form','structured_form','00000000-0000-4000-8000-000000000053','bound','valid form','system:bid-extraction-worker',convert_to('bind3','UTF8'),encode(digest(convert_to('bind3','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000114','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000104',1,'00000000-0000-4000-8000-000000000204','00000000-0000-4000-8000-0000000000b2','quotation','quote','00000000-0000-4000-8000-0000000000f1','bound','valid quote','system:bid-extraction-worker',convert_to('bind4','UTF8'),encode(digest(convert_to('bind4','UTF8'),'sha256'),'hex'));
DO $$ DECLARE k text; DECLARE target uuid; DECLARE line uuid; BEGIN
  FOR k,target,line IN VALUES
    ('outline_node','00000000-0000-4000-8000-0000000000c9'::uuid,'00000000-0000-4000-8000-000000000105'::uuid),
    ('response_table','00000000-0000-4000-8000-0000000000d2'::uuid,'00000000-0000-4000-8000-000000000106'::uuid),
    ('structured_form','00000000-0000-4000-8000-000000000058'::uuid,'00000000-0000-4000-8000-000000000107'::uuid),
    ('quote','00000000-0000-4000-8000-0000000000f9'::uuid,'00000000-0000-4000-8000-000000000108'::uuid)
  LOOP
    BEGIN
      INSERT INTO bid_outline_fulfillment_binding_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,need_occurrence_id,requirement_projection_id,channel,target_kind,target_id,state,reason,actor,canonical_payload,content_sha256)
      VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',line,1,gen_random_uuid(),'00000000-0000-4000-8000-0000000000b2','narrative_content',k,target,'bound','must reject','system:bid-extraction-worker',convert_to('invalid-'||k,'UTF8'),encode(digest(convert_to('invalid-'||k,'UTF8'),'sha256'),'hex'));
      RAISE EXCEPTION 'invalid % binding target accepted',k;
    EXCEPTION WHEN foreign_key_violation THEN NULL; END;
  END LOOP;
END $$;

-- Seventh-round knowledge media identity fixture. This publishes only the
-- frozen storage identity and OCR-chunk mapping; no V3 retrieval path exists.
INSERT INTO workspaces(id,name,slug,kind) VALUES
 ('00000000-0000-4000-8000-000000000170','Phase 0 Knowledge','phase-0-knowledge','product_line');
INSERT INTO products(id,workspace_id,kind,name,slug) VALUES
 ('00000000-0000-4000-8000-000000000171','00000000-0000-4000-8000-000000000170','product','Fixture','fixture');
INSERT INTO product_versions(id,product_id,label,status) VALUES
 ('00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000171','v1','active');
SELECT kb_object_reference_add('objects/'||repeat('5',64),repeat('5',64),'image/png',4,
 'knowledge_document','00000000-0000-4000-8000-000000000173','original','system:knowledge-document-ingest');
UPDATE products SET current_version_id='00000000-0000-4000-8000-000000000172'
 WHERE id='00000000-0000-4000-8000-000000000171';
INSERT INTO documents(id,product_version_id,type,title,parse_status,enable_status,index_ready,file_name,file_size,file_hash,object_ref) VALUES
 ('00000000-0000-4000-8000-000000000173','00000000-0000-4000-8000-000000000172','file','proof.png','completed','enabled',true,'proof.png',4,repeat('5',64),'objects/'||repeat('5',64));
INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content) VALUES
 ('00000000-0000-4000-8000-000000000174','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','image_ocr','proof image'),
 ('00000000-0000-4000-8000-000000000177','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','image_ocr','unit1'),
 ('00000000-0000-4000-8000-000000000178','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','image_ocr','unit1');

-- Sixth-round canonical publication/provenance closure.
INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state) VALUES
('objects/'||repeat('a',64),repeat('a',64),'application/pdf',1,'available'),
('objects/'||repeat('6',64),repeat('6',64),'image/png',4,'available'),
 ('objects/'||repeat('7',64),repeat('7',64),'font/ttf',4,'available'),
 ('objects/'||repeat('8',64),repeat('8',64),'image/jpeg',4,'available'),
 ('objects/'||repeat('9',64),repeat('9',64),'image/png',4,'available'),
 ('objects/'||repeat('f',64),repeat('f',64),'application/pdf',1,'available'),
 ('objects/'||encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),'application/json',2,'available');
INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state,deleting_at) VALUES
 ('objects/'||repeat('d',64),repeat('d',64),'application/pdf',1,'deleting',clock_timestamp());
INSERT INTO knowledge_image_artifact_revisions(id,product_version_id,document_id,revision,object_ref,content_sha256,media_type,width,height,page_ordinal,bounding_region,source_image_key,canonical_payload,artifact_sha256) VALUES
 ('00000000-0000-4000-8000-000000000145','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173',1,'objects/'||repeat('6',64),repeat('6',64),'image/png',1,1,0,'{"left":0,"top":0,"right":1,"bottom":1}','images/proof.png',convert_to('knowledge-image-1','UTF8'),encode(digest(convert_to('knowledge-image-1','UTF8'),'sha256'),'hex'));
INSERT INTO knowledge_image_ocr_chunk_artifact_mappings(chunk_id,product_version_id,document_id,image_artifact_revision_id,object_ref,content_sha256,media_type) VALUES
 ('00000000-0000-4000-8000-000000000174','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','00000000-0000-4000-8000-000000000145','objects/'||repeat('6',64),repeat('6',64),'image/png'),
 ('00000000-0000-4000-8000-000000000177','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','00000000-0000-4000-8000-000000000145','objects/'||repeat('6',64),repeat('6',64),'image/png'),
 ('00000000-0000-4000-8000-000000000178','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','00000000-0000-4000-8000-000000000145','objects/'||repeat('6',64),repeat('6',64),'image/png');
INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content) VALUES
 ('00000000-0000-4000-8000-000000000175','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','image_ocr','unknown media fixture'),
 ('00000000-0000-4000-8000-000000000176','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','image_ocr','wrong digest fixture');
DO $$ BEGIN
 BEGIN INSERT INTO knowledge_image_artifact_revisions(id,product_version_id,document_id,revision,object_ref,content_sha256,media_type,width,height,source_image_key,canonical_payload,artifact_sha256) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173',1,'objects/'||repeat('6',64),repeat('6',64),'image/jpeg',1,1,'bad-mime',convert_to('bad','UTF8'),encode(digest(convert_to('bad','UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'knowledge media MIME mismatch accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO knowledge_image_ocr_chunk_artifact_mappings(chunk_id,product_version_id,document_id,image_artifact_revision_id,object_ref,content_sha256,media_type) VALUES('00000000-0000-4000-8000-000000000175','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173',gen_random_uuid(),'objects/'||repeat('6',64),repeat('6',64),'image/png'); RAISE EXCEPTION 'unknown knowledge media identity accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO knowledge_image_ocr_chunk_artifact_mappings(chunk_id,product_version_id,document_id,image_artifact_revision_id,object_ref,content_sha256,media_type) VALUES('00000000-0000-4000-8000-000000000176','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','00000000-0000-4000-8000-000000000145','objects/'||repeat('f',64),repeat('f',64),'image/png'); RAISE EXCEPTION 'wrong knowledge media ObjectRegistry digest accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
INSERT INTO bid_quote_snapshot_object_identities(quote_snapshot_id,project_id,object_ref,content_sha256) VALUES
 ('00000000-0000-4000-8000-0000000000f1','00000000-0000-4000-8000-000000000010','objects/'||encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'));
INSERT INTO bid_workspace_asset_artifacts(id,project_id,workspace_id,object_ref,content_sha256,media_type,file_name,byte_length,width_px,height_px,source,created_by) VALUES
('00000000-0000-4000-8000-000000000150','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','objects/'||repeat('8',64),repeat('8',64),'image/jpeg','fixture-one.jpg',4,1,1,'human_upload','user:00000000-0000-4000-8000-000000000001'),
('00000000-0000-4000-8000-000000000152','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','objects/'||repeat('8',64),repeat('8',64),'image/jpeg','fixture-two.jpg',4,1,1,'human_upload','user:00000000-0000-4000-8000-000000000001');
INSERT INTO bid_render_font_artifacts(id,object_ref,content_sha256,media_type,family,script) VALUES
 ('00000000-0000-4000-8000-000000000147','objects/'||repeat('7',64),repeat('7',64),'font/ttf','Noto Sans JP','cjk');
DO $$ BEGIN
 BEGIN INSERT INTO bid_render_font_artifacts(id,object_ref,content_sha256,media_type,family,script) VALUES(gen_random_uuid(),'objects/'||repeat('7',64),repeat('7',64),'font/otf','Wrong MIME','cjk'); RAISE EXCEPTION 'font ObjectRegistry MIME mismatch accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;

INSERT INTO bid_workspace_scope_revision_artifacts(id,project_id,workspace_id,revision,scope_kind,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-000000000121','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',1,'project_wide',convert_to('scope1','UTF8'),encode(digest(convert_to('scope1','UTF8'),'sha256'),'hex'));
INSERT INTO bid_document_settings_revision_artifacts(id,project_id,workspace_id,revision,schema_version,settings,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-000000000122','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',1,1,'{"page_size":"A4"}',convert_to('settings1','UTF8'),encode(digest(convert_to('settings1','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
INSERT INTO bid_workspace_revision_artifacts(id,project_id,workspace_id,revision,scope_revision_id,requirement_projection_id,requirement_projection_sha256,document_settings_revision_id,quote_snapshot_id,quote_snapshot_sha256,canonical_payload,content_sha256,actor) VALUES
('00000000-0000-4000-8000-000000000123','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',1,'00000000-0000-4000-8000-000000000121','00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000122','00000000-0000-4000-8000-0000000000f1',encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),convert_to('{"fixture":1}','UTF8'),encode(digest(convert_to('{"fixture":1}','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
INSERT INTO bid_workspace_revision_artifacts(id,project_id,workspace_id,revision,parent_revision_id,parent_sha256,scope_revision_id,requirement_projection_id,requirement_projection_sha256,document_settings_revision_id,quote_snapshot_id,quote_snapshot_sha256,canonical_payload,content_sha256,actor) VALUES
('00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',2,'00000000-0000-4000-8000-000000000123',encode(digest(convert_to('{"fixture":1}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000121','00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000122','00000000-0000-4000-8000-0000000000f1',encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),convert_to('{"fixture":2}','UTF8'),encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
INSERT INTO bid_workspace_heads(scope_id,project_id,artifact_id,artifact_sha256,generation,created_at) VALUES
 ('00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),2,now());
INSERT INTO bid_outline_node_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,title,semantic_role,render_role,origin,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-00000000013b','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-0000000000c1',1,'Response','technical','section','human',convert_to('node1','UTF8'),encode(digest(convert_to('node1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-0000000001d0','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-0000000001d2',1,'Child response','technical','section','human',convert_to('node-child','UTF8'),encode(digest(convert_to('node-child','UTF8'),'sha256'),'hex'));
INSERT INTO bid_workspace_node_occurrences(id,project_id,workspace_revision_id,node_revision_id,parent_occurrence_id,ordinal,depth) VALUES
 ('00000000-0000-4000-8000-00000000013c','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-00000000013b',NULL,0,0),
 ('00000000-0000-4000-8000-0000000001d1','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-0000000001d0','00000000-0000-4000-8000-00000000013c',0,1);
INSERT INTO bid_workspace_block_occurrences(id,project_id,workspace_revision_id,node_occurrence_id,block_revision_id,ordinal) VALUES
 ('00000000-0000-4000-8000-00000000013d','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-00000000013c','00000000-0000-4000-8000-0000000000e2',0);
-- The coherent checkpoint freezes the exact projection owned by WorkspaceRevision 2.
INSERT INTO bid_outline_checkpoint_artifacts(id,project_id,workspace_id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-00000000013e','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),convert_to('checkpoint1','UTF8'),encode(digest(convert_to('checkpoint1','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
-- Projection b1 is also legitimate in this workspace, but WorkspaceRevision 2 owns b2.
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_outline_checkpoint_artifacts(id,project_id,workspace_id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,canonical_payload,content_sha256,actor)
    VALUES('00000000-0000-4000-8000-00000000013f','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-0000000000b1',encode(digest(convert_to('proj1','UTF8'),'sha256'),'hex'),convert_to('checkpoint-cross-projection','UTF8'),encode(digest(convert_to('checkpoint-cross-projection','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');
    RAISE EXCEPTION 'outline checkpoint accepted another legitimate projection not owned by its WorkspaceRevision';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
INSERT INTO bid_submission_assessment_snapshot_artifacts(id,project_id,workspace_id,workspace_revision_id,requirement_projection_id,scope_revision_id,document_settings_revision_id,asset_set_sha256,quote_snapshot_id,quote_snapshot_sha256,status,assessment_input_sha256,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-000000000124','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-0000000000b2','00000000-0000-4000-8000-000000000121','00000000-0000-4000-8000-000000000122',repeat('a',64),'00000000-0000-4000-8000-0000000000f1',encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),'ready',repeat('b',64),convert_to('assessment1','UTF8'),encode(digest(convert_to('assessment1','UTF8'),'sha256'),'hex'));

-- The fixture publishes a supported embedding/rerank/policy chain, then asks
-- the knowledge-owned procedure to attest the scope. It never forges an
-- attestation row directly.
CREATE TEMP TABLE phase0_attestation(id uuid PRIMARY KEY,content_sha256 kb_sha256 NOT NULL);
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
 scope:=jsonb_build_object(
  'schema_version',2,
  'products',jsonb_build_array(jsonb_build_object('id','00000000-0000-4000-8000-0000000001b4',
    'product_id','00000000-0000-4000-8000-0000000001b7','product_version_id','00000000-0000-4000-8000-0000000001b4',
    'workspace_kind','company','frozen_display_name','00000000-0000-4000-8000-0000000001b4',
    'identity_sha256',encode(digest(convert_to('ProductVersionEvidenceV1:00000000-0000-4000-8000-0000000001b7:00000000-0000-4000-8000-0000000001b4:company','UTF8'),'sha256'),'hex')),
    jsonb_build_object('id','00000000-0000-4000-8000-000000000172','product_id','00000000-0000-4000-8000-000000000171',
      'product_version_id','00000000-0000-4000-8000-000000000172','workspace_kind','product_line',
      'frozen_display_name','00000000-0000-4000-8000-000000000172','identity_sha256',encode(digest(convert_to(
      'ProductVersionEvidenceV1:00000000-0000-4000-8000-000000000171:00000000-0000-4000-8000-000000000172:product_line','UTF8'),'sha256'),'hex'))),
  'frozen_hits',jsonb_build_array(jsonb_build_object('id','00000000-0000-4000-8000-0000000001bc',
    'route_id','00000000-0000-4000-8000-000000000071','requirement_artifact_id','00000000-0000-4000-8000-000000000071',
    'product_version_artifact_id','00000000-0000-4000-8000-0000000001b4','document_id','00000000-0000-4000-8000-0000000001b2',
    'source_chunk_id','00000000-0000-4000-8000-0000000001b3','frozen_document_display_name','verified-source.txt',
    'chunk_utf8','verified fact','chunk_sha256',encode(digest(convert_to('verified fact','UTF8'),'sha256'),'hex'),
    'chunk_byte_length',13,'source_type','text','media',NULL,'retrieval_rank',1,'retrieval_raw_score','1.000000',
    'pre_rerank_rrf_rank',NULL,'quote_start_offset',0,'quote_end_offset',13,'offset_unit','utf8_byte',
    'retrieval_contract_version','knowledge-evidence-v2'),
    jsonb_build_object('id','00000000-0000-4000-8000-0000000001bd','route_id','00000000-0000-4000-8000-000000000071',
      'requirement_artifact_id','00000000-0000-4000-8000-000000000071','product_version_artifact_id','00000000-0000-4000-8000-000000000172',
      'document_id','00000000-0000-4000-8000-000000000173','source_chunk_id','00000000-0000-4000-8000-000000000174',
      'frozen_document_display_name','proof.png','chunk_utf8','proof image',
      'chunk_sha256',encode(digest(convert_to('proof image','UTF8'),'sha256'),'hex'),'chunk_byte_length',11,
      'source_type','image_ocr','media',jsonb_build_object('image_artifact_revision_id','00000000-0000-4000-8000-000000000145',
        'object_ref','objects/'||repeat('6',64),'sha256',repeat('6',64),'media_type','image/png','width',1,'height',1,
        'page_ordinal',0,'bounding_region',jsonb_build_object('left',0,'top',0,'right',1,'bottom',1),
        'frozen_document_display_name','proof.png'),'retrieval_rank',2,'retrieval_raw_score','0.500000',
      'pre_rerank_rrf_rank',1,'quote_start_offset',0,'quote_end_offset',11,'offset_unit','utf8_byte',
      'retrieval_contract_version','knowledge-evidence-v2')),
  'retrieval_requirements',jsonb_build_array(jsonb_build_object('route_id','00000000-0000-4000-8000-000000000071',
    'requirement_artifact_id','00000000-0000-4000-8000-000000000071',
    'requirement_identity_sha256',encode(digest(convert_to('verified fact','UTF8'),'sha256'),'hex'),
    'requirement_text','verified fact','exact_prefix_hit_count',1)),
  'version_selections',jsonb_build_object('company','[]'::jsonb,'product_line','[]'::jsonb),
  'workspace_kinds',jsonb_build_array('company','product_line'),
  'retrieval_policy',jsonb_build_object('contract_version','knowledge-evidence-v2','policy_sha256',policy_sha,'max_hits',2,'max_chunk_bytes',1024,'max_total_bytes',1024));
 attestation:=kb_knowledge_attest_matching_scope_v2(scope);
 INSERT INTO phase0_attestation VALUES((attestation->>'id')::uuid,attestation->>'content_sha256');
END $$;
INSERT INTO bid_evidence_match_reports(id,project_id,workspace_id,requirement_revision_id,retrieval_contract_version,knowledge_scope_attestation_id,knowledge_scope_attestation_sha256,canonical_payload,content_sha256)
SELECT '00000000-0000-4000-8000-000000000141','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','knowledge-evidence-v2',id,content_sha256,convert_to('match1','UTF8'),encode(digest(convert_to('match1','UTF8'),'sha256'),'hex')
FROM phase0_attestation;
DO $$ BEGIN
 BEGIN INSERT INTO bid_evidence_match_reports(id,project_id,workspace_id,requirement_revision_id,retrieval_contract_version,knowledge_scope_attestation_id,knowledge_scope_attestation_sha256,canonical_payload,content_sha256)
 VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','v3',gen_random_uuid(),repeat('a',64),convert_to('bad-attestation','UTF8'),encode(digest(convert_to('bad-attestation','UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'unknown attestation accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO bid_evidence_match_reports(id,project_id,workspace_id,requirement_revision_id,retrieval_contract_version,knowledge_scope_attestation_id,knowledge_scope_attestation_sha256,canonical_payload,content_sha256)
 SELECT gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','v3',id,repeat('f',64),convert_to('bad-attestation-sha','UTF8'),encode(digest(convert_to('bad-attestation-sha','UTF8'),'sha256'),'hex') FROM phase0_attestation; RAISE EXCEPTION 'wrong attestation sha accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;

-- Schema-valid EvidenceBundleV1 plus exact item/media projections in one transaction.
DO $$ DECLARE item jsonb; base jsonb; payload jsonb; sha text; created constant timestamptz:='2026-01-01T00:00:00Z'; BEGIN
 item:=jsonb_build_object('kind','image','evidence_item_id','00000000-0000-4000-8000-000000000144',
 'document_id','00000000-0000-4000-8000-000000000173','source_chunk_id','00000000-0000-4000-8000-000000000174',
 'product_version_id','00000000-0000-4000-8000-000000000172','workspace_kind','product_line',
 'quote_utf8','proof image','quote_sha256',encode(digest(convert_to('proof image','UTF8'),'sha256'),'hex'),
 'quote_start_offset',0,'quote_end_offset',11,'retrieval_rank',2,'retrieval_contract_version','knowledge-evidence-v2',
 'image_artifact_revision_id','00000000-0000-4000-8000-000000000145','object_ref','objects/'||repeat('6',64),'sha256',repeat('6',64),'media_type','image/png','width',1,'height',1,'page_ordinal',0,'bounding_region',jsonb_build_object('left',0,'top',0,'right',1,'bottom',1),'frozen_document_display_name','proof.png');
 base:=jsonb_build_object('schema_version',1,'evidence_bundle_id','00000000-0000-4000-8000-000000000143','project_id','00000000-0000-4000-8000-000000000010','workspace_id','00000000-0000-4000-8000-0000000000a0','workspace_scope','project_wide','requirement_revision_id','00000000-0000-4000-8000-000000000071','matching_report_id','00000000-0000-4000-8000-000000000141','knowledge_scope_attestation_id',(SELECT id FROM phase0_attestation),'knowledge_scope_attestation_sha256',(SELECT content_sha256 FROM phase0_attestation),'items',jsonb_build_array(item),'created_at','2026-01-01T00:00:00Z');
 sha:=encode(digest(convert_to(base::text,'UTF8'),'sha256'),'hex'); payload:=base||jsonb_build_object('bundle_sha256',sha);
 INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at) VALUES('00000000-0000-4000-8000-000000000143','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000141',payload,sha,created);
 INSERT INTO bid_evidence_bundle_items(id,project_id,workspace_id,evidence_bundle_id,ordinal,item_kind,source_media_revision_id,item_payload,content_sha256) VALUES('00000000-0000-4000-8000-000000000144','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000143',0,'image','00000000-0000-4000-8000-000000000145',item,encode(digest(convert_to(item::text,'UTF8'),'sha256'),'hex'));
 INSERT INTO bid_evidence_asset_artifacts(id,project_id,workspace_id,evidence_bundle_id,evidence_item_id,image_artifact_revision_id,object_ref,content_sha256,media_type,width,height,page_ordinal,bounding_region) VALUES('00000000-0000-4000-8000-000000000146','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000143','00000000-0000-4000-8000-000000000144','00000000-0000-4000-8000-000000000145','objects/'||repeat('6',64),repeat('6',64),'image/png',1,1,0,'{"left":0,"top":0,"right":1,"bottom":1}');
 SET CONSTRAINTS ALL IMMEDIATE;
END $$;

-- Text evidence freezes the SHA-256 of the exact UTF-8 quote bytes. The
-- positive proves a coherent projection; the negative recomputes the bundle
-- hash so only the quote digest invariant can reject publication.
DO $$
DECLARE
 item jsonb;
 base jsonb;
 payload jsonb;
 bundle_sha text;
 quote constant text:='verified fact';
 quote_sha text:=encode(digest(convert_to(quote,'UTF8'),'sha256'),'hex');
 created constant timestamptz:='2026-01-01T00:00:00Z';
 bad_item jsonb;
 bad_base jsonb;
 bad_payload jsonb;
 bad_bundle_sha text;
 tender_item jsonb;
 tender_base jsonb;
 tender_payload jsonb;
 tender_bundle_sha text;
BEGIN
 item:=jsonb_build_object(
  'kind','text_quote','evidence_item_id','00000000-0000-4000-8000-0000000001b1',
  'document_id','00000000-0000-4000-8000-0000000001b2','source_chunk_id','00000000-0000-4000-8000-0000000001b3',
  'product_version_id','00000000-0000-4000-8000-0000000001b4','workspace_kind','company',
  'frozen_document_display_name','verified-source.txt','quote_utf8',quote,'quote_sha256',quote_sha,
  'quote_start_offset',0,'quote_end_offset',13,'retrieval_rank',1,'retrieval_contract_version','knowledge-evidence-v2');
 base:=jsonb_build_object(
  'schema_version',1,'evidence_bundle_id','00000000-0000-4000-8000-0000000001b0',
  'project_id','00000000-0000-4000-8000-000000000010','workspace_id','00000000-0000-4000-8000-0000000000a0',
  'workspace_scope','project_wide','requirement_revision_id','00000000-0000-4000-8000-000000000071',
  'matching_report_id','00000000-0000-4000-8000-000000000141',
  'knowledge_scope_attestation_id',(SELECT id FROM phase0_attestation),
  'knowledge_scope_attestation_sha256',(SELECT content_sha256 FROM phase0_attestation),
  'items',jsonb_build_array(item),'created_at','2026-01-01T00:00:00Z');
 bundle_sha:=encode(digest(convert_to(base::text,'UTF8'),'sha256'),'hex');
 payload:=base||jsonb_build_object('bundle_sha256',bundle_sha);
 INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at)
 VALUES('00000000-0000-4000-8000-0000000001b0','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000141',payload,bundle_sha,created);
 INSERT INTO bid_evidence_bundle_items(id,project_id,workspace_id,evidence_bundle_id,ordinal,item_kind,item_payload,content_sha256)
 VALUES('00000000-0000-4000-8000-0000000001b1','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-0000000001b0',0,'text_quote',item,encode(digest(convert_to(item::text,'UTF8'),'sha256'),'hex'));
 SET CONSTRAINTS ALL IMMEDIATE;

 bad_item:=item||jsonb_build_object(
  'evidence_item_id','00000000-0000-4000-8000-0000000001b6',
  'quote_sha256',repeat('f',64));
 bad_base:=jsonb_set(base,'{evidence_bundle_id}',to_jsonb('00000000-0000-4000-8000-0000000001b5'::text));
 bad_base:=jsonb_set(bad_base,'{items}',jsonb_build_array(bad_item));
 bad_bundle_sha:=encode(digest(convert_to(bad_base::text,'UTF8'),'sha256'),'hex');
 bad_payload:=bad_base||jsonb_build_object('bundle_sha256',bad_bundle_sha);
 BEGIN
  INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at)
  VALUES('00000000-0000-4000-8000-0000000001b5','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000141',bad_payload,bad_bundle_sha,created);
  RAISE EXCEPTION 'wrong text quote digest accepted';
 EXCEPTION WHEN check_violation THEN NULL;
 END;

 tender_item:=item||jsonb_build_object(
  'evidence_item_id','00000000-0000-4000-8000-0000000001ba',
  'document_id','00000000-0000-4000-8000-000000000011',
  'source_chunk_id','00000000-0000-4000-8000-000000000052',
  'product_version_id','00000000-0000-4000-8000-000000000031');
 tender_base:=jsonb_set(base,'{evidence_bundle_id}',to_jsonb('00000000-0000-4000-8000-0000000001b9'::text));
 tender_base:=jsonb_set(tender_base,'{items}',jsonb_build_array(tender_item));
 tender_bundle_sha:=encode(digest(convert_to(tender_base::text,'UTF8'),'sha256'),'hex');
 tender_payload:=tender_base||jsonb_build_object('bundle_sha256',tender_bundle_sha);
 BEGIN
  INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at)
  VALUES('00000000-0000-4000-8000-0000000001b9','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000141',tender_payload,tender_bundle_sha,created);
  RAISE EXCEPTION 'tender SourceUnit accepted as bidder text evidence';
 EXCEPTION WHEN check_violation THEN NULL;
 END;
END $$;

DO $$ DECLARE p jsonb:=(SELECT canonical_payload FROM bid_evidence_bundle_artifacts WHERE id='00000000-0000-4000-8000-000000000143'); BEGIN
 BEGIN INSERT INTO bid_evidence_bundle_artifacts SELECT gen_random_uuid(),project_id,workspace_id,requirement_revision_id,matching_report_id,p||jsonb_build_object('unknown',true),content_sha256,created_at FROM bid_evidence_bundle_artifacts WHERE id='00000000-0000-4000-8000-000000000143'; RAISE EXCEPTION 'closed evidence schema accepted unknown key'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_evidence_bundle_items(id,project_id,workspace_id,evidence_bundle_id,ordinal,item_kind,item_payload,content_sha256) VALUES('00000000-0000-4000-8000-000000000149','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000143',1,'no_evidence','{"kind":"no_evidence","evidence_item_id":"00000000-0000-4000-8000-000000000149","reason_code":"NO_MATCHING_HIT"}',encode(digest(convert_to('{"evidence_item_id": "00000000-0000-4000-8000-000000000149", "kind": "no_evidence", "reason_code": "NO_MATCHING_HIT"}'::jsonb::text,'UTF8'),'sha256'),'hex')); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'extra evidence projection accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 SET CONSTRAINTS ALL DEFERRED;
 BEGIN INSERT INTO bid_evidence_bundle_items(id,project_id,workspace_id,evidence_bundle_id,ordinal,item_kind,item_payload,content_sha256) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000143',1,'no_evidence','{"kind":"no_evidence","evidence_item_id":"00000000-0000-4000-8000-000000000149","reason_code":"NO_MATCHING_HIT"}',encode(digest(convert_to('{"evidence_item_id": "00000000-0000-4000-8000-000000000149", "kind": "no_evidence", "reason_code": "NO_MATCHING_HIT"}'::jsonb::text,'UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'mismatched evidence item id accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_evidence_bundle_items(id,project_id,workspace_id,evidence_bundle_id,ordinal,item_kind,item_payload,content_sha256) VALUES('00000000-0000-4000-8000-000000000149','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000143',1,'text_quote','{"kind":"no_evidence","evidence_item_id":"00000000-0000-4000-8000-000000000149","reason_code":"NO_MATCHING_HIT"}',encode(digest(convert_to('{"evidence_item_id": "00000000-0000-4000-8000-000000000149", "kind": "no_evidence", "reason_code": "NO_MATCHING_HIT"}'::jsonb::text,'UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'mismatched evidence item kind accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_evidence_asset_artifacts(id,project_id,workspace_id,evidence_bundle_id,evidence_item_id,image_artifact_revision_id,object_ref,content_sha256,media_type,width,height) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-0000000000a9','00000000-0000-4000-8000-000000000143','00000000-0000-4000-8000-000000000144','00000000-0000-4000-8000-000000000145','objects/'||repeat('6',64),repeat('6',64),'image/png',1,1); RAISE EXCEPTION 'cross-workspace evidence asset accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;

-- Every media mismatch uses fresh bundle/item/asset identities. Only the
-- qualified knowledge-media FK/trigger SQLSTATE is accepted; uniqueness cannot
-- mask MIME or frozen geometry failures.
CREATE FUNCTION pg_temp.assert_evidence_media_rejected(
  label text,bad_media text,bad_width integer,bad_height integer,bad_page integer,bad_bounds jsonb
) RETURNS void LANGUAGE plpgsql AS $$
DECLARE bundle_id uuid:=gen_random_uuid(); item_id uuid:=gen_random_uuid(); asset_id uuid:=gen_random_uuid();
 item jsonb; base jsonb; payload jsonb; sha text; created constant timestamptz:='2026-01-01T00:00:00Z';
BEGIN
 item:=jsonb_build_object('kind','image','evidence_item_id',item_id,
 'document_id','00000000-0000-4000-8000-000000000173','source_chunk_id','00000000-0000-4000-8000-000000000174',
 'product_version_id','00000000-0000-4000-8000-000000000172','workspace_kind','product_line',
 'quote_utf8','proof image','quote_sha256',encode(digest(convert_to('proof image','UTF8'),'sha256'),'hex'),
 'quote_start_offset',0,'quote_end_offset',11,'retrieval_rank',2,'retrieval_contract_version','knowledge-evidence-v2',
 'image_artifact_revision_id','00000000-0000-4000-8000-000000000145',
  'object_ref','objects/'||repeat('6',64),'sha256',repeat('6',64),'media_type',bad_media,'width',bad_width,'height',bad_height,
  'page_ordinal',bad_page,'bounding_region',bad_bounds,'frozen_document_display_name','proof.png');
 base:=jsonb_build_object('schema_version',1,'evidence_bundle_id',bundle_id,'project_id','00000000-0000-4000-8000-000000000010',
  'workspace_id','00000000-0000-4000-8000-0000000000a0','workspace_scope','project_wide','requirement_revision_id','00000000-0000-4000-8000-000000000071',
  'matching_report_id','00000000-0000-4000-8000-000000000141','knowledge_scope_attestation_id',(SELECT id FROM phase0_attestation),
  'knowledge_scope_attestation_sha256',(SELECT content_sha256 FROM phase0_attestation),'items',jsonb_build_array(item),'created_at','2026-01-01T00:00:00Z');
 sha:=encode(digest(convert_to(base::text,'UTF8'),'sha256'),'hex'); payload:=base||jsonb_build_object('bundle_sha256',sha);
 BEGIN
  INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at)
   VALUES(bundle_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000141',payload,sha,created);
  INSERT INTO bid_evidence_bundle_items(id,project_id,workspace_id,evidence_bundle_id,ordinal,item_kind,source_media_revision_id,item_payload,content_sha256)
   VALUES(item_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',bundle_id,0,'image','00000000-0000-4000-8000-000000000145',item,encode(digest(convert_to(item::text,'UTF8'),'sha256'),'hex'));
  INSERT INTO bid_evidence_asset_artifacts(id,project_id,workspace_id,evidence_bundle_id,evidence_item_id,image_artifact_revision_id,object_ref,content_sha256,media_type,width,height,page_ordinal,bounding_region)
   VALUES(asset_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',bundle_id,item_id,'00000000-0000-4000-8000-000000000145','objects/'||repeat('6',64),repeat('6',64),bad_media,bad_width,bad_height,bad_page,bad_bounds);
  RAISE EXCEPTION '% mismatch unexpectedly accepted',label;
 EXCEPTION WHEN foreign_key_violation OR check_violation THEN NULL; END;
END $$;
SELECT pg_temp.assert_evidence_media_rejected('MIME','image/jpeg',1,1,0,'{"left":0,"top":0,"right":1,"bottom":1}');
SELECT pg_temp.assert_evidence_media_rejected('width','image/png',2,1,0,'{"left":0,"top":0,"right":1,"bottom":1}');
SELECT pg_temp.assert_evidence_media_rejected('height','image/png',1,2,0,'{"left":0,"top":0,"right":1,"bottom":1}');
SELECT pg_temp.assert_evidence_media_rejected('page','image/png',1,1,1,'{"left":0,"top":0,"right":1,"bottom":1}');
SELECT pg_temp.assert_evidence_media_rejected('bounds','image/png',1,1,0,'{"left":0.1,"top":0,"right":1,"bottom":1}');

-- Required scalar nulls and non-integer/negative page ordinals fail with a
-- recomputed canonical hash, proving the validator rather than the digest fires.
DO $$ DECLARE source jsonb:=(SELECT canonical_payload FROM bid_evidence_bundle_artifacts WHERE id='00000000-0000-4000-8000-000000000143'); bad jsonb; base jsonb; sha text; new_id uuid; BEGIN
 FOREACH bad IN ARRAY ARRAY[
   jsonb_set(source,'{workspace_scope}','null'::jsonb),
   jsonb_set(source,'{items,0,page_ordinal}','-1'::jsonb),
   jsonb_set(source,'{items,0,page_ordinal}','1.5'::jsonb),
   jsonb_set(source,'{items,0,media_type}','null'::jsonb),
   jsonb_set(source,'{created_at}',to_jsonb('2026-01-01'::text)),
   jsonb_set(source,'{created_at}',to_jsonb('infinity'::text)),
   jsonb_set(source,'{created_at}',to_jsonb('2026-01-01T00:00:00+24:00'::text)),
   jsonb_set(source,'{created_at}',to_jsonb('2026-02-30T00:00:00Z'::text))
 ] LOOP
   new_id:=gen_random_uuid();
   base:=jsonb_set(bad-'bundle_sha256','{evidence_bundle_id}',to_jsonb(new_id::text));
   sha:=encode(digest(convert_to(base::text,'UTF8'),'sha256'),'hex');
   bad:=base||jsonb_build_object('bundle_sha256',sha);
   BEGIN
     INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at)
     VALUES(new_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000141',bad,sha,'2026-01-01T00:00:00Z');
     RAISE EXCEPTION 'malformed evidence scalar accepted';
   EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
END $$;

INSERT INTO bid_render_style_contract_artifacts(id,version,schema_version,canonical_payload,content_sha256) VALUES ('00000000-0000-4000-8000-000000000125',1001,1,convert_to('style1','UTF8'),encode(digest(convert_to('style1','UTF8'),'sha256'),'hex'));

-- Eleventh-round typed request/candidate contracts. Every generic request and
-- its one matching projection are committed atomically.
INSERT INTO bid_authoring_contract_artifacts(id,contract_kind,schema_version,canonical_payload,content_sha256) VALUES
 ('00000000-0000-4000-8000-000000000191','prompt',1,convert_to('prompt1','UTF8'),encode(digest(convert_to('prompt1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000192','template',1,convert_to('template1','UTF8'),encode(digest(convert_to('template1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000193','model',1,convert_to('model1','UTF8'),encode(digest(convert_to('model1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000194','agent',1,convert_to('agent1','UTF8'),encode(digest(convert_to('agent1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-000000000195','matching_policy',1,convert_to('matching-policy1','UTF8'),encode(digest(convert_to('matching-policy1','UTF8'),'sha256'),'hex'));
INSERT INTO bid_evidence_selection_artifacts(id,project_id,workspace_id,selection_kind,matching_report_id,canonical_payload,content_sha256,actor) VALUES
 ('00000000-0000-4000-8000-000000000181','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','user_pick_set','00000000-0000-4000-8000-000000000141',convert_to('pick-set-1','UTF8'),encode(digest(convert_to('pick-set-1','UTF8'),'sha256'),'hex'),'user:00000000-0000-4000-8000-000000000001');

BEGIN;
INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES
 ('00000000-0000-4000-8000-0000000001a1','00000000-0000-4000-8000-000000000010',NULL,'tender_document_process',1,repeat('1',64),convert_to('tender-request-1','UTF8'),encode(digest(convert_to('tender-request-1','UTF8'),'sha256'),'hex'),'pending');
INSERT INTO bid_tender_document_process_request_identities(request_artifact_id,project_id,request_revision,request_sha256,frozen_input_sha256,document_id,document_sha256,role_revision_id,role_revision_sha256,converter_contract_id,converter_contract_sha256)
VALUES('00000000-0000-4000-8000-0000000001a1','00000000-0000-4000-8000-000000000010',1,encode(digest(convert_to('tender-request-1','UTF8'),'sha256'),'hex'),repeat('1',64),'00000000-0000-4000-8000-000000000011',repeat('a',64),'00000000-0000-4000-8000-000000000021',encode(digest(convert_to('role1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000190',encode(digest(convert_to('converter1','UTF8'),'sha256'),'hex'));
COMMIT;

-- The deployed Worker role has no direct bidding-table SELECT. Its narrow
-- SECURITY DEFINER loader must still resolve the complete frozen input.
SET ROLE kb_runtime_worker;
DO $$ DECLARE loaded record; BEGIN
  SELECT * INTO STRICT loaded FROM kb_bid_v2_load_tender_document_process_input(
    '00000000-0000-4000-8000-0000000001a1',1,repeat('1',64));
  IF loaded.document_id<>'00000000-0000-4000-8000-000000000011'::uuid
     OR loaded.original_object_ref<>'objects/'||repeat('a',64) THEN
    RAISE EXCEPTION 'runtime Worker tender loader returned the wrong frozen identity';
  END IF;
END $$;
RESET ROLE;

BEGIN;
INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES
 ('00000000-0000-4000-8000-0000000001a2','00000000-0000-4000-8000-000000000010',NULL,'requirement_set_compile',1,repeat('2',64),convert_to('requirement-request-1','UTF8'),encode(digest(convert_to('requirement-request-1','UTF8'),'sha256'),'hex'),'pending');
INSERT INTO bid_requirement_set_compile_request_identities(request_artifact_id,project_id,request_revision,request_sha256,frozen_input_sha256,document_set_revision_id,document_set_sha256,disposition_set_revision_id,disposition_set_sha256)
VALUES('00000000-0000-4000-8000-0000000001a2','00000000-0000-4000-8000-000000000010',1,encode(digest(convert_to('requirement-request-1','UTF8'),'sha256'),'hex'),repeat('2',64),'00000000-0000-4000-8000-000000000042',encode(digest(convert_to('set2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000063',encode(digest(convert_to('disp3','UTF8'),'sha256'),'hex'));
COMMIT;

BEGIN;
INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES
 ('00000000-0000-4000-8000-0000000001a3','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','outline_generate',1,repeat('3',64),convert_to('outline-request-1','UTF8'),encode(digest(convert_to('outline-request-1','UTF8'),'sha256'),'hex'),'pending');
INSERT INTO bid_outline_generation_request_identities(request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,base_workspace_revision_id,base_workspace_sha256,document_set_revision_id,document_set_sha256,disposition_set_revision_id,disposition_set_sha256,requirement_set_revision_id,requirement_set_sha256,requirement_projection_id,requirement_projection_sha256,scope_revision_id,scope_revision_sha256,prompt_contract_id,prompt_contract_sha256,template_contract_id,template_contract_sha256,model_contract_id,model_contract_sha256,agent_contract_id,agent_contract_sha256)
VALUES('00000000-0000-4000-8000-0000000001a3','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',1,encode(digest(convert_to('outline-request-1','UTF8'),'sha256'),'hex'),repeat('3',64),'00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000042',encode(digest(convert_to('set2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000063',encode(digest(convert_to('disp3','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000083',encode(digest(convert_to('rset-new','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000121',encode(digest(convert_to('scope1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000191',encode(digest(convert_to('prompt1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000192',encode(digest(convert_to('template1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000193',encode(digest(convert_to('model1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000194',encode(digest(convert_to('agent1','UTF8'),'sha256'),'hex'));
INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,request_operation,state,canonical_payload,content_sha256)
VALUES('00000000-0000-4000-8000-0000000001a4','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','outline','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000001a3','outline_generate',1,encode(digest(convert_to('outline-request-1','UTF8'),'sha256'),'hex'),'outline_generate','proposed',convert_to('outline-candidate-1','UTF8'),encode(digest(convert_to('outline-candidate-1','UTF8'),'sha256'),'hex'));
COMMIT;

BEGIN;
INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES
 ('00000000-0000-4000-8000-000000000182','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','content_generate',1,repeat('a',64),convert_to('content-request-1','UTF8'),encode(digest(convert_to('content-request-1','UTF8'),'sha256'),'hex'),'pending');
INSERT INTO bid_content_generation_request_identities(request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,request_operation,base_workspace_revision_id,base_workspace_sha256,requirement_projection_id,requirement_projection_sha256,outline_checkpoint_id,outline_checkpoint_sha256,scope_revision_id,scope_revision_sha256,document_settings_revision_id,document_settings_sha256,render_style_contract_id,render_style_contract_sha256,evidence_selection_mode,evidence_selection_sha256,pick_set_kind,pick_set_artifact_id,pick_set_sha256,pick_set_matching_report_id,quote_snapshot_id,quote_snapshot_sha256,prompt_contract_id,prompt_contract_sha256,template_contract_id,template_contract_sha256,model_contract_id,model_contract_sha256,agent_contract_id,agent_contract_sha256,target_kind,target_node_lineage_id,target_node_revision_id,fill_policy,insertion_node_revision_id,insertion_block_revision_id)
VALUES('00000000-0000-4000-8000-000000000182','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',1,encode(digest(convert_to('content-request-1','UTF8'),'sha256'),'hex'),repeat('a',64),'generate','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-00000000013e',encode(digest(convert_to('checkpoint1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000121',encode(digest(convert_to('scope1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000122',encode(digest(convert_to('settings1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000125',encode(digest(convert_to('style1','UTF8'),'sha256'),'hex'),'user_pick_set',repeat('e',64),'user_pick_set','00000000-0000-4000-8000-000000000181',encode(digest(convert_to('pick-set-1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000141','00000000-0000-4000-8000-0000000000f1',encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000191',encode(digest(convert_to('prompt1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000192',encode(digest(convert_to('template1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000193',encode(digest(convert_to('model1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000194',encode(digest(convert_to('agent1','UTF8'),'sha256'),'hex'),'node','00000000-0000-4000-8000-0000000000c1','00000000-0000-4000-8000-00000000013b','append_candidate','00000000-0000-4000-8000-00000000013b','00000000-0000-4000-8000-0000000000e2');
INSERT INTO bid_content_generation_request_evidence_bundles(request_artifact_id,project_id,workspace_id,ordinal,evidence_bundle_id,evidence_bundle_sha256)
 SELECT '00000000-0000-4000-8000-000000000182',project_id,workspace_id,0,id,content_sha256 FROM bid_evidence_bundle_artifacts WHERE id='00000000-0000-4000-8000-000000000143';
INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,request_operation,state,canonical_payload,content_sha256)
VALUES('00000000-0000-4000-8000-000000000183','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','content','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000182','content_generate',1,encode(digest(convert_to('content-request-1','UTF8'),'sha256'),'hex'),'generate','proposed',convert_to('content-candidate-1','UTF8'),encode(digest(convert_to('content-candidate-1','UTF8'),'sha256'),'hex'));
COMMIT;

-- System-proposed match-only is a direct positive path and cannot publish a candidate.
BEGIN;
INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES
 ('00000000-0000-4000-8000-0000000001a5','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','content_generate',1,repeat('5',64),convert_to('content-system-1','UTF8'),encode(digest(convert_to('content-system-1','UTF8'),'sha256'),'hex'),'pending');
INSERT INTO bid_content_generation_request_identities(request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,request_operation,base_workspace_revision_id,base_workspace_sha256,requirement_projection_id,requirement_projection_sha256,outline_checkpoint_id,outline_checkpoint_sha256,scope_revision_id,scope_revision_sha256,document_settings_revision_id,document_settings_sha256,render_style_contract_id,render_style_contract_sha256,evidence_selection_mode,evidence_selection_sha256,matching_policy_id,matching_policy_sha256,prompt_contract_id,prompt_contract_sha256,template_contract_id,template_contract_sha256,model_contract_id,model_contract_sha256,agent_contract_id,agent_contract_sha256,target_kind,target_workspace_revision_id,fill_policy)
VALUES('00000000-0000-4000-8000-0000000001a5','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',1,encode(digest(convert_to('content-system-1','UTF8'),'sha256'),'hex'),repeat('5',64),'match_only','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-00000000013e',encode(digest(convert_to('checkpoint1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000121',encode(digest(convert_to('scope1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000122',encode(digest(convert_to('settings1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000125',encode(digest(convert_to('style1','UTF8'),'sha256'),'hex'),'system_proposed',repeat('5',64),'00000000-0000-4000-8000-000000000195',encode(digest(convert_to('matching-policy1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000191',encode(digest(convert_to('prompt1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000192',encode(digest(convert_to('template1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000193',encode(digest(convert_to('model1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000194',encode(digest(convert_to('agent1','UTF8'),'sha256'),'hex'),'workspace','00000000-0000-4000-8000-000000000135','empty_only');
COMMIT;

CREATE FUNCTION pg_temp.clone_content_request(p_id uuid,p_payload text,p_override jsonb)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE request_sha kb_sha256:=encode(digest(convert_to(p_payload,'UTF8'),'sha256'),'hex');
BEGIN
 INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status)
 VALUES(p_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','content_generate',1,request_sha,convert_to(p_payload,'UTF8'),request_sha,'pending');
 INSERT INTO bid_content_generation_request_identities
 SELECT (jsonb_populate_record(NULL::bid_content_generation_request_identities,
   to_jsonb(source)||jsonb_build_object('request_artifact_id',p_id,'request_sha256',request_sha,'frozen_input_sha256',request_sha)||p_override)).*
 FROM bid_content_generation_request_identities source WHERE request_artifact_id='00000000-0000-4000-8000-000000000182';
END $$;

-- Each ContentGenerate scalar/mode/target/anchor negative uses a fresh request,
-- so no uniqueness error can mask the named two-valued constraint.
DO $$ DECLARE id uuid; BEGIN
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'null-operation',jsonb_build_object('request_operation',NULL)); RAISE EXCEPTION 'NULL ContentGenerate operation accepted'; EXCEPTION WHEN not_null_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'null-selection-digest',jsonb_build_object('evidence_selection_sha256',NULL)); RAISE EXCEPTION 'NULL evidence selection digest accepted'; EXCEPTION WHEN not_null_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'manual-policy-mix',jsonb_build_object('matching_policy_id','00000000-0000-4000-8000-000000000195','matching_policy_sha256',encode(digest(convert_to('matching-policy1','UTF8'),'sha256'),'hex'))); RAISE EXCEPTION 'manual PickSet accepted system policy fields'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'system-policy-missing',jsonb_build_object('evidence_selection_mode','system_proposed','pick_set_kind',NULL,'pick_set_artifact_id',NULL,'pick_set_sha256',NULL,'pick_set_matching_report_id',NULL,'matching_policy_id',NULL,'matching_policy_sha256',NULL)); RAISE EXCEPTION 'system selection without matching policy accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'system-policy-kind',jsonb_build_object('evidence_selection_mode','system_proposed','pick_set_kind',NULL,'pick_set_artifact_id',NULL,'pick_set_sha256',NULL,'pick_set_matching_report_id',NULL,'matching_policy_id','00000000-0000-4000-8000-000000000191','matching_policy_sha256',encode(digest(convert_to('prompt1','UTF8'),'sha256'),'hex'))); RAISE EXCEPTION 'wrong matching-policy contract kind accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'manual-report-splice',jsonb_build_object('pick_set_matching_report_id',gen_random_uuid())); RAISE EXCEPTION 'manual PickSet accepted another MatchingReport'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'bad-fill-policy',jsonb_build_object('fill_policy','overwrite')); RAISE EXCEPTION 'unknown fill policy accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'fill-anchor-mismatch',jsonb_build_object('fill_policy','empty_only')); RAISE EXCEPTION 'non-append fill accepted insertion anchor'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'anchor-outside-target',jsonb_build_object('target_kind','subtree','target_node_lineage_id','00000000-0000-4000-8000-0000000001d2','target_node_revision_id','00000000-0000-4000-8000-0000000001d0','insertion_node_revision_id','00000000-0000-4000-8000-00000000013b','insertion_block_revision_id',NULL)); RAISE EXCEPTION 'anchor outside frozen target subtree accepted'; EXCEPTION WHEN check_violation THEN IF SQLERRM NOT LIKE 'CONTENT_GENERATION_INPUT_INVALID:%' THEN RAISE; END IF; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_content_request(id,'workspace-target-null',jsonb_build_object('target_kind','workspace','target_node_lineage_id',NULL,'target_node_revision_id',NULL,'target_workspace_revision_id',NULL,'insertion_node_revision_id',NULL,'insertion_block_revision_id',NULL)); RAISE EXCEPTION 'workspace target without frozen revision accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
END $$;

BEGIN;
INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES
 ('00000000-0000-4000-8000-0000000001a6','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','submission_export',1,repeat('6',64),convert_to('export-request-1','UTF8'),encode(digest(convert_to('export-request-1','UTF8'),'sha256'),'hex'),'pending');
INSERT INTO bid_submission_export_request_identities(request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,workspace_revision_id,workspace_sha256,outline_checkpoint_id,outline_checkpoint_sha256,requirement_projection_id,requirement_projection_sha256,scope_revision_id,scope_revision_sha256,document_settings_revision_id,document_settings_sha256,render_style_contract_id,render_style_contract_sha256,output_mode,format,mode_options)
VALUES('00000000-0000-4000-8000-0000000001a6','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',1,encode(digest(convert_to('export-request-1','UTF8'),'sha256'),'hex'),repeat('6',64),'00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-00000000013e',encode(digest(convert_to('checkpoint1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000121',encode(digest(convert_to('scope1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000122',encode(digest(convert_to('settings1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000125',encode(digest(convert_to('style1','UTF8'),'sha256'),'hex'),'submission','pdf','{"watermark":null}');
COMMIT;

-- The second valid SubmissionExport branch freezes review-draft options.
BEGIN;
INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES
 ('00000000-0000-4000-8000-0000000001a7','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','submission_export',1,repeat('7',64),convert_to('export-review-draft-1','UTF8'),encode(digest(convert_to('export-review-draft-1','UTF8'),'sha256'),'hex'),'pending');
INSERT INTO bid_submission_export_request_identities(request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,workspace_revision_id,workspace_sha256,outline_checkpoint_id,outline_checkpoint_sha256,requirement_projection_id,requirement_projection_sha256,scope_revision_id,scope_revision_sha256,document_settings_revision_id,document_settings_sha256,render_style_contract_id,render_style_contract_sha256,output_mode,format,mode_options)
VALUES('00000000-0000-4000-8000-0000000001a7','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',1,encode(digest(convert_to('export-review-draft-1','UTF8'),'sha256'),'hex'),repeat('7',64),'00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-00000000013e',encode(digest(convert_to('checkpoint1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000121',encode(digest(convert_to('scope1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000122',encode(digest(convert_to('settings1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000125',encode(digest(convert_to('style1','UTF8'),'sha256'),'hex'),'review_draft','docx','{"watermark":"REVIEW DRAFT"}');
COMMIT;

CREATE FUNCTION pg_temp.clone_export_request(p_id uuid,p_payload text,p_mode text,p_format text,p_options jsonb)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE request_sha kb_sha256:=encode(digest(convert_to(p_payload,'UTF8'),'sha256'),'hex');
BEGIN
 INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status)
 VALUES(p_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','submission_export',1,request_sha,convert_to(p_payload,'UTF8'),request_sha,'pending');
 INSERT INTO bid_submission_export_request_identities
 SELECT (jsonb_populate_record(NULL::bid_submission_export_request_identities,
   to_jsonb(source)||jsonb_build_object('request_artifact_id',p_id,'request_sha256',request_sha,'frozen_input_sha256',request_sha,'output_mode',p_mode,'format',p_format,'mode_options',p_options))).*
 FROM bid_submission_export_request_identities source WHERE request_artifact_id='00000000-0000-4000-8000-0000000001a6';
END $$;

-- SubmissionExport mode options are a closed, watermark-only contract.
DO $$ DECLARE id uuid; BEGIN
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_export_request(id,'export-extra-options','submission','pdf','{"watermark":null,"extra":true}'); RAISE EXCEPTION 'extra export option accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_export_request(id,'export-missing-options','submission','pdf','{}'); RAISE EXCEPTION 'missing export option accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_export_request(id,'export-watermark-type','review_draft','docx','{"watermark":1}'); RAISE EXCEPTION 'numeric export watermark accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_export_request(id,'export-empty-watermark','review_draft','docx','{"watermark":""}'); RAISE EXCEPTION 'empty draft watermark accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 id:=gen_random_uuid(); BEGIN PERFORM pg_temp.clone_export_request(id,'export-submission-watermark','submission','pdf','{"watermark":"DRAFT"}'); RAISE EXCEPTION 'submission watermark accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
END $$;

-- Initial rows cannot bypass one-way transitions by starting terminal.
DO $$ DECLARE terminal_status text; failure_message text; BEGIN
 FOR terminal_status IN SELECT unnest(ARRAY['succeeded','failed']) LOOP
  BEGIN
   INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status,result_identity,error_code,finished_at)
   VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010',NULL,'tender_document_process',1,repeat('8',64),convert_to('terminal-request-'||terminal_status,'UTF8'),encode(digest(convert_to('terminal-request-'||terminal_status,'UTF8'),'sha256'),'hex'),terminal_status,CASE WHEN terminal_status='succeeded' THEN '{}'::jsonb ELSE NULL END,CASE WHEN terminal_status='failed' THEN 'INPUT_SCHEMA_INVALID' ELSE NULL END,now());
   RAISE EXCEPTION 'terminal request insert accepted: %',terminal_status;
  EXCEPTION WHEN check_violation THEN
   GET STACKED DIAGNOSTICS failure_message=MESSAGE_TEXT;
   IF failure_message<>'async request initial status must be pending' THEN RAISE; END IF;
  END;
 END LOOP;
 FOR terminal_status IN SELECT unnest(ARRAY['accepted','rejected']) LOOP
  BEGIN
   INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,request_operation,state,canonical_payload,content_sha256,decided_at)
   VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','outline','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000001a3','outline_generate',1,encode(digest(convert_to('outline-request-1','UTF8'),'sha256'),'hex'),'outline_generate',terminal_status,convert_to('terminal-candidate-'||terminal_status,'UTF8'),encode(digest(convert_to('terminal-candidate-'||terminal_status,'UTF8'),'sha256'),'hex'),now());
   RAISE EXCEPTION 'terminal candidate insert accepted: %',terminal_status;
  EXCEPTION WHEN check_violation THEN
   GET STACKED DIAGNOSTICS failure_message=MESSAGE_TEXT;
   IF failure_message<>'candidate initial state must be proposed and undecided' THEN RAISE; END IF;
  END;
 END LOOP;
 BEGIN
  INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,request_operation,state,canonical_payload,content_sha256,decided_at)
  VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','outline','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000001a3','outline_generate',1,encode(digest(convert_to('outline-request-1','UTF8'),'sha256'),'hex'),'outline_generate','proposed',convert_to('pre-decided-candidate','UTF8'),encode(digest(convert_to('pre-decided-candidate','UTF8'),'sha256'),'hex'),now());
  RAISE EXCEPTION 'pre-decided proposed candidate insert accepted';
 EXCEPTION WHEN check_violation THEN
  GET STACKED DIAGNOSTICS failure_message=MESSAGE_TEXT;
  IF failure_message<>'candidate initial state must be proposed and undecided' THEN RAISE; END IF;
 END;
END $$;

-- Missing/wrong/multiple projection paths fail with the exact deferred or FK state.
DO $$ BEGIN
 BEGIN
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010',NULL,'tender_document_process',1,repeat('7',64),convert_to('missing-projection','UTF8'),encode(digest(convert_to('missing-projection','UTF8'),'sha256'),'hex'),'pending');
  SET CONSTRAINTS ALL IMMEDIATE;
  RAISE EXCEPTION 'request without typed projection accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status) VALUES('00000000-0000-4000-8000-0000000001b1','00000000-0000-4000-8000-000000000010',NULL,'tender_document_process',1,repeat('8',64),convert_to('wrong-projection','UTF8'),encode(digest(convert_to('wrong-projection','UTF8'),'sha256'),'hex'),'pending');
  INSERT INTO bid_requirement_set_compile_request_identities(request_artifact_id,project_id,request_revision,request_sha256,frozen_input_sha256,document_set_revision_id,document_set_sha256,disposition_set_revision_id,disposition_set_sha256) VALUES('00000000-0000-4000-8000-0000000001b1','00000000-0000-4000-8000-000000000010',1,encode(digest(convert_to('wrong-projection','UTF8'),'sha256'),'hex'),repeat('8',64),'00000000-0000-4000-8000-000000000042',encode(digest(convert_to('set2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000063',encode(digest(convert_to('disp3','UTF8'),'sha256'),'hex'));
  RAISE EXCEPTION 'wrong-kind projection accepted';
 EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN
  INSERT INTO bid_requirement_set_compile_request_identities SELECT '00000000-0000-4000-8000-0000000001a1',project_id,'requirement_set_compile',request_revision,request_sha256,frozen_input_sha256,'00000000-0000-4000-8000-000000000042',encode(digest(convert_to('set2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000063',encode(digest(convert_to('disp3','UTF8'),'sha256'),'hex') FROM bid_tender_document_process_request_identities WHERE request_artifact_id='00000000-0000-4000-8000-0000000001a1';
  RAISE EXCEPTION 'multiple typed projections accepted';
 EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;

-- Frozen request/projection and closed terminal transitions.
DO $$ BEGIN
 BEGIN UPDATE bid_async_request_snapshot_artifacts SET request_payload=convert_to('mutated','UTF8'),request_sha256=encode(digest(convert_to('mutated','UTF8'),'sha256'),'hex') WHERE id='00000000-0000-4000-8000-0000000001a2'; RAISE EXCEPTION 'request payload mutation accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE bid_requirement_set_compile_request_identities SET frozen_input_sha256=repeat('f',64) WHERE request_artifact_id='00000000-0000-4000-8000-0000000001a2'; RAISE EXCEPTION 'typed projection mutation accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN DELETE FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id='00000000-0000-4000-8000-0000000001a2'; RAISE EXCEPTION 'typed projection delete accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN EXECUTE 'TRUNCATE bid_requirement_set_compile_request_identities'; RAISE EXCEPTION 'typed projection truncate accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity='{}',finished_at=now() WHERE id='00000000-0000-4000-8000-0000000001a1';
 BEGIN UPDATE bid_async_request_snapshot_artifacts SET status='pending',result_identity=NULL,finished_at=NULL WHERE id='00000000-0000-4000-8000-0000000001a1'; RAISE EXCEPTION 'terminal request reopened'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN DELETE FROM bid_async_request_snapshot_artifacts WHERE id='00000000-0000-4000-8000-0000000001a2'; RAISE EXCEPTION 'request delete accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN EXECUTE 'TRUNCATE bid_async_request_snapshot_artifacts CASCADE'; RAISE EXCEPTION 'request truncate accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

DO $$ DECLARE relation_name text; request_id uuid; BEGIN
 FOR relation_name,request_id IN VALUES
  ('bid_tender_document_process_request_identities','00000000-0000-4000-8000-0000000001a1'::uuid),
  ('bid_requirement_set_compile_request_identities','00000000-0000-4000-8000-0000000001a2'::uuid),
  ('bid_outline_generation_request_identities','00000000-0000-4000-8000-0000000001a3'::uuid),
  ('bid_content_generation_request_identities','00000000-0000-4000-8000-000000000182'::uuid),
  ('bid_submission_export_request_identities','00000000-0000-4000-8000-0000000001a6'::uuid)
 LOOP
  BEGIN EXECUTE format('UPDATE %I SET request_artifact_id=request_artifact_id WHERE request_artifact_id=%L',relation_name,request_id); RAISE EXCEPTION '% projection update accepted',relation_name; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN EXECUTE format('DELETE FROM %I WHERE request_artifact_id=%L',relation_name,request_id); RAISE EXCEPTION '% projection delete accepted',relation_name; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN EXECUTE format('TRUNCATE %I CASCADE',relation_name); RAISE EXCEPTION '% projection truncate accepted',relation_name; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
END $$;

-- Candidate exact-input and one-way decision semantics.
DO $$ BEGIN
 BEGIN INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,request_operation,state,canonical_payload,content_sha256) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','outline','00000000-0000-4000-8000-000000000123',encode(digest(convert_to('{"fixture":1}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000001a3','outline_generate',1,encode(digest(convert_to('outline-request-1','UTF8'),'sha256'),'hex'),'outline_generate','proposed',convert_to('bad-outline-candidate','UTF8'),encode(digest(convert_to('bad-outline-candidate','UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'outline candidate cross-input accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,request_operation,state,canonical_payload,content_sha256) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','content','00000000-0000-4000-8000-000000000123',encode(digest(convert_to('{"fixture":1}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000182','content_generate',1,encode(digest(convert_to('content-request-1','UTF8'),'sha256'),'hex'),'generate','proposed',convert_to('content-cross-base','UTF8'),encode(digest(convert_to('content-cross-base','UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'content candidate cross-input accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,request_operation,state,canonical_payload,content_sha256) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','content','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000001a5','content_generate',1,encode(digest(convert_to('content-system-1','UTF8'),'sha256'),'hex'),'generate','proposed',convert_to('match-only-candidate','UTF8'),encode(digest(convert_to('match-only-candidate','UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'match-only request produced candidate'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,request_operation,state,canonical_payload,content_sha256) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','content','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000182','content_generate',1,encode(digest(convert_to('content-request-1','UTF8'),'sha256'),'hex'),NULL,'proposed',convert_to('null-operation','UTF8'),encode(digest(convert_to('null-operation','UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'NULL candidate operation accepted'; EXCEPTION WHEN not_null_violation THEN NULL; END;
 BEGIN UPDATE bid_candidate_artifacts SET canonical_payload=convert_to('mutated','UTF8'),content_sha256=encode(digest(convert_to('mutated','UTF8'),'sha256'),'hex') WHERE id='00000000-0000-4000-8000-000000000183'; RAISE EXCEPTION 'candidate frozen mutation accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 UPDATE bid_candidate_artifacts SET state='accepted',decided_at=now() WHERE id='00000000-0000-4000-8000-000000000183';
 BEGIN UPDATE bid_candidate_artifacts SET state='proposed',decided_at=NULL WHERE id='00000000-0000-4000-8000-000000000183'; RAISE EXCEPTION 'candidate reopened'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN UPDATE bid_candidate_artifacts SET state='rejected',decided_at=now() WHERE id='00000000-0000-4000-8000-000000000183'; RAISE EXCEPTION 'candidate terminal state switched'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN DELETE FROM bid_candidate_artifacts WHERE id='00000000-0000-4000-8000-0000000001a4'; RAISE EXCEPTION 'candidate delete accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN EXECUTE 'TRUNCATE bid_candidate_artifacts CASCADE'; RAISE EXCEPTION 'candidate truncate accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

INSERT INTO bid_renderer_contract_artifacts(id,format,version,schema_version,canonical_payload,content_sha256,approved_at) VALUES
('00000000-0000-4000-8000-000000000128','docx',99,1,convert_to('renderer-docx','UTF8'),encode(digest(convert_to('renderer-docx','UTF8'),'sha256'),'hex'),now()),
('00000000-0000-4000-8000-000000000129','pdf',99,1,convert_to('renderer-pdf','UTF8'),encode(digest(convert_to('renderer-pdf','UTF8'),'sha256'),'hex'),now());
CREATE FUNCTION pg_temp.preparation_payload(p_id uuid,p_source uuid,p_revision bigint,p_status text,p_pages jsonb)
RETURNS jsonb LANGUAGE sql AS $$
 WITH base AS (SELECT jsonb_build_object('schema_version',1,'attachment_preparation_revision_id',p_id,
  'project_id','00000000-0000-4000-8000-000000000010','workspace_id','00000000-0000-4000-8000-0000000000a0',
  'source_asset_revision_id',p_source,'revision',p_revision,'status',p_status,'page_assets',p_pages) p)
 SELECT p||jsonb_build_object('preparation_sha256',encode(digest(convert_to(p::text,'UTF8'),'sha256'),'hex')) FROM base $$;
DO $$ DECLARE page jsonb:=jsonb_build_object('page_asset_id','00000000-0000-4000-8000-000000000151','page_number',1,
 'object_ref','objects/'||repeat('9',64),'sha256',repeat('9',64),'media_type','image/png','geometry',jsonb_build_object('width_px',100,'height_px',200));
 payload jsonb; BEGIN
 payload:=pg_temp.preparation_payload('00000000-0000-4000-8000-00000000012a','00000000-0000-4000-8000-000000000150',1,'ready',jsonb_build_array(page));
 INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,revision,status,page_assets,canonical_payload,preparation_sha256)
  VALUES ('00000000-0000-4000-8000-00000000012a','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000150',1,'ready',payload->'page_assets',payload,payload->>'preparation_sha256');
 INSERT INTO bid_attachment_preparation_asset_items(id,project_id,workspace_id,attachment_preparation_revision_id,ordinal,page_number,object_ref,content_sha256,media_type,geometry)
  VALUES ('00000000-0000-4000-8000-000000000151','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-00000000012a',0,1,'objects/'||repeat('9',64),repeat('9',64),'image/png','{"width_px":100,"height_px":200}');
 SET CONSTRAINTS ALL IMMEDIATE;
END $$;
DO $$ DECLARE id uuid; source uuid; page1 jsonb; page2 jsonb; payload jsonb; BEGIN
 -- Unknown source identity.
 id:=gen_random_uuid(); source:=gen_random_uuid(); page1:=jsonb_build_object('page_asset_id',gen_random_uuid(),'page_number',1,'object_ref','objects/'||repeat('9',64),'sha256',repeat('9',64),'media_type','image/png','geometry',jsonb_build_object('width_px',100,'height_px',200)); payload:=pg_temp.preparation_payload(id,source,1,'ready',jsonb_build_array(page1));
 BEGIN INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,revision,status,page_assets,canonical_payload,preparation_sha256) VALUES(id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',source,1,'ready',payload->'page_assets',payload,payload->>'preparation_sha256'); RAISE EXCEPTION 'unknown preparation source accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 -- Missing ordered projection.
 id:=gen_random_uuid(); page1:=jsonb_set(page1,'{page_asset_id}',to_jsonb(gen_random_uuid()::text)); payload:=pg_temp.preparation_payload(id,'00000000-0000-4000-8000-000000000150',2,'ready',jsonb_build_array(page1));
 BEGIN INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,revision,status,page_assets,canonical_payload,preparation_sha256) VALUES(id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000150',2,'ready',payload->'page_assets',payload,payload->>'preparation_sha256'); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'missing preparation projection accepted'; EXCEPTION WHEN check_violation THEN NULL; END; SET CONSTRAINTS ALL DEFERRED;
 -- Canonical hash mismatch with otherwise valid payload.
 id:=gen_random_uuid(); page1:=jsonb_set(page1,'{page_asset_id}',to_jsonb(gen_random_uuid()::text)); payload:=pg_temp.preparation_payload(id,'00000000-0000-4000-8000-000000000150',3,'ready',jsonb_build_array(page1));
 BEGIN INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,revision,status,page_assets,canonical_payload,preparation_sha256) VALUES(id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000150',3,'ready',payload->'page_assets',payload,repeat('f',64)); RAISE EXCEPTION 'wrong preparation canonical hash accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 -- Extra page projection against the valid frozen artifact.
 BEGIN INSERT INTO bid_attachment_preparation_asset_items(id,project_id,workspace_id,attachment_preparation_revision_id,ordinal,page_number,object_ref,content_sha256,media_type,geometry) VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-00000000012a',1,2,'objects/'||repeat('9',64),repeat('9',64),'image/png','{"width_px":100,"height_px":200}'); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'extra preparation page accepted'; EXCEPTION WHEN check_violation THEN NULL; END; SET CONSTRAINTS ALL DEFERRED;
 -- Reordered two-page projection.
 id:=gen_random_uuid(); page1:=jsonb_build_object('page_asset_id',gen_random_uuid(),'page_number',1,'object_ref','objects/'||repeat('9',64),'sha256',repeat('9',64),'media_type','image/png','geometry',jsonb_build_object('width_px',100,'height_px',200)); page2:=jsonb_build_object('page_asset_id',gen_random_uuid(),'page_number',2,'object_ref','objects/'||repeat('9',64),'sha256',repeat('9',64),'media_type','image/png','geometry',jsonb_build_object('width_px',100,'height_px',200)); payload:=pg_temp.preparation_payload(id,'00000000-0000-4000-8000-000000000150',4,'ready',jsonb_build_array(page1,page2));
 BEGIN INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,revision,status,page_assets,canonical_payload,preparation_sha256) VALUES(id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000150',4,'ready',payload->'page_assets',payload,payload->>'preparation_sha256'); INSERT INTO bid_attachment_preparation_asset_items(id,project_id,workspace_id,attachment_preparation_revision_id,ordinal,page_number,object_ref,content_sha256,media_type,geometry) VALUES((page2->>'page_asset_id')::uuid,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',id,0,2,'objects/'||repeat('9',64),repeat('9',64),'image/png',page2->'geometry'),((page1->>'page_asset_id')::uuid,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',id,1,1,'objects/'||repeat('9',64),repeat('9',64),'image/png',page1->'geometry'); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'reordered preparation pages accepted'; EXCEPTION WHEN check_violation THEN NULL; END; SET CONSTRAINTS ALL DEFERRED;
 -- Digest and geometry each diverge while every row identity remains otherwise valid.
 id:=gen_random_uuid(); page1:=jsonb_build_object('page_asset_id',gen_random_uuid(),'page_number',1,'object_ref','objects/'||repeat('9',64),'sha256',repeat('9',64),'media_type','image/png','geometry',jsonb_build_object('width_px',100,'height_px',200)); payload:=pg_temp.preparation_payload(id,'00000000-0000-4000-8000-000000000150',5,'ready',jsonb_build_array(page1));
 BEGIN INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,revision,status,page_assets,canonical_payload,preparation_sha256) VALUES(id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000150',5,'ready',payload->'page_assets',payload,payload->>'preparation_sha256'); INSERT INTO bid_attachment_preparation_asset_items(id,project_id,workspace_id,attachment_preparation_revision_id,ordinal,page_number,object_ref,content_sha256,media_type,geometry) VALUES((page1->>'page_asset_id')::uuid,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',id,0,1,'objects/'||repeat('6',64),repeat('6',64),'image/png',page1->'geometry'); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'preparation page digest mismatch accepted'; EXCEPTION WHEN check_violation THEN NULL; END; SET CONSTRAINTS ALL DEFERRED;
 id:=gen_random_uuid(); page1:=jsonb_set(page1,'{page_asset_id}',to_jsonb(gen_random_uuid()::text)); payload:=pg_temp.preparation_payload(id,'00000000-0000-4000-8000-000000000150',6,'ready',jsonb_build_array(page1));
 BEGIN INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,revision,status,page_assets,canonical_payload,preparation_sha256) VALUES(id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000150',6,'ready',payload->'page_assets',payload,payload->>'preparation_sha256'); INSERT INTO bid_attachment_preparation_asset_items(id,project_id,workspace_id,attachment_preparation_revision_id,ordinal,page_number,object_ref,content_sha256,media_type,geometry) VALUES((page1->>'page_asset_id')::uuid,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',id,0,1,'objects/'||repeat('9',64),repeat('9',64),'image/png','{"width_px":101,"height_px":200}'); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'preparation page geometry mismatch accepted'; EXCEPTION WHEN check_violation THEN NULL; END; SET CONSTRAINTS ALL DEFERRED;
 BEGIN UPDATE bid_attachment_preparation_revision_artifacts preparation SET status=preparation.status WHERE preparation.id='00000000-0000-4000-8000-00000000012a'; RAISE EXCEPTION 'preparation update accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN DELETE FROM bid_attachment_preparation_asset_items page WHERE page.id='00000000-0000-4000-8000-000000000151'; RAISE EXCEPTION 'preparation page delete accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN TRUNCATE bid_attachment_preparation_asset_items; RAISE EXCEPTION 'preparation page truncate accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

CREATE FUNCTION pg_temp.render_payload_base(p_id uuid) RETURNS jsonb LANGUAGE sql AS $$ SELECT jsonb_build_object(
 'schema_version',2,'render_snapshot_id',p_id,'project_id','00000000-0000-4000-8000-000000000010','project_title','Project','workspace_id','00000000-0000-4000-8000-0000000000a0','workspace_scope','project_wide','workspace_scope_revision_id','00000000-0000-4000-8000-000000000121','workspace_revision_id','00000000-0000-4000-8000-000000000135','workspace_sha256',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'outline_checkpoint_id','00000000-0000-4000-8000-00000000013e','outline_checkpoint_sha256',encode(digest(convert_to('checkpoint1','UTF8'),'sha256'),'hex'),'requirement_projection_revision_id','00000000-0000-4000-8000-0000000000b2','requirement_projection_sha256',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'document_settings_revision_id','00000000-0000-4000-8000-000000000122','document_settings_sha256',encode(digest(convert_to('settings1','UTF8'),'sha256'),'hex'),'submission_assessment_snapshot_id','00000000-0000-4000-8000-000000000124','submission_assessment_snapshot_sha256',encode(digest(convert_to('assessment1','UTF8'),'sha256'),'hex'),'output_mode','review_draft','format','pdf','mode_options',jsonb_build_object('watermark','DRAFT'),
 'ordered_nodes',jsonb_build_array(jsonb_build_object('node_occurrence_id','00000000-0000-4000-8000-00000000013c','node_revision_id','00000000-0000-4000-8000-00000000013b','parent_occurrence_id',NULL,'ordinal',0,'depth',0,'title','Response','render_role','section','block_occurrences',jsonb_build_array(jsonb_build_object('block_occurrence_id','00000000-0000-4000-8000-00000000013d','block_revision_id','00000000-0000-4000-8000-0000000000e2','ordinal',0,'block_sha256',encode(digest(convert_to('text1','UTF8'),'sha256'),'hex'))))),
 'assets',jsonb_build_array(
  jsonb_build_object('asset_revision_id','00000000-0000-4000-8000-000000000146','object_ref','objects/'||repeat('6',64),'sha256',repeat('6',64),'media_type','image/png','provenance','knowledge_evidence'),
  jsonb_build_object('asset_revision_id','00000000-0000-4000-8000-000000000150','object_ref','objects/'||repeat('8',64),'sha256',repeat('8',64),'media_type','image/jpeg','provenance','manual_workspace'),
  jsonb_build_object('asset_revision_id','00000000-0000-4000-8000-000000000151','object_ref','objects/'||repeat('9',64),'sha256',repeat('9',64),'media_type','image/png','provenance','prepared_attachment'),
  jsonb_build_object('asset_revision_id','00000000-0000-4000-8000-0000000000f1','object_ref','objects/'||encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),'sha256',encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),'media_type','application/json','provenance','quote_snapshot')),
 'form_definition_occurrences',jsonb_build_array(jsonb_build_object('form_definition_revision_id','00000000-0000-4000-8000-000000000053','canonical_sha256',encode(digest(convert_to('form1','UTF8'),'sha256'),'hex'))),'attachment_preparation_occurrences',jsonb_build_array(jsonb_build_object('attachment_preparation_revision_id','00000000-0000-4000-8000-00000000012a','status','ready','canonical_sha256',(SELECT preparation_sha256 FROM bid_attachment_preparation_revision_artifacts WHERE id='00000000-0000-4000-8000-00000000012a'))),'content_block_schema_version',1,'content_block_schema_sha256',repeat('d',64),'render_operation_contract_version',1,'render_operation_contract_sha256',repeat('e',64),'docx_renderer_contract_id','00000000-0000-4000-8000-000000000128','docx_renderer_contract_sha256',encode(digest(convert_to('renderer-docx','UTF8'),'sha256'),'hex'),'pdf_renderer_contract_id','00000000-0000-4000-8000-000000000129','pdf_renderer_contract_sha256',encode(digest(convert_to('renderer-pdf','UTF8'),'sha256'),'hex'),'style_contract_id','00000000-0000-4000-8000-000000000125','style_contract_sha256',encode(digest(convert_to('style1','UTF8'),'sha256'),'hex'),'page_geometry',jsonb_build_object('page_size','A4','width_mm',210,'height_mm',297,'margins_mm',jsonb_build_object('top',20,'right',20,'bottom',20,'left',20)),'font_artifact_identities',jsonb_build_array(jsonb_build_object('font_artifact_id','00000000-0000-4000-8000-000000000147','object_ref','objects/'||repeat('7',64),'sha256',repeat('7',64),'media_type','font/ttf','family','Noto Sans JP','script','cjk')),'numbering_policy','decimal','toc_policy','included') $$;
CREATE FUNCTION pg_temp.render_payload(p_id uuid) RETURNS jsonb LANGUAGE sql AS $$ WITH base AS (SELECT pg_temp.render_payload_base(p_id) p) SELECT p||jsonb_build_object('snapshot_sha256',encode(digest(convert_to(p::text,'UTF8'),'sha256'),'hex')) FROM base $$;

DO $$ DECLARE p jsonb:=pg_temp.render_payload('00000000-0000-4000-8000-000000000127'); sha text:=p->>'snapshot_sha256'; BEGIN
 INSERT INTO bid_render_document_snapshot_artifacts(id,project_id,workspace_id,schema_version,workspace_revision_id,workspace_sha256,scope_revision_id,outline_checkpoint_id,outline_checkpoint_sha256,requirement_projection_id,requirement_projection_sha256,document_settings_revision_id,document_settings_sha256,submission_assessment_snapshot_id,submission_assessment_snapshot_sha256,output_mode,format,mode_options,content_block_schema_version,content_block_schema_sha256,render_operation_contract_version,render_operation_contract_sha256,docx_renderer_contract_id,docx_renderer_contract_sha256,pdf_renderer_contract_id,pdf_renderer_contract_sha256,style_contract_id,style_contract_sha256,page_size,page_width_mm,page_height_mm,margins_mm,numbering_policy,toc_policy,canonical_payload,content_sha256)
 VALUES('00000000-0000-4000-8000-000000000127','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',2,'00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000121','00000000-0000-4000-8000-00000000013e',encode(digest(convert_to('checkpoint1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000122',encode(digest(convert_to('settings1','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000124',encode(digest(convert_to('assessment1','UTF8'),'sha256'),'hex'),'review_draft','pdf','{"watermark":"DRAFT"}',1,repeat('d',64),1,repeat('e',64),'00000000-0000-4000-8000-000000000128',encode(digest(convert_to('renderer-docx','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000129',encode(digest(convert_to('renderer-pdf','UTF8'),'sha256'),'hex'),'00000000-0000-4000-8000-000000000125',encode(digest(convert_to('style1','UTF8'),'sha256'),'hex'),'A4',210,297,'{"top":20,"right":20,"bottom":20,"left":20}','decimal','included',p,sha);
 INSERT INTO bid_render_snapshot_node_occurrences VALUES('00000000-0000-4000-8000-000000000127','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-00000000013c','00000000-0000-4000-8000-00000000013b',0);
 INSERT INTO bid_render_snapshot_block_occurrences VALUES('00000000-0000-4000-8000-000000000127','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000135','00000000-0000-4000-8000-00000000013c','00000000-0000-4000-8000-00000000013d','00000000-0000-4000-8000-0000000000e2',encode(digest(convert_to('text1','UTF8'),'sha256'),'hex'),0);
 INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance) SELECT '00000000-0000-4000-8000-000000000127',ordinal-1,(a->>'asset_revision_id')::uuid,a->>'object_ref',a->>'sha256',a->>'media_type',a->>'provenance' FROM jsonb_array_elements(p->'assets') WITH ORDINALITY x(a,ordinal);
 INSERT INTO bid_render_snapshot_font_items(render_snapshot_id,ordinal,font_artifact_id,object_ref,content_sha256,media_type,family,script) VALUES('00000000-0000-4000-8000-000000000127',0,'00000000-0000-4000-8000-000000000147','objects/'||repeat('7',64),repeat('7',64),'font/ttf','Noto Sans JP','cjk');
 INSERT INTO bid_render_snapshot_form_definition_items VALUES('00000000-0000-4000-8000-000000000127','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',0,'00000000-0000-4000-8000-000000000053',encode(digest(convert_to('form1','UTF8'),'sha256'),'hex'));
 INSERT INTO bid_render_snapshot_attachment_preparation_items(render_snapshot_id,project_id,workspace_id,ordinal,attachment_preparation_revision_id,canonical_sha256) VALUES('00000000-0000-4000-8000-000000000127','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',0,'00000000-0000-4000-8000-00000000012a',(SELECT preparation_sha256 FROM bid_attachment_preparation_revision_artifacts WHERE id='00000000-0000-4000-8000-00000000012a'));
 SET CONSTRAINTS ALL IMMEDIATE;
END $$;

CREATE FUNCTION pg_temp.clone_render_parent(p_id uuid,p jsonb) RETURNS void LANGUAGE plpgsql AS $$ BEGIN
 INSERT INTO bid_render_document_snapshot_artifacts SELECT p_id,source.project_id,source.workspace_id,source.schema_version,source.workspace_revision_id,source.workspace_sha256,source.scope_revision_id,source.outline_checkpoint_id,source.outline_checkpoint_sha256,source.requirement_projection_id,source.requirement_projection_sha256,source.document_settings_revision_id,source.document_settings_sha256,source.submission_assessment_snapshot_id,source.submission_assessment_snapshot_sha256,source.output_mode,source.format,source.mode_options,source.content_block_schema_version,source.content_block_schema_sha256,source.render_operation_contract_version,source.render_operation_contract_sha256,source.docx_renderer_format,source.docx_renderer_contract_id,source.docx_renderer_contract_sha256,source.pdf_renderer_format,source.pdf_renderer_contract_id,source.pdf_renderer_contract_sha256,source.style_contract_id,source.style_contract_sha256,source.page_size,source.page_width_mm,source.page_height_mm,source.margins_mm,source.numbering_policy,source.toc_policy,p,p->>'snapshot_sha256',now() FROM bid_render_document_snapshot_artifacts source WHERE source.id='00000000-0000-4000-8000-000000000127';
END $$;
DO $$ DECLARE p jsonb; BEGIN
 BEGIN p:=pg_temp.render_payload('00000000-0000-4000-8000-000000000167'); PERFORM pg_temp.clone_render_parent('00000000-0000-4000-8000-000000000167',p); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'missing render projections accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 SET CONSTRAINTS ALL DEFERRED;
 BEGIN
  p:=pg_temp.render_payload('00000000-0000-4000-8000-000000000168'); PERFORM pg_temp.clone_render_parent('00000000-0000-4000-8000-000000000168',p);
  INSERT INTO bid_render_snapshot_node_occurrences SELECT '00000000-0000-4000-8000-000000000168',project_id,workspace_revision_id,node_occurrence_id,node_revision_id,ordinal FROM bid_render_snapshot_node_occurrences WHERE render_snapshot_id='00000000-0000-4000-8000-000000000127';
  INSERT INTO bid_render_snapshot_block_occurrences SELECT '00000000-0000-4000-8000-000000000168',project_id,workspace_revision_id,node_occurrence_id,block_occurrence_id,block_revision_id,block_sha256,ordinal FROM bid_render_snapshot_block_occurrences WHERE render_snapshot_id='00000000-0000-4000-8000-000000000127';
  INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,object_state,media_type,provenance)
   SELECT '00000000-0000-4000-8000-000000000168',CASE ordinal WHEN 0 THEN 1 WHEN 1 THEN 0 ELSE ordinal END,asset_revision_id,object_ref,content_sha256,object_state,media_type,provenance FROM bid_render_snapshot_asset_items WHERE render_snapshot_id='00000000-0000-4000-8000-000000000127';
  INSERT INTO bid_render_snapshot_font_items SELECT '00000000-0000-4000-8000-000000000168',ordinal,font_artifact_id,object_ref,content_sha256,object_state,media_type,family,script FROM bid_render_snapshot_font_items WHERE render_snapshot_id='00000000-0000-4000-8000-000000000127';
  INSERT INTO bid_render_snapshot_form_definition_items SELECT '00000000-0000-4000-8000-000000000168',project_id,workspace_id,ordinal,form_definition_revision_id,canonical_sha256 FROM bid_render_snapshot_form_definition_items WHERE render_snapshot_id='00000000-0000-4000-8000-000000000127';
  INSERT INTO bid_render_snapshot_attachment_preparation_items SELECT '00000000-0000-4000-8000-000000000168',project_id,workspace_id,ordinal,attachment_preparation_revision_id,preparation_status,canonical_sha256 FROM bid_render_snapshot_attachment_preparation_items WHERE render_snapshot_id='00000000-0000-4000-8000-000000000127';
  SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'reordered render projection accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 SET CONSTRAINTS ALL DEFERRED;
END $$;

-- Malformed canonical payload and provenance/projection negatives execute (none disabled).
DO $$ DECLARE p jsonb; bad jsonb; new_id uuid; BEGIN
 FOREACH bad IN ARRAY ARRAY[
  pg_temp.render_payload('00000000-0000-4000-8000-000000000160')||jsonb_build_object('unknown',true),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-000000000169'),'{workspace_id}','null'::jsonb),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-00000000016a'),'{assets,0}',(pg_temp.render_payload('00000000-0000-4000-8000-00000000016a')->'assets'->0)||jsonb_build_object('unknown',true)),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-00000000016b'),'{font_artifact_identities,0,family}','null'::jsonb),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-00000000016c'),'{assets}',jsonb_build_array(pg_temp.render_payload('00000000-0000-4000-8000-00000000016c')->'assets'->0,pg_temp.render_payload('00000000-0000-4000-8000-00000000016c')->'assets'->0)),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-000000000161'),'{render_snapshot_id}',to_jsonb('bad-uuid'::text)),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-000000000162'),'{ordered_nodes,0,depth}','33'::jsonb),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-000000000163'),'{assets,0,provenance}',to_jsonb('unknown'::text)),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-000000000164'),'{page_geometry,margins_mm,top}','81'::jsonb),
  jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-000000000165'),'{ordered_nodes}',jsonb_build_array(pg_temp.render_payload('00000000-0000-4000-8000-000000000165')->'ordered_nodes'->0,pg_temp.render_payload('00000000-0000-4000-8000-000000000165')->'ordered_nodes'->0))
 ] LOOP
  bad:=(bad-'snapshot_sha256')||jsonb_build_object('snapshot_sha256',encode(digest(convert_to((bad-'snapshot_sha256')::text,'UTF8'),'sha256'),'hex'));
  BEGIN new_id:=COALESCE(NULLIF(bad->>'render_snapshot_id','bad-uuid')::uuid,gen_random_uuid()); INSERT INTO bid_render_document_snapshot_artifacts SELECT new_id,source.project_id,source.workspace_id,source.schema_version,source.workspace_revision_id,source.workspace_sha256,source.scope_revision_id,source.outline_checkpoint_id,source.outline_checkpoint_sha256,source.requirement_projection_id,source.requirement_projection_sha256,source.document_settings_revision_id,source.document_settings_sha256,source.submission_assessment_snapshot_id,source.submission_assessment_snapshot_sha256,source.output_mode,source.format,source.mode_options,source.content_block_schema_version,source.content_block_schema_sha256,source.render_operation_contract_version,source.render_operation_contract_sha256,source.docx_renderer_format,source.docx_renderer_contract_id,source.docx_renderer_contract_sha256,source.pdf_renderer_format,source.pdf_renderer_contract_id,source.pdf_renderer_contract_sha256,source.style_contract_id,source.style_contract_sha256,source.page_size,source.page_width_mm,source.page_height_mm,source.margins_mm,source.numbering_policy,source.toc_policy,bad,bad->>'snapshot_sha256',now() FROM bid_render_document_snapshot_artifacts source WHERE source.id='00000000-0000-4000-8000-000000000127'; RAISE EXCEPTION 'malformed render payload accepted'; EXCEPTION WHEN check_violation OR invalid_text_representation THEN NULL; END;
 END LOOP;
 BEGIN p:=jsonb_set(pg_temp.render_payload('00000000-0000-4000-8000-000000000166'),'{snapshot_sha256}',to_jsonb(repeat('f',64))); INSERT INTO bid_render_document_snapshot_artifacts SELECT (p->>'render_snapshot_id')::uuid,source.project_id,source.workspace_id,source.schema_version,source.workspace_revision_id,source.workspace_sha256,source.scope_revision_id,source.outline_checkpoint_id,source.outline_checkpoint_sha256,source.requirement_projection_id,source.requirement_projection_sha256,source.document_settings_revision_id,source.document_settings_sha256,source.submission_assessment_snapshot_id,source.submission_assessment_snapshot_sha256,source.output_mode,source.format,source.mode_options,source.content_block_schema_version,source.content_block_schema_sha256,source.render_operation_contract_version,source.render_operation_contract_sha256,source.docx_renderer_format,source.docx_renderer_contract_id,source.docx_renderer_contract_sha256,source.pdf_renderer_format,source.pdf_renderer_contract_id,source.pdf_renderer_contract_sha256,source.style_contract_id,source.style_contract_sha256,source.page_size,source.page_width_mm,source.page_height_mm,source.margins_mm,source.numbering_policy,source.toc_policy,p,p->>'snapshot_sha256',now() FROM bid_render_document_snapshot_artifacts source WHERE source.id='00000000-0000-4000-8000-000000000127'; RAISE EXCEPTION 'wrong canonical render hash accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance) VALUES('00000000-0000-4000-8000-000000000127',4,'00000000-0000-4000-8000-000000000150','objects/'||repeat('8',64),repeat('8',64),'image/png','manual_workspace'); RAISE EXCEPTION 'render MIME mismatch accepted'; EXCEPTION WHEN foreign_key_violation OR check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance) VALUES('00000000-0000-4000-8000-000000000127',4,gen_random_uuid(),'objects/'||repeat('8',64),repeat('8',64),'image/jpeg','manual_workspace'); RAISE EXCEPTION 'unknown workspace asset revision accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance) VALUES('00000000-0000-4000-8000-000000000127',4,gen_random_uuid(),'objects/'||repeat('6',64),repeat('6',64),'image/png','knowledge_evidence'); RAISE EXCEPTION 'unknown knowledge evidence revision accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance) VALUES('00000000-0000-4000-8000-000000000127',4,gen_random_uuid(),'objects/'||repeat('9',64),repeat('9',64),'image/png','prepared_attachment'); RAISE EXCEPTION 'unknown prepared asset revision accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance) VALUES('00000000-0000-4000-8000-000000000127',4,'00000000-0000-4000-8000-0000000000f9','objects/'||encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),'application/json','quote_snapshot'); RAISE EXCEPTION 'cross-project quote revision accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance) VALUES('00000000-0000-4000-8000-000000000127',4,'00000000-0000-4000-8000-000000000152','objects/'||repeat('8',64),repeat('8',64),'image/jpeg','manual_workspace'); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'extra render projection accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 SET CONSTRAINTS ALL DEFERRED;
 BEGIN INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance) VALUES('00000000-0000-4000-8000-000000000127',4,'00000000-0000-4000-8000-000000000152','objects/'||repeat('8',64),repeat('f',64),'image/jpeg','manual_workspace'); RAISE EXCEPTION 'render projection digest mismatch accepted'; EXCEPTION WHEN foreign_key_violation OR check_violation THEN NULL; END;
 BEGIN UPDATE bid_render_snapshot_font_items SET family=family WHERE render_snapshot_id='00000000-0000-4000-8000-000000000127'; RAISE EXCEPTION 'projection update accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN DELETE FROM bid_render_snapshot_node_occurrences WHERE render_snapshot_id='00000000-0000-4000-8000-000000000127'; RAISE EXCEPTION 'projection delete accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN TRUNCATE bid_render_snapshot_asset_items; RAISE EXCEPTION 'projection truncate accepted'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;

-- Formal manifest uses a literal, independent inventory. Neither insertion nor
-- oracle derives expected tuples from the production helper.
CREATE TEMP TABLE phase0_expected_manifest_dependencies(
 ordinal integer PRIMARY KEY,dependency_kind text NOT NULL,dependency_id uuid NOT NULL,dependency_sha256 kb_sha256 NOT NULL,
 UNIQUE(dependency_kind,dependency_id));
INSERT INTO phase0_expected_manifest_dependencies VALUES
 (0,'assessment','00000000-0000-4000-8000-000000000124',encode(digest(convert_to('assessment1','UTF8'),'sha256'),'hex')),
 (1,'asset','00000000-0000-4000-8000-000000000146',repeat('6',64)),
 (2,'asset','00000000-0000-4000-8000-000000000150',repeat('8',64)),
 (3,'asset','00000000-0000-4000-8000-000000000151',repeat('9',64)),
 (4,'asset','00000000-0000-4000-8000-0000000000f1',encode(digest(convert_to('{}','UTF8'),'sha256'),'hex')),
 (5,'attachment_preparation','00000000-0000-4000-8000-00000000012a',(SELECT preparation_sha256 FROM bid_attachment_preparation_revision_artifacts WHERE id='00000000-0000-4000-8000-00000000012a')),
 (6,'document_set','00000000-0000-4000-8000-000000000042',encode(digest(convert_to('set2','UTF8'),'sha256'),'hex')),
 (7,'document_settings','00000000-0000-4000-8000-000000000122',encode(digest(convert_to('settings1','UTF8'),'sha256'),'hex')),
 (8,'font','00000000-0000-4000-8000-000000000147',repeat('7',64)),
 (9,'form_definition','00000000-0000-4000-8000-000000000053',encode(digest(convert_to('form1','UTF8'),'sha256'),'hex')),
 (10,'outline_checkpoint','00000000-0000-4000-8000-00000000013e',encode(digest(convert_to('checkpoint1','UTF8'),'sha256'),'hex')),
 (11,'quote_snapshot','00000000-0000-4000-8000-0000000000f1',encode(digest(convert_to('{}','UTF8'),'sha256'),'hex')),
 (12,'renderer','00000000-0000-4000-8000-000000000128',encode(digest(convert_to('renderer-docx','UTF8'),'sha256'),'hex')),
 (13,'renderer','00000000-0000-4000-8000-000000000129',encode(digest(convert_to('renderer-pdf','UTF8'),'sha256'),'hex')),
 (14,'render_snapshot','00000000-0000-4000-8000-000000000127',(SELECT content_sha256 FROM bid_render_document_snapshot_artifacts WHERE id='00000000-0000-4000-8000-000000000127')),
 (15,'requirement_projection','00000000-0000-4000-8000-0000000000b2',encode(digest(convert_to('proj2','UTF8'),'sha256'),'hex')),
 (16,'scope','00000000-0000-4000-8000-000000000121',encode(digest(convert_to('scope1','UTF8'),'sha256'),'hex')),
 (17,'style','00000000-0000-4000-8000-000000000125',encode(digest(convert_to('style1','UTF8'),'sha256'),'hex')),
 (18,'workspace','00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'));
DO $$ BEGIN
 INSERT INTO bid_submission_manifest_artifacts(id,project_id,workspace_id,render_snapshot_id,output_mode,format,mode_options,canonical_payload,content_sha256) VALUES('00000000-0000-4000-8000-000000000130','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000127','review_draft','pdf','{"watermark":"DRAFT"}',convert_to('manifest-draft','UTF8'),encode(digest(convert_to('manifest-draft','UTF8'),'sha256'),'hex'));
 INSERT INTO bid_submission_manifest_dependencies(manifest_id,dependency_kind,dependency_id,dependency_sha256,ordinal)
 SELECT '00000000-0000-4000-8000-000000000130',dependency_kind,dependency_id,dependency_sha256,ordinal FROM phase0_expected_manifest_dependencies ORDER BY ordinal;
 IF (SELECT count(*) FROM bid_submission_manifest_dependencies WHERE manifest_id='00000000-0000-4000-8000-000000000130')<>19
    OR EXISTS (SELECT ordinal,dependency_kind,dependency_id,dependency_sha256 FROM phase0_expected_manifest_dependencies
               EXCEPT SELECT ordinal,dependency_kind,dependency_id,dependency_sha256 FROM bid_submission_manifest_dependencies WHERE manifest_id='00000000-0000-4000-8000-000000000130')
    OR EXISTS (SELECT ordinal,dependency_kind,dependency_id,dependency_sha256 FROM bid_submission_manifest_dependencies WHERE manifest_id='00000000-0000-4000-8000-000000000130'
               EXCEPT SELECT ordinal,dependency_kind,dependency_id,dependency_sha256 FROM phase0_expected_manifest_dependencies)
 THEN RAISE EXCEPTION 'literal manifest dependency inventory mismatch'; END IF;
 SET CONSTRAINTS ALL IMMEDIATE;
END $$;
DO $$ BEGIN
 BEGIN INSERT INTO bid_submission_manifest_artifacts(id,project_id,workspace_id,render_snapshot_id,output_mode,format,mode_options,canonical_payload,content_sha256) VALUES('00000000-0000-4000-8000-000000000131','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000127','review_draft','pdf','{"watermark":"DRAFT"}',convert_to('missing-deps','UTF8'),encode(digest(convert_to('missing-deps','UTF8'),'sha256'),'hex')); SET CONSTRAINTS ALL IMMEDIATE; RAISE EXCEPTION 'manifest with zero dependencies accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 SET CONSTRAINTS ALL DEFERRED;
 BEGIN INSERT INTO bid_submission_manifest_dependencies VALUES('00000000-0000-4000-8000-000000000130','asset',gen_random_uuid(),repeat('f',64),999); RAISE EXCEPTION 'unknown manifest dependency accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  INSERT INTO bid_submission_manifest_artifacts(id,project_id,workspace_id,render_snapshot_id,output_mode,format,mode_options,canonical_payload,content_sha256) VALUES('00000000-0000-4000-8000-000000000133','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000127','review_draft','pdf','{"watermark":"DRAFT"}',convert_to('wrong-digest','UTF8'),encode(digest(convert_to('wrong-digest','UTF8'),'sha256'),'hex'));
  INSERT INTO bid_submission_manifest_dependencies(manifest_id,dependency_kind,dependency_id,dependency_sha256,ordinal) VALUES('00000000-0000-4000-8000-000000000133','render_snapshot','00000000-0000-4000-8000-000000000127',repeat('f',64),0);
  RAISE EXCEPTION 'wrong manifest dependency digest accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;

-- Output bytes are registered and held by the exact project/workspace/manifest
-- owner occurrence before the immutable output artifact is published.
INSERT INTO object_owner_references(object_ref,owner_kind,owner_id,occurrence,created_by) VALUES
 ('objects/'||repeat('f',64),'bid_submission_output','00000000-0000-4000-8000-000000000132','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130','system:bid-attachment-preparation'),
 ('objects/'||repeat('f',64),'bid_submission_output','00000000-0000-4000-8000-000000000181','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130','system:bid-attachment-preparation'),
 ('objects/'||repeat('f',64),'bid_submission_output','00000000-0000-4000-8000-000000000182','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130','system:bid-attachment-preparation'),
 ('objects/'||repeat('f',64),'bid_submission_output','00000000-0000-4000-8000-000000000183','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130','system:bid-attachment-preparation'),
 ('objects/'||repeat('d',64),'bid_submission_output','00000000-0000-4000-8000-000000000184','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130','system:bid-attachment-preparation');
INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,media_type,byte_length,owner_id,owner_occurrence)
 VALUES ('00000000-0000-4000-8000-000000000132','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000130','pdf','objects/'||repeat('f',64),repeat('f',64),'application/pdf',1,'00000000-0000-4000-8000-000000000132','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130');
DO $$ BEGIN
 BEGIN INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,media_type,byte_length,owner_id,owner_occurrence) VALUES('00000000-0000-4000-8000-000000000180','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000130','pdf','objects/'||repeat('c',64),repeat('c',64),'application/pdf',1,'00000000-0000-4000-8000-000000000180','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130'); RAISE EXCEPTION 'unknown output object accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,media_type,byte_length,owner_id,owner_occurrence) VALUES('00000000-0000-4000-8000-000000000181','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000130','pdf','objects/'||repeat('f',64),repeat('e',64),'application/pdf',1,'00000000-0000-4000-8000-000000000181','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130'); RAISE EXCEPTION 'wrong output digest accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,media_type,byte_length,owner_id,owner_occurrence) VALUES('00000000-0000-4000-8000-000000000182','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000130','pdf','objects/'||repeat('f',64),repeat('f',64),'image/png',1,'00000000-0000-4000-8000-000000000182','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130'); RAISE EXCEPTION 'wrong output media accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,media_type,byte_length,owner_id,owner_occurrence) VALUES('00000000-0000-4000-8000-000000000183','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000130','pdf','objects/'||repeat('f',64),repeat('f',64),'application/pdf',2,'00000000-0000-4000-8000-000000000183','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130'); RAISE EXCEPTION 'wrong output length accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,media_type,byte_length,object_state,owner_id,owner_occurrence) VALUES('00000000-0000-4000-8000-000000000184','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000130','pdf','objects/'||repeat('d',64),repeat('d',64),'application/pdf',1,'deleting','00000000-0000-4000-8000-000000000184','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130'); RAISE EXCEPTION 'unavailable output object accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,media_type,byte_length,owner_id,owner_occurrence) VALUES('00000000-0000-4000-8000-000000000185','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000130','pdf','objects/'||repeat('f',64),repeat('f',64),'application/pdf',1,'00000000-0000-4000-8000-000000000185','output:00000000-0000-4000-8000-000000000010:00000000-0000-4000-8000-0000000000a0:00000000-0000-4000-8000-000000000130'); RAISE EXCEPTION 'missing output owner accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;

-- Real asynchronous RequirementSet delivery regression: D2 is compiled before late D1.
-- D1 must terminate succeeded/unpublished without moving the RequirementSet, projection, or Workspace heads.
INSERT INTO bid_source_unit_disposition_set_artifacts(
  id,project_id,document_set_id,document_set_sequence,revision,canonical_payload,content_sha256,actor)
VALUES
 ('00000000-0000-4000-8000-000000000064','00000000-0000-4000-8000-000000000010',
  '00000000-0000-4000-8000-000000000042',2,4,convert_to('compile-d1-disposition','UTF8'),
  encode(digest(convert_to('compile-d1-disposition','UTF8'),'sha256'),'hex'),'system:requirement-set-compile-v2'),
 ('00000000-0000-4000-8000-000000000065','00000000-0000-4000-8000-000000000010',
  '00000000-0000-4000-8000-000000000042',2,5,convert_to('compile-d2-disposition','UTF8'),
  encode(digest(convert_to('compile-d2-disposition','UTF8'),'sha256'),'hex'),'system:requirement-set-compile-v2');
INSERT INTO bid_source_unit_disposition_set_items(disposition_set_id,project_id,source_unit_revision_id,disposition)
VALUES
 ('00000000-0000-4000-8000-000000000064','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000052','requirement'),
 ('00000000-0000-4000-8000-000000000065','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000052','requirement');
BEGIN;
INSERT INTO bid_async_request_snapshot_artifacts(
  id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status)
VALUES
 ('00000000-0000-4000-8000-0000000001c1','00000000-0000-4000-8000-000000000010',NULL,
  'requirement_set_compile',1,encode(digest(convert_to('compile-d1-frozen','UTF8'),'sha256'),'hex'),
  convert_to('compile-d1-request','UTF8'),encode(digest(convert_to('compile-d1-request','UTF8'),'sha256'),'hex'),'pending'),
 ('00000000-0000-4000-8000-0000000001c2','00000000-0000-4000-8000-000000000010',NULL,
  'requirement_set_compile',1,encode(digest(convert_to('compile-d2-frozen','UTF8'),'sha256'),'hex'),
  convert_to('compile-d2-request','UTF8'),encode(digest(convert_to('compile-d2-request','UTF8'),'sha256'),'hex'),'pending');
INSERT INTO bid_requirement_set_compile_request_identities(
  request_artifact_id,project_id,request_revision,request_sha256,frozen_input_sha256,
  document_set_revision_id,document_set_sha256,disposition_set_revision_id,disposition_set_sha256)
VALUES
 ('00000000-0000-4000-8000-0000000001c1','00000000-0000-4000-8000-000000000010',1,
  encode(digest(convert_to('compile-d1-request','UTF8'),'sha256'),'hex'),
  encode(digest(convert_to('compile-d1-frozen','UTF8'),'sha256'),'hex'),
  '00000000-0000-4000-8000-000000000042',encode(digest(convert_to('set2','UTF8'),'sha256'),'hex'),
  '00000000-0000-4000-8000-000000000064',encode(digest(convert_to('compile-d1-disposition','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-0000000001c2','00000000-0000-4000-8000-000000000010',1,
  encode(digest(convert_to('compile-d2-request','UTF8'),'sha256'),'hex'),
  encode(digest(convert_to('compile-d2-frozen','UTF8'),'sha256'),'hex'),
  '00000000-0000-4000-8000-000000000042',encode(digest(convert_to('set2','UTF8'),'sha256'),'hex'),
  '00000000-0000-4000-8000-000000000065',encode(digest(convert_to('compile-d2-disposition','UTF8'),'sha256'),'hex'));
COMMIT;
DO $$
DECLARE d2 jsonb; d2_replay jsonb; d1 jsonb; compiled_d2 jsonb; compiled_d1 jsonb;
  set_head uuid; projection_head uuid; projection_sha kb_sha256;
  workspace_before bid_workspace_heads%ROWTYPE; workspace_after bid_workspace_heads%ROWTYPE;
  status_view jsonb; apply_bytes bytea:=convert_to('apply-v3-projection','UTF8');
BEGIN
  compiled_d2:=jsonb_build_object('schema_version',3,
    'source_unit_revision_ids',jsonb_build_array('00000000-0000-4000-8000-000000000052'),
    'requirements',jsonb_build_array(jsonb_build_object(
      'requirement_ref',repeat('d',64),'requirement_kind','technical','requiredness','mandatory',
      'compliance_policy','must_comply','requirement_text','D2 current requirement','channel','narrative_content',
      'applicability',jsonb_build_object('status','required','reason','D2','source_unit_revision_ids',
        jsonb_build_array('00000000-0000-4000-8000-000000000052')),
      'source_unit_revision_ids',jsonb_build_array('00000000-0000-4000-8000-000000000052'),
      'structured_form_revision_ids','[]'::jsonb)),'notices','[]'::jsonb);
  compiled_d1:=jsonb_build_object('schema_version',3,
    'source_unit_revision_ids',jsonb_build_array('00000000-0000-4000-8000-000000000052'),
    'requirements',jsonb_build_array(jsonb_build_object(
      'requirement_ref',repeat('c',64),'requirement_kind','technical','requiredness','mandatory',
      'compliance_policy','must_comply','requirement_text','D1 superseded requirement','channel','narrative_content',
      'applicability',jsonb_build_object('status','required','reason','D1','source_unit_revision_ids',
        jsonb_build_array('00000000-0000-4000-8000-000000000052')),
      'source_unit_revision_ids',jsonb_build_array('00000000-0000-4000-8000-000000000052'),
      'structured_form_revision_ids','[]'::jsonb)),'notices','[]'::jsonb);
  SELECT * INTO STRICT workspace_before FROM bid_workspace_heads
    WHERE scope_id='00000000-0000-4000-8000-0000000000a0';
  d2:=kb_bid_v2_publish_requirement_set_v3('00000000-0000-4000-8000-0000000001c2',1,
    encode(digest(convert_to('compile-d2-frozen','UTF8'),'sha256'),'hex'),compiled_d2,
    'system:requirement-set-compile-v3');
  SELECT artifact_id INTO STRICT set_head FROM bid_requirement_set_current
    WHERE scope_id='00000000-0000-4000-8000-000000000010';
  SELECT artifact_id,artifact_sha256 INTO STRICT projection_head,projection_sha
    FROM bid_workspace_requirement_projection_current
    WHERE scope_id='00000000-0000-4000-8000-0000000000a0';
  IF set_head<>(d2->>'requirement_set_id')::uuid
     OR projection_head<>(d2->>'requirement_projection_id')::uuid
     OR d2->>'status'<>'succeeded' OR d2->>'published_current'<>'true'
     OR d2->>'workspace_apply_required'<>'true' THEN
    RAISE EXCEPTION 'D2 V3 compile did not publish the current requirement projection';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_workspace_heads WHERE scope_id=workspace_before.scope_id
      AND artifact_id=workspace_before.artifact_id AND artifact_sha256=workspace_before.artifact_sha256) THEN
    RAISE EXCEPTION 'V3 worker publication changed WorkspaceHead';
  END IF;
  d2_replay:=kb_bid_v2_publish_requirement_set_v3('00000000-0000-4000-8000-0000000001c2',1,
    encode(digest(convert_to('compile-d2-frozen','UTF8'),'sha256'),'hex'),compiled_d2,
    'system:requirement-set-compile-v3');
  IF d2_replay->>'replayed'<>'true' OR d2_replay->>'requirement_projection_id'<>projection_head::text THEN
    RAISE EXCEPTION 'V3 replay did not return the frozen publication receipt';
  END IF;
  status_view:=kb_bid_v2_get_requirement_set_compile_request(
    '00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000001c2',
    'user:00000000-0000-4000-8000-000000000001');
  IF status_view->>'status'<>'succeeded'
     OR status_view#>>'{result_identity,requirement_projection_id}'<>projection_head::text
     OR status_view->>'document_set_revision_id'<>'00000000-0000-4000-8000-000000000042' THEN
    RAISE EXCEPTION 'typed RequirementSetCompile status view lost frozen identities';
  END IF;
  IF kb_bid_v2_get_requirement_set_compile_request(
      '00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-0000000001c2',
      'user:00000000-0000-4000-8000-000000000001') IS NOT NULL
     OR kb_bid_v2_get_requirement_set_compile_request(
      '00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000001a1',
      'user:00000000-0000-4000-8000-000000000001') IS NOT NULL THEN
    RAISE EXCEPTION 'typed RequirementSetCompile status view crossed project or request-kind binding';
  END IF;
  BEGIN
    PERFORM kb_bid_v2_get_requirement_set_compile_request(
      '00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000001c2',
      'user:00000000-0000-4000-8000-000000000002');
    RAISE EXCEPTION 'non-owner read RequirementSetCompile status';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  PERFORM kb_bid_v2_refresh_requirement_projection(workspace_before.scope_id,projection_head,projection_sha,
    workspace_before.artifact_id,workspace_before.artifact_sha256,
    'user:00000000-0000-4000-8000-000000000001','phase0-v3-explicit-apply',apply_bytes,
    encode(digest(apply_bytes,'sha256'),'hex'));
  SELECT * INTO STRICT workspace_after FROM bid_workspace_heads WHERE scope_id=workspace_before.scope_id;
  IF workspace_after.artifact_id=workspace_before.artifact_id THEN
    RAISE EXCEPTION 'explicit owner projection apply did not advance WorkspaceHead';
  END IF;
  d1:=kb_bid_v2_publish_requirement_set_v3('00000000-0000-4000-8000-0000000001c1',1,
    encode(digest(convert_to('compile-d1-frozen','UTF8'),'sha256'),'hex'),compiled_d1,
    'system:requirement-set-compile-v3');
  IF d1->>'status'<>'succeeded' OR d1->>'published_current'<>'false'
     OR d1->>'workspace_apply_required'<>'false' OR NOT EXISTS (
      SELECT 1 FROM bid_async_request_snapshot_artifacts
      WHERE id='00000000-0000-4000-8000-0000000001c1' AND status='succeeded') THEN
    RAISE EXCEPTION 'late D1 V3 compile did not succeed as an unpublished result';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_requirement_set_current
      WHERE scope_id='00000000-0000-4000-8000-000000000010' AND artifact_id=set_head)
     OR NOT EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_current
      WHERE scope_id='00000000-0000-4000-8000-0000000000a0' AND artifact_id=projection_head)
     OR NOT EXISTS (SELECT 1 FROM bid_workspace_heads
      WHERE scope_id=workspace_after.scope_id AND artifact_id=workspace_after.artifact_id
        AND artifact_sha256=workspace_after.artifact_sha256) THEN
    RAISE EXCEPTION 'late D1 V3 compile rolled back a current pointer';
  END IF;
END $$;

-- Explicit quote apply advances the head while historical WorkspaceRevision keeps its quote identity.
BEGIN;
INSERT INTO bid_quote_snapshot_artifacts(id,project_id,revision,currency,canonical_payload,content_sha256,actor)
VALUES('00000000-0000-4000-8000-0000000000f2','00000000-0000-4000-8000-000000000010',2,'CNY',
  convert_to('{"revision":2}','UTF8'),encode(digest(convert_to('{"revision":2}','UTF8'),'sha256'),'hex'),
  'user:00000000-0000-4000-8000-000000000001');
INSERT INTO bid_quote_snapshot_current(scope_id,artifact_id,artifact_sha256,generation,created_at)
VALUES('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000f2',
  encode(digest(convert_to('{"revision":2}','UTF8'),'sha256'),'hex'),2,clock_timestamp());
SELECT kb_bid_v2_advance_workspace_quote('00000000-0000-4000-8000-000000000010',
  '00000000-0000-4000-8000-0000000000f2',encode(digest(convert_to('{"revision":2}','UTF8'),'sha256'),'hex'),
  (SELECT artifact_id FROM bid_workspace_heads WHERE scope_id='00000000-0000-4000-8000-0000000000a0'),
  (SELECT artifact_sha256 FROM bid_workspace_heads WHERE scope_id='00000000-0000-4000-8000-0000000000a0'),
  'user:00000000-0000-4000-8000-000000000001');
DO $$
DECLARE old_value jsonb; new_value jsonb; head bid_workspace_heads%ROWTYPE;
  revision_count bigint; evidence_count bigint;
BEGIN
  old_value:=kb_bid_v2_load_workspace_revision('00000000-0000-4000-8000-0000000000a0',
    '00000000-0000-4000-8000-000000000135',encode(digest(convert_to('{"fixture":2}','UTF8'),'sha256'),'hex'));
  IF old_value#>>'{quote_snapshot,artifact_id}' IS DISTINCT FROM '00000000-0000-4000-8000-0000000000f1' THEN
    RAISE EXCEPTION 'historical workspace quote identity changed';
  END IF;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id='00000000-0000-4000-8000-0000000000a0';
  new_value:=kb_bid_v2_load_workspace_revision(head.scope_id,head.artifact_id,head.artifact_sha256);
  IF new_value#>>'{quote_snapshot,artifact_id}' IS DISTINCT FROM '00000000-0000-4000-8000-0000000000f2' THEN
    RAISE EXCEPTION 'new workspace revision did not freeze the new quote';
  END IF;
  SELECT count(*) INTO revision_count FROM bid_workspace_revision_artifacts WHERE workspace_id=head.scope_id;
  SELECT count(*) INTO evidence_count FROM bid_submission_fulfillment_evidence_revision_artifacts WHERE workspace_id=head.scope_id;
  BEGIN
    PERFORM kb_bid_v2_advance_workspace_quote('00000000-0000-4000-8000-000000000010',
      '00000000-0000-4000-8000-0000000000f1',encode(digest(convert_to('{}','UTF8'),'sha256'),'hex'),
      head.artifact_id,head.artifact_sha256,'user:00000000-0000-4000-8000-000000000001');
    RAISE EXCEPTION 'non-current Q1 applied after Q2 publication';
  EXCEPTION WHEN serialization_failure THEN NULL; END;
  IF NOT EXISTS (SELECT 1 FROM bid_workspace_heads WHERE scope_id=head.scope_id
      AND artifact_id=head.artifact_id AND artifact_sha256=head.artifact_sha256)
     OR (SELECT count(*) FROM bid_workspace_revision_artifacts WHERE workspace_id=head.scope_id)<>revision_count
     OR (SELECT count(*) FROM bid_submission_fulfillment_evidence_revision_artifacts WHERE workspace_id=head.scope_id)<>evidence_count THEN
    RAISE EXCEPTION 'rejected stale quote apply left partial Workspace artifacts';
  END IF;
END $$;
ROLLBACK;

-- A compile request must acquire a fenced terminal state after its final worker retry.
BEGIN;
SELECT kb_bid_v2_mark_requirement_set_compile_failed('00000000-0000-4000-8000-0000000001a2',1,
  repeat('2',64),'WORKER_FINAL_RETRY');
DO $$ BEGIN
  IF NOT EXISTS (SELECT 1 FROM bid_async_request_snapshot_artifacts
      WHERE id='00000000-0000-4000-8000-0000000001a2' AND status='failed'
        AND finished_at IS NOT NULL AND error_code='REQUIREMENT_COMPILE_FAILED') THEN
    RAISE EXCEPTION 'final compile failure did not become terminal';
  END IF;
END $$;
ROLLBACK;

-- Repeated terminal delivery is an ACK/no-op and cannot corrupt a ready document.
BEGIN;
SELECT kb_bid_v2_mark_tender_document_failed('00000000-0000-4000-8000-0000000001a1','AGENT_OUTPUT_INVALID');
SELECT kb_bid_v2_mark_tender_document_failed('00000000-0000-4000-8000-0000000001a1','AGENT_OUTPUT_INVALID');
DO $$ BEGIN
 IF NOT EXISTS (SELECT 1 FROM bid_async_request_snapshot_artifacts
     WHERE id='00000000-0000-4000-8000-0000000001a1' AND status='succeeded')
    OR NOT EXISTS (SELECT 1 FROM bid_documents
     WHERE id='00000000-0000-4000-8000-000000000011' AND parse_status='ready') THEN
   RAISE EXCEPTION 'terminal tender failure redelivery corrupted succeeded request/document';
 END IF;
END $$;
ROLLBACK;

SELECT 'V2 Phase 0 live canonical evidence/render/manifest and provenance: PASS';
