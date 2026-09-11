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
 ('00000000-0000-4000-8000-0000000001c4','00000000-0000-4000-8000-0000000001b4','00000000-0000-4000-8000-0000000001b2','text','unit1',0,5),
 ('00000000-0000-4000-8000-0000000001d5','00000000-0000-4000-8000-0000000001b4','00000000-0000-4000-8000-0000000001b2','text','req2 evidence A',0,15),
 ('00000000-0000-4000-8000-0000000001d6','00000000-0000-4000-8000-0000000001b4','00000000-0000-4000-8000-0000000001b2','text','req2 evidence B',0,15);

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
 ('00000000-0000-4000-8000-000000000052','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000051',1,'00000000-0000-4000-8000-000000000011','00000000-0000-4000-8000-000000000031','section',0,'{"schema_version":2,"project_id":"00000000-0000-4000-8000-000000000010","document_id":"00000000-0000-4000-8000-000000000011","converted_source_revision_id":"00000000-0000-4000-8000-000000000031","parser_unit_key":"phase0-1","parser_ordinal":0,"source_purpose":"tender_requirements_and_structure_only","locator":{"locator_kind":"document","section_ordinal":0,"table_ordinal":null,"row_ordinal":null,"form_ordinal":null,"heading_path":""}}',repeat('1',64),convert_to('unit1','UTF8'),encode(digest(convert_to('unit1','UTF8'),'sha256'),'hex'),convert_to('unit1','UTF8'),encode(digest(convert_to('unit1','UTF8'),'sha256'),'hex')),
 ('00000000-0000-4000-8000-00000000005a','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000059',1,'00000000-0000-4000-8000-000000000013','00000000-0000-4000-8000-000000000039','form_region',0,'{"schema_version":2,"project_id":"00000000-0000-4000-8000-000000000019","document_id":"00000000-0000-4000-8000-000000000013","converted_source_revision_id":"00000000-0000-4000-8000-000000000039","parser_unit_key":"phase0-2","parser_ordinal":0,"source_purpose":"tender_requirements_and_structure_only","locator":{"locator_kind":"document","section_ordinal":0,"table_ordinal":null,"row_ordinal":null,"form_ordinal":0,"heading_path":""}}',repeat('2',64),convert_to('unit2','UTF8'),encode(digest(convert_to('unit2','UTF8'),'sha256'),'hex'),convert_to('unit2','UTF8'),encode(digest(convert_to('unit2','UTF8'),'sha256'),'hex'));
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_source_unit_revision_artifacts(id,project_id,lineage_id,revision,document_id,source_revision_id,unit_kind,ordinal,source_locator,source_span_sha256,text_utf8,text_sha256,canonical_payload,content_sha256)
    VALUES('00000000-0000-4000-8000-00000000005b','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000051',2,'00000000-0000-4000-8000-000000000012','00000000-0000-4000-8000-000000000032','section',0,'{"schema_version":2,"project_id":"00000000-0000-4000-8000-000000000010","document_id":"00000000-0000-4000-8000-000000000012","converted_source_revision_id":"00000000-0000-4000-8000-000000000032","parser_unit_key":"phase0-bad","parser_ordinal":0,"source_purpose":"tender_requirements_and_structure_only","locator":{"locator_kind":"document","section_ordinal":0,"table_ordinal":null,"row_ordinal":null,"form_ordinal":null,"heading_path":""}}',repeat('3',64),convert_to('bad','UTF8'),encode(digest(convert_to('bad','UTF8'),'sha256'),'hex'),convert_to('bad','UTF8'),encode(digest(convert_to('bad','UTF8'),'sha256'),'hex'));
    RAISE EXCEPTION 'source composite identity unexpectedly accepted';
  EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;
DO $$ BEGIN
  BEGIN
    INSERT INTO bid_source_unit_revision_artifacts(id,project_id,lineage_id,revision,document_id,source_revision_id,unit_kind,ordinal,source_locator,source_span_sha256,text_utf8,text_sha256,canonical_payload,content_sha256)
    VALUES('00000000-0000-4000-8000-00000000005c','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000051',2,'00000000-0000-4000-8000-000000000011','00000000-0000-4000-8000-000000000031','section',1,'{"locator_kind":"document","section_ordinal":1}',repeat('4',64),convert_to('direct-locator','UTF8'),encode(digest(convert_to('direct-locator','UTF8'),'sha256'),'hex'),convert_to('direct-locator','UTF8'),encode(digest(convert_to('direct-locator','UTF8'),'sha256'),'hex'));
    RAISE EXCEPTION 'clean-slate direct source locator unexpectedly accepted';
  EXCEPTION WHEN check_violation THEN NULL; END;
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
INSERT INTO bid_workspace_requirement_projection_items(projection_id,project_id,requirement_revision_id,effective_applicability,ordinal) VALUES
 ('00000000-0000-4000-8000-0000000000b1','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000071','{}',0),
 ('00000000-0000-4000-8000-0000000000b2','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000072','{}',0),
 ('00000000-0000-4000-8000-0000000000b9','00000000-0000-4000-8000-000000000019','00000000-0000-4000-8000-000000000079','{}',0);
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
 ('00000000-0000-4000-8000-000000000178','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','image_ocr','unit1'),
 ('00000000-0000-4000-8000-0000000001d7','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','text','req2 evidence C'),
 ('00000000-0000-4000-8000-0000000001d8','00000000-0000-4000-8000-000000000172','00000000-0000-4000-8000-000000000173','text','req2 evidence D');

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
CREATE TABLE phase0_attestation(id uuid PRIMARY KEY,content_sha256 kb_sha256 NOT NULL);
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
    'route_id','00000000-0000-4000-8000-000000000072','requirement_artifact_id','00000000-0000-4000-8000-000000000072',
    'product_version_artifact_id','00000000-0000-4000-8000-0000000001b4','document_id','00000000-0000-4000-8000-0000000001b2',
    'source_chunk_id','00000000-0000-4000-8000-0000000001b3','frozen_document_display_name','verified-source.txt',
    'chunk_utf8','verified fact','chunk_sha256',encode(digest(convert_to('verified fact','UTF8'),'sha256'),'hex'),
    'chunk_byte_length',13,'source_type','text','media',NULL,'retrieval_rank',1,'retrieval_raw_score','1.000000',
    'pre_rerank_rrf_rank',NULL,'quote_start_offset',0,'quote_end_offset',13,'offset_unit','utf8_byte',
    'retrieval_contract_version','knowledge-evidence-v2'),
    jsonb_build_object('id','00000000-0000-4000-8000-0000000001bd','route_id','00000000-0000-4000-8000-000000000072',
      'requirement_artifact_id','00000000-0000-4000-8000-000000000072','product_version_artifact_id','00000000-0000-4000-8000-000000000172',
      'document_id','00000000-0000-4000-8000-000000000173','source_chunk_id','00000000-0000-4000-8000-000000000174',
      'frozen_document_display_name','proof.png','chunk_utf8','proof image',
      'chunk_sha256',encode(digest(convert_to('proof image','UTF8'),'sha256'),'hex'),'chunk_byte_length',11,
      'source_type','image_ocr','media',jsonb_build_object('image_artifact_revision_id','00000000-0000-4000-8000-000000000145',
        'object_ref','objects/'||repeat('6',64),'sha256',repeat('6',64),'media_type','image/png','width',1,'height',1,
        'page_ordinal',0,'bounding_region',jsonb_build_object('left',0,'top',0,'right',1,'bottom',1),
        'frozen_document_display_name','proof.png'),'retrieval_rank',2,'retrieval_raw_score','0.500000',
      'pre_rerank_rrf_rank',1,'quote_start_offset',0,'quote_end_offset',11,'offset_unit','utf8_byte',
      'retrieval_contract_version','knowledge-evidence-v2')),
  'retrieval_requirements',jsonb_build_array(jsonb_build_object('route_id','00000000-0000-4000-8000-000000000072',
    'requirement_artifact_id','00000000-0000-4000-8000-000000000072',
    'requirement_identity_sha256',encode(digest(convert_to('verified fact','UTF8'),'sha256'),'hex'),
    'requirement_text','verified fact','exact_prefix_hit_count',1)),
  'version_selections',jsonb_build_object('company','[]'::jsonb,'product_line','[]'::jsonb),
  'workspace_kinds',jsonb_build_array('company','product_line'),
  'retrieval_policy',jsonb_build_object('contract_version','knowledge-evidence-v2','policy_sha256',policy_sha,'max_hits',2,'max_chunk_bytes',1024,'max_total_bytes',1024));
 attestation:=kb_knowledge_attest_matching_scope_v2(scope);
 INSERT INTO phase0_attestation VALUES((attestation->>'id')::uuid,attestation->>'content_sha256');
END $$;
INSERT INTO bid_evidence_match_reports(id,project_id,workspace_id,node_lineage_id,requirement_revision_id,retrieval_contract_version,knowledge_scope_attestation_id,knowledge_scope_attestation_sha256,canonical_payload,content_sha256)
SELECT '00000000-0000-4000-8000-000000000141','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-0000000000c1','00000000-0000-4000-8000-000000000072','knowledge-evidence-v2',id,content_sha256,convert_to('{}','UTF8'),encode(digest(convert_to('{}','UTF8'),'sha256'),'hex')
FROM phase0_attestation;
DO $$ BEGIN
 BEGIN INSERT INTO bid_evidence_match_reports(id,project_id,workspace_id,requirement_revision_id,retrieval_contract_version,knowledge_scope_attestation_id,knowledge_scope_attestation_sha256,canonical_payload,content_sha256)
 VALUES(gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000072','v3',gen_random_uuid(),repeat('a',64),convert_to('bad-attestation','UTF8'),encode(digest(convert_to('bad-attestation','UTF8'),'sha256'),'hex')); RAISE EXCEPTION 'unknown attestation accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
 BEGIN INSERT INTO bid_evidence_match_reports(id,project_id,workspace_id,requirement_revision_id,retrieval_contract_version,knowledge_scope_attestation_id,knowledge_scope_attestation_sha256,canonical_payload,content_sha256)
 SELECT gen_random_uuid(),'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000072','v3',id,repeat('f',64),convert_to('bad-attestation-sha','UTF8'),encode(digest(convert_to('bad-attestation-sha','UTF8'),'sha256'),'hex') FROM phase0_attestation; RAISE EXCEPTION 'wrong attestation sha accepted'; EXCEPTION WHEN foreign_key_violation THEN NULL; END;
END $$;

-- Schema-valid EvidenceBundleV1 plus exact item/media projections in one transaction.
DO $$ DECLARE item jsonb; base jsonb; payload jsonb; sha text; created constant timestamptz:='2026-01-01T00:00:00Z'; BEGIN
 item:=jsonb_build_object('kind','image','evidence_item_id','00000000-0000-4000-8000-000000000144',
 'document_id','00000000-0000-4000-8000-000000000173','source_chunk_id','00000000-0000-4000-8000-000000000174',
 'product_version_id','00000000-0000-4000-8000-000000000172','workspace_kind','product_line',
 'quote_utf8','proof image','quote_sha256',encode(digest(convert_to('proof image','UTF8'),'sha256'),'hex'),
 'quote_start_offset',0,'quote_end_offset',11,'retrieval_rank',2,'retrieval_contract_version','knowledge-evidence-v2',
 'image_artifact_revision_id','00000000-0000-4000-8000-000000000145','object_ref','objects/'||repeat('6',64),'sha256',repeat('6',64),'media_type','image/png','width',1,'height',1,'page_ordinal',0,'bounding_region',jsonb_build_object('left',0,'top',0,'right',1,'bottom',1),'frozen_document_display_name','proof.png');
 base:=jsonb_build_object('schema_version',1,'evidence_bundle_id','00000000-0000-4000-8000-000000000143','project_id','00000000-0000-4000-8000-000000000010','workspace_id','00000000-0000-4000-8000-0000000000a0','workspace_scope','project_wide','requirement_revision_id','00000000-0000-4000-8000-000000000072','matching_report_id','00000000-0000-4000-8000-000000000141','knowledge_scope_attestation_id',(SELECT id FROM phase0_attestation),'knowledge_scope_attestation_sha256',(SELECT content_sha256 FROM phase0_attestation),'items',jsonb_build_array(item),'created_at','2026-01-01T00:00:00Z');
 sha:=encode(digest(convert_to(base::text,'UTF8'),'sha256'),'hex'); payload:=base||jsonb_build_object('bundle_sha256',sha);
 INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at) VALUES('00000000-0000-4000-8000-000000000143','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000072','00000000-0000-4000-8000-000000000141',payload,sha,created);
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
  'workspace_scope','project_wide','requirement_revision_id','00000000-0000-4000-8000-000000000072',
  'matching_report_id','00000000-0000-4000-8000-000000000141',
  'knowledge_scope_attestation_id',(SELECT id FROM phase0_attestation),
  'knowledge_scope_attestation_sha256',(SELECT content_sha256 FROM phase0_attestation),
  'items',jsonb_build_array(item),'created_at','2026-01-01T00:00:00Z');
 bundle_sha:=encode(digest(convert_to(base::text,'UTF8'),'sha256'),'hex');
 payload:=base||jsonb_build_object('bundle_sha256',bundle_sha);
 INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at)
 VALUES('00000000-0000-4000-8000-0000000001b0','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000072','00000000-0000-4000-8000-000000000141',payload,bundle_sha,created);
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
  VALUES('00000000-0000-4000-8000-0000000001b5','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000072','00000000-0000-4000-8000-000000000141',bad_payload,bad_bundle_sha,created);
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
  VALUES('00000000-0000-4000-8000-0000000001b9','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000072','00000000-0000-4000-8000-000000000141',tender_payload,tender_bundle_sha,created);
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
CREATE FUNCTION phase0_assert_evidence_media_rejected(
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
  'workspace_id','00000000-0000-4000-8000-0000000000a0','workspace_scope','project_wide','requirement_revision_id','00000000-0000-4000-8000-000000000072',
  'matching_report_id','00000000-0000-4000-8000-000000000141','knowledge_scope_attestation_id',(SELECT id FROM phase0_attestation),
  'knowledge_scope_attestation_sha256',(SELECT content_sha256 FROM phase0_attestation),'items',jsonb_build_array(item),'created_at','2026-01-01T00:00:00Z');
 sha:=encode(digest(convert_to(base::text,'UTF8'),'sha256'),'hex'); payload:=base||jsonb_build_object('bundle_sha256',sha);
 BEGIN
  INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,canonical_payload,content_sha256,created_at)
   VALUES(bundle_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000072','00000000-0000-4000-8000-000000000141',payload,sha,created);
  INSERT INTO bid_evidence_bundle_items(id,project_id,workspace_id,evidence_bundle_id,ordinal,item_kind,source_media_revision_id,item_payload,content_sha256)
   VALUES(item_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',bundle_id,0,'image','00000000-0000-4000-8000-000000000145',item,encode(digest(convert_to(item::text,'UTF8'),'sha256'),'hex'));
  INSERT INTO bid_evidence_asset_artifacts(id,project_id,workspace_id,evidence_bundle_id,evidence_item_id,image_artifact_revision_id,object_ref,content_sha256,media_type,width,height,page_ordinal,bounding_region)
   VALUES(asset_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0',bundle_id,item_id,'00000000-0000-4000-8000-000000000145','objects/'||repeat('6',64),repeat('6',64),bad_media,bad_width,bad_height,bad_page,bad_bounds);
  RAISE EXCEPTION '% mismatch unexpectedly accepted',label;
 EXCEPTION WHEN foreign_key_violation OR check_violation THEN NULL; END;
END $$;
SELECT phase0_assert_evidence_media_rejected('MIME','image/jpeg',1,1,0,'{"left":0,"top":0,"right":1,"bottom":1}');
SELECT phase0_assert_evidence_media_rejected('width','image/png',2,1,0,'{"left":0,"top":0,"right":1,"bottom":1}');
SELECT phase0_assert_evidence_media_rejected('height','image/png',1,2,0,'{"left":0,"top":0,"right":1,"bottom":1}');
SELECT phase0_assert_evidence_media_rejected('page','image/png',1,1,1,'{"left":0,"top":0,"right":1,"bottom":1}');
SELECT phase0_assert_evidence_media_rejected('bounds','image/png',1,1,0,'{"left":0.1,"top":0,"right":1,"bottom":1}');
DROP FUNCTION phase0_assert_evidence_media_rejected(text,text,integer,integer,integer,jsonb);

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
     VALUES(new_id,'00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-0000000000a0','00000000-0000-4000-8000-000000000072','00000000-0000-4000-8000-000000000141',bad,sha,'2026-01-01T00:00:00Z');
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
DROP TABLE phase0_attestation;
COMMIT;

