const SQL: &str = include_str!("../../../migrations/bidding_v2_baseline.sql");
const KNOWLEDGE_SQL: &str = include_str!("../../../migrations/knowledge_base_baseline.sql");
const ACTIVE_QUEUE_REGISTRY: &str = include_str!("../../../deploy/queue-registry.toml");
const PHASE1_LIVE: &str = include_str!("../../../scripts/bidding_v2_phase1_live.sql");
const PHASE3_LIVE: &str = include_str!("../../../scripts/bidding_v2_phase3_live.sql");
const API_ROUTER: &str = include_str!("../../api/src/routes.rs");
const BID_API_ROUTER: &str = include_str!("../../api/src/bid_v2_routes.rs");
const WORKER: &str = include_str!("../../worker/src/consume.rs");
const KNOWLEDGE_CLONE: &str = include_str!("../../knowledge/src/clone/mod.rs");
const KNOWLEDGE_SEARCH: &str = include_str!("../../knowledge/src/search/mod.rs");
const FRESH_SCHEMA_ACCEPTANCE: &str = include_str!("../../../scripts/fresh_schema_acceptance.sh");
const BIDDING_TEST_SUPPORT: &str = include_str!("support/mod.rs");

use knowledge::{LaunchMode, QueueRegistry};
use platform::{
    BID_AUTHORING_V2_PAYLOAD_SCHEMA, BID_AUTHORING_V2_PAYLOAD_VERSION, BID_AUTHORING_V2_QUEUE,
};

fn create_table_names() -> Vec<&'static str> {
    SQL.lines()
        .filter_map(|line| line.trim().strip_prefix("CREATE TABLE "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect()
}

#[test]
fn destructive_postgres_tests_require_an_isolated_non_live_database() {
    for (name, source) in [
        ("worker", WORKER),
        ("knowledge clone", KNOWLEDGE_CLONE),
        ("knowledge search", KNOWLEDGE_SEARCH),
        ("fresh acceptance", FRESH_SCHEMA_ACCEPTANCE),
    ] {
        assert!(source.contains("DROP SCHEMA public CASCADE"), "{name}");
        assert!(
            source.contains("KNOWLEDGEBRAIN_TEST_DATABASE_URL"),
            "{name} can inherit the live database"
        );
        assert!(
            source.contains(":15432/"),
            "{name} does not explicitly reject the live PostgreSQL port"
        );
    }
    assert!(BIDDING_TEST_SUPPORT.contains("KNOWLEDGEBRAIN_TEST_DATABASE_URL"));
    assert!(!BIDDING_TEST_SUPPORT.contains("platform::database_url()"));
    assert!(BIDDING_TEST_SUPPORT.contains(":15432/"));
}

#[test]
fn v2_baseline_has_the_complete_authoring_foundation() {
    let tables = create_table_names();
    for table in [
        "bid_projects",
        "bid_documents",
        "bid_document_role_revision_artifacts",
        "bid_document_relation_revision_artifacts",
        "bid_document_set_artifacts",
        "bid_source_unit_revision_artifacts",
        "bid_source_unit_disposition_set_artifacts",
        "bid_requirement_set_artifacts",
        "bid_requirement_revision_artifacts",
        "bid_workspace_requirement_projection_artifacts",
        "bid_workspace_requirement_projection_current",
        "bid_requirement_supersession_current",
        "bid_submission_workspaces",
        "bid_document_settings_revision_artifacts",
        "bid_outline_node_revision_artifacts",
        "bid_content_block_revision_artifacts",
        "bid_outline_fulfillment_binding_revision_artifacts",
        "bid_workspace_revision_artifacts",
        "bid_workspace_node_occurrences",
        "bid_workspace_block_occurrences",
        "bid_workspace_binding_occurrences",
        "bid_workspace_heads",
        "bid_async_request_snapshot_artifacts",
        "bid_authoring_contract_artifacts",
        "bid_tender_document_process_request_identities",
        "bid_requirement_set_compile_request_identities",
        "bid_outline_generation_request_identities",
        "bid_content_generation_request_identities",
        "bid_submission_export_request_identities",
        "bid_content_generation_request_evidence_bundles",
        "bid_candidate_artifacts",
        "bid_evidence_match_reports",
        "bid_evidence_bundle_artifacts",
        "bid_evidence_bundle_items",
        "bid_evidence_asset_artifacts",
        "bid_workspace_asset_artifacts",
        "bid_workspace_asset_retirement_artifacts",
        "bid_outline_assessment_snapshot_artifacts",
        "bid_submission_assessment_snapshot_artifacts",
        "bid_submission_assessment_snapshot_evidence_items",
        "bid_quote_snapshot_artifacts",
        "bid_renderer_contract_artifacts",
        "bid_render_font_artifacts",
        "bid_attachment_preparation_revision_artifacts",
        "bid_render_document_snapshot_artifacts",
        "bid_render_snapshot_node_occurrences",
        "bid_render_snapshot_block_occurrences",
        "bid_render_snapshot_asset_items",
        "bid_render_snapshot_font_items",
        "bid_render_snapshot_form_definition_items",
        "bid_render_snapshot_attachment_preparation_items",
        "bid_submission_manifest_artifacts",
        "bid_submission_output_artifacts",
        "bid_submission_assessment_report_artifacts",
    ] {
        assert!(tables.contains(&table), "missing V2 table {table}");
    }
    assert!(SQL.contains("scope_kind text NOT NULL DEFAULT 'project_wide'"));
    assert!(SQL.contains("CHECK (scope_kind='project_wide')"));
    assert!(SQL.contains("kb_bid_v2_advance_workspace_head"));
    assert!(SQL.contains("kb_bid_v2_publish_requirement_set"));
    for function in [
        "kb_bid_v2_advance_document_set",
        "kb_bid_v2_advance_disposition_set",
        "kb_bid_v2_advance_requirement_supersession",
        "kb_bid_v2_advance_requirement_projection",
    ] {
        assert!(SQL.contains(function));
    }
    assert!(SQL.contains("document_set_sequence"));
    assert!(SQL.contains("disposition_set_sequence"));
    assert!(SQL.contains("disposition IN ('requirement','non_requirement','unresolved')"));
    assert!(!SQL.contains("mandatory boolean"));
    assert!(SQL.contains("requiredness IN ('mandatory','optional','informational')"));
    assert!(SQL.contains(
        "compliance_policy IN ('must_comply','explicit_response','deviation_allowed','scored')"
    ));
    assert!(SQL.contains("lifecycle IN ('current','superseded','withdrawn','unresolved')"));
    assert!(SQL.contains("status IN ('ready','has_warnings','has_critical_warnings')"));
    assert!(SQL.contains("mode_options jsonb NOT NULL"));
    for function in [
        "kb_bid_v2_get_requirement_projection",
        "kb_bid_v2_refresh_requirement_projection",
        "kb_bid_v2_retire_workspace_asset",
        "kb_bid_v2_create_node_evidence_pick_set",
        "kb_bid_v2_get_node_evidence",
        "kb_bid_v2_prepare_workspace_attachment",
        "kb_bid_v2_load_user_pick_evidence",
        "kb_bid_v2_publish_quote_snapshot",
        "kb_bid_v2_prepare_submission_export",
        "kb_bid_v2_load_submission_manifest_render_input",
    ] {
        assert!(
            SQL.contains(function),
            "missing V2 resource function {function}"
        );
    }
    for route in [
        "/fulfillment-bindings",
        "/nodes/{node_lineage_id}/evidence",
        "/nodes/{node_lineage_id}/evidence-pick-set",
        "/assets/{asset_revision_id}",
        "/document-settings",
        "/requirement-projection",
        "/exports/{export_id}/assessment-report",
        "/quote-snapshots",
    ] {
        assert!(
            BID_API_ROUTER.contains(route),
            "missing V2 API route {route}"
        );
    }
}

#[test]
fn reviewed_publication_target_and_render_constraints_are_frozen() {
    let requirement_publish = &SQL[SQL
        .find("CREATE FUNCTION kb_bid_v2_publish_requirement_set")
        .expect("requirement publication")
        ..SQL
            .find("CREATE VIEW bidding_v2_projects")
            .expect("view fence")];
    assert!(requirement_publish.contains("p_artifact_id uuid,p_artifact_sha256 kb_sha256"));
    assert!(!requirement_publish.contains("p_expected_artifact_id"));
    assert!(!requirement_publish.contains("candidate.revision<>current_value.generation+1"));
    assert!(requirement_publish.contains("RETURN 'obsolete'"));
    assert!(requirement_publish.contains("RETURN 'replayed'"));
    assert!(requirement_publish.contains("generation=current_value.generation+1"));

    assert!(SQL.contains("kb_bid_v2_validate_fulfillment_binding_target"));
    for target_table in [
        "bid_outline_node_lineages",
        "bid_content_block_revision_artifacts",
        "bid_tender_structured_form_definition_artifacts",
        "bid_quote_snapshot_artifacts",
    ] {
        assert!(
            SQL.contains(target_table),
            "missing typed binding target {target_table}"
        );
    }
    assert!(SQL.contains("mode_options ?& ARRAY['watermark','include_assessment_notices','include_knowledge_sources']"));
    assert!(SQL.contains("mode_options - ARRAY['watermark','include_assessment_notices','include_knowledge_sources']::text[] = '{}'::jsonb"));
    assert!(SQL.contains(
        "jsonb_typeof(mode_options->'include_assessment_notices') IS NOT DISTINCT FROM 'boolean'"
    ));
    assert!(SQL.contains("mode_options @> '{\"watermark\":null,\"include_assessment_notices\":false,\"include_knowledge_sources\":false}'::jsonb"));
    assert!(SQL.contains("docx_renderer_contract_sha256 kb_sha256 NOT NULL"));
    assert!(SQL.contains("pdf_renderer_contract_sha256 kb_sha256 NOT NULL"));
    assert!(SQL.contains("canonical_payload-'preparation_sha256'"));
    assert!(SQL.contains("kb_bid_v2_verify_attachment_preparation_projection"));
    assert!(SQL.contains("REFERENCES bid_workspace_asset_artifacts(project_id,workspace_id,id)"));
    assert!(SQL.contains("UNIQUE(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id)"));
    assert!(SQL.contains(
        "UNIQUE(project_id,workspace_id,id,requirement_projection_id,requirement_projection_sha256)"
    ));
    assert!(SQL.contains("FOREIGN KEY(project_id,workspace_id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256)"));
    assert!(SQL.contains("REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,requirement_projection_id,requirement_projection_sha256)"));
    assert!(SQL.contains("FOREIGN KEY(project_id,workspace_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id)"));
    assert!(SQL.contains("submission_assessment_snapshot_sha256 kb_sha256 NOT NULL"));
    assert!(SQL.contains("UNIQUE(project_id,workspace_id,id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id,content_sha256)"));
    assert!(SQL.contains("FOREIGN KEY(project_id,workspace_id,submission_assessment_snapshot_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id,submission_assessment_snapshot_sha256)"));
    assert!(SQL.contains(
        "preparation_status text NOT NULL DEFAULT 'ready' CHECK (preparation_status='ready')"
    ));
    assert!(SQL.contains("FOREIGN KEY(project_id,workspace_id,attachment_preparation_revision_id,preparation_status,canonical_sha256)"));
    assert!(SQL.contains(
        "FOREIGN KEY(project_id,workspace_id,render_snapshot_id,output_mode,format,mode_options)"
    ));
    assert!(SQL.contains("FOREIGN KEY(project_id,workspace_id,manifest_id,format)"));
    assert!(SQL.contains("FOREIGN KEY(project_id,parent_revision_id,parent_sha256)"));
    assert!(SQL.contains("FOREIGN KEY(project_id,artifact_id,artifact_sha256) REFERENCES bid_workspace_revision_artifacts"));
    assert!(SQL.contains(
        "FOREIGN KEY(project_id,workspace_id,base_workspace_revision_id,base_workspace_sha256)"
    ));
    assert!(SQL.contains(
        "FOREIGN KEY(project_id,workspace_id,requirement_revision_id,matching_report_id)"
    ));
    assert!(SQL.contains("REFERENCES bid_evidence_bundle_items(project_id,workspace_id,evidence_bundle_id,id,source_media_revision_id)"));
    assert!(SQL.contains("REFERENCES knowledge_image_artifact_revisions(id,object_ref,content_sha256,media_type,object_state)"));
    assert!(SQL.contains("item_payload->>'evidence_item_id')::uuid=id"));
    assert!(SQL.contains("item_payload->>'kind' IS NOT DISTINCT FROM item_kind"));
    assert!(SQL.contains("ALTER TABLE knowledge_image_artifact_revisions"));
    assert!(SQL.contains("REFERENCES object_registry(object_ref,digest,media_type,state)"));
    assert!(SQL.contains("REFERENCES knowledge_matching_scope_attestations_v2(id,content_sha256)"));
    assert!(SQL.contains("kb_bid_v2_validate_render_snapshot_payload"));
    assert!(SQL.contains("kb_bid_v2_validate_render_snapshot_strict"));
    assert!(SQL.contains("kb_bid_v2_verify_render_snapshot_projection"));
    assert!(SQL.contains("kb_bid_v2_validate_evidence_bundle_payload"));
    assert!(SQL.contains("item->>'quote_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(item->>'quote_utf8','UTF8'))"));
    assert!(SQL.contains("kb_bid_v2_rfc3339_datetime_matches"));
    assert!(SQL.contains("CREATE TABLE bid_content_generation_request_identities"));
    assert!(SQL.contains("request_kind text NOT NULL DEFAULT 'content_generate' CHECK (request_kind='content_generate')"));
    assert!(SQL.contains(
        "request_operation text NOT NULL CHECK (request_operation IN ('match_only','generate'))"
    ));
    assert!(SQL.contains("FOREIGN KEY(project_id,workspace_id,base_workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id)"));
    assert!(SQL.contains("REFERENCES bid_outline_checkpoint_artifacts(project_id,workspace_id,id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,content_sha256)"));
    assert!(SQL.contains("REFERENCES bid_evidence_selection_artifacts(project_id,workspace_id,id,content_sha256,selection_kind,matching_report_id)"));
    assert!(SQL.contains("CREATE TABLE bid_content_generation_request_evidence_bundles"));
    assert!(SQL.contains(
        "REFERENCES bid_evidence_bundle_artifacts(project_id,workspace_id,id,content_sha256)"
    ));
    assert!(SQL.contains("CREATE FUNCTION kb_bid_v2_validate_content_generation_anchor"));
    assert!(SQL.contains("kb_bid_v2_verify_request_typed_projection"));
    assert!(SQL.contains("async request must have exactly one matching typed projection"));
    assert!(SQL.contains("kb_bid_v2_guard_async_request_initial_state"));
    assert!(SQL.contains("async request initial status must be pending"));
    assert!(SQL.contains("kb_bid_v2_guard_async_request_transition"));
    assert!(SQL.contains("kb_bid_v2_validate_candidate_request_identity"));
    assert!(SQL.contains("kb_bid_v2_guard_candidate_initial_state"));
    assert!(SQL.contains("candidate initial state must be proposed and undecided"));
    assert!(SQL.contains("kb_bid_v2_guard_candidate_transition"));
    assert!(SQL.contains("request_operation text NOT NULL CHECK (request_operation IN ('outline_generate','generate'))"));
    assert!(SQL.contains("evidence_selection_sha256 kb_sha256 NOT NULL"));
    assert!(SQL.contains("pick_set_matching_report_id uuid"));
    assert!(SQL.contains("matching_policy_id uuid"));
    assert!(SQL.contains("prompt_contract_id uuid NOT NULL"));
    assert!(SQL.contains("template_contract_id uuid NOT NULL"));
    assert!(SQL.contains("model_contract_id uuid NOT NULL"));
    assert!(SQL.contains("agent_contract_id uuid NOT NULL"));
    assert!(SQL.contains("CREATE TABLE bid_tender_document_process_request_identities"));
    assert!(SQL.contains("converter_contract_id uuid NOT NULL"));
    assert!(SQL.contains("ADD FOREIGN KEY(converter_contract_id,converter_contract_sha256)"));
    assert!(SQL.contains("TenderDocumentProcess publication set digest mismatch"));
    assert!(SQL.contains(
        "(p_source->>'converter_contract_id')::uuid<>typed_request.converter_contract_id"
    ));
    assert!(SQL.contains("'source_unit_set_sha256',computed_source_unit_set_sha"));
    assert!(SQL.contains("CREATE TABLE bid_requirement_set_compile_request_identities"));
    assert!(SQL.contains("CREATE TABLE bid_outline_generation_request_identities"));
    assert!(SQL.contains("CREATE TABLE bid_submission_export_request_identities"));
    assert!(SQL.contains("EvidenceAsset knowledge media qualified identity mismatch"));
    assert!(SQL.contains("kb_bid_v2_verify_evidence_bundle_projection"));
    for issue_code in [
        "DOCUMENT_INPUT_NOT_READY",
        "UNRESOLVED_REQUIREMENT",
        "MANDATORY_REQUIREMENT_UNBOUND",
        "DEVIATION_REVIEW_REQUIRED",
        "SCORING_EVIDENCE_MISSING",
        "STRUCTURED_FORM_INCOMPLETE",
        "ATTACHMENT_PREPARATION_MISSING",
        "FULFILLMENT_EVIDENCE_STALE_OR_MISSING",
        "STALE_CONTENT",
        "NO_ELIGIBLE_EVIDENCE",
        "QUOTE_SNAPSHOT_MISSING",
    ] {
        assert!(
            SQL.contains(issue_code),
            "missing deterministic Assessment issue {issue_code}"
        );
    }
    assert!(SQL.contains("canonical_payload-'snapshot_sha256'"));
    assert!(SQL.contains("canonical_payload-'bundle_sha256'"));
    assert!(SQL.contains("kb_bid_v2_manifest_expected_dependencies"));
    assert!(SQL.contains("kb_bid_v2_verify_manifest_dependency_set"));
    assert!(
        SQL.contains("REFERENCES object_registry(object_ref,digest,media_type,byte_length,state)")
    );
    assert!(
        SQL.contains(
            "REFERENCES object_owner_references(object_ref,owner_kind,owner_id,occurrence)"
        )
    );
    assert!(SQL.contains("UNION ALL SELECT 'outline_checkpoint',outline_checkpoint_id,outline_checkpoint_sha256 FROM manifest"));
    assert!(SQL.contains("canonical_payload jsonb NOT NULL"));
    assert!(SQL.contains("outline_checkpoint_sha256 kb_sha256 NOT NULL"));
    assert!(SQL.contains("workspace_sha256 kb_sha256 NOT NULL"));
    assert!(SQL.contains("bid_render_font_artifacts"));
    assert!(SQL.contains("bid_render_snapshot_font_items"));
    assert!(SQL.contains("REFERENCES bid_render_font_artifacts(id,object_ref,content_sha256,media_type,family,script)"));
    for storage_contract in [
        "CREATE TABLE knowledge_image_artifact_revisions",
        "CREATE TABLE knowledge_image_ocr_chunk_artifact_mappings",
        "REFERENCES chunks(id,product_version_id,document_id)",
        "KNOWLEDGE_IMAGE_OCR_MAPPING_SOURCE_INVALID",
    ] {
        assert!(
            KNOWLEDGE_SQL.contains(storage_contract),
            "missing knowledge media storage contract: {storage_contract}"
        );
    }
    assert!(!KNOWLEDGE_SQL.contains("KnowledgeEvidenceHitV3"));
}

#[test]
fn v2_baseline_has_no_deleted_or_transport_state() {
    let normalized = SQL.to_ascii_lowercase();
    for forbidden in [
        "bid_part_content_artifacts",
        "bid_current_parts",
        "submissiongate",
        "submission_gate",
        "template_slot",
        "company_profile",
        "submission_profile",
        "procedural_classification",
        "procedural_decision",
        "delivery_attempt",
        "lease_expires",
        "retry_count",
        "dispatch_head",
        "dispatch_intent",
        "fan_out",
        "fan_in",
    ] {
        assert!(
            !normalized.contains(forbidden),
            "forbidden V2 SQL: {forbidden}"
        );
    }
    assert!(SQL.contains("request_kind IN ('tender_document_process','requirement_set_compile','outline_generate','content_generate','submission_export')"));
    assert!(!SQL.contains("matching_schedule"));
    assert!(!SQL.contains("attachment_preparation_jobs"));
}

#[test]
fn phase_one_vertical_has_owner_checked_mutations_and_is_active() {
    for procedure in [
        "kb_bid_v2_create_project",
        "kb_bid_v2_upload_tender_document",
        "kb_bid_v2_patch_document_role",
        "kb_bid_v2_upsert_document_relation",
        "kb_bid_v2_freeze_document_set",
        "kb_bid_v2_compile_requirement_set",
        "kb_bid_v2_list_source_units",
        "kb_bid_v2_list_requirements",
    ] {
        assert!(
            SQL.contains(&format!("CREATE FUNCTION {procedure}")),
            "{procedure}"
        );
    }
    assert!(SQL.contains("PERFORM kb_bid_v2_require_project_owner"));
    assert!(SQL.contains("owner_user_id=split_part(p_actor,':',2)::uuid"));
    assert!(SQL.contains("PROJECT_OWNER_REQUIRED"));
    assert!(SQL.contains("kb_bid_v2_idempotency_begin"));
    assert!(SQL.contains("DOCUMENT_SET_CAS_MISMATCH"));
    assert!(SQL.contains("disposition='requirement'"));
    assert!(SQL.contains("'requirement_projection_id',projection_id"));
    assert!(PHASE1_LIVE.contains("cross-owner tender read accepted"));
    assert!(PHASE1_LIVE.contains("idempotency payload mismatch accepted"));
    assert!(PHASE1_LIVE.contains("stale document set CAS accepted"));
    assert!(PHASE1_LIVE.contains("source unit lacks exactly one requirement disposition"));
    assert!(API_ROUTER.contains("merge(crate::bid_v2_routes::router())"));
    let active_worker = WORKER
        .split("\n#[cfg(test)]")
        .next()
        .expect("worker source");
    assert!(active_worker.contains("queue_with_concurrency::<BidAuthoringV2Queue>"));
    assert!(active_worker.contains("TenderDocumentProcessV2Worker"));
    assert!(active_worker.contains("RequirementSetCompileV2Worker"));
    assert!(active_worker.contains("OutlineGenerateV2Worker"));
    assert!(active_worker.contains("ContentGenerateV2Worker"));
}

#[test]
fn phase_three_has_async_workers_and_live_evidence_candidate_publication() {
    for procedure in [
        "kb_bid_v2_load_outline_generation_input",
        "kb_bid_v2_publish_outline_generation",
        "kb_bid_v2_load_content_generation_input",
        "kb_bid_v2_publish_content_generation",
        "kb_bid_v2_get_evidence_overview",
    ] {
        assert!(
            SQL.contains(&format!("CREATE FUNCTION {procedure}")),
            "{procedure}"
        );
    }
    assert!(PHASE1_LIVE.contains("kb_bid_v2_publish_outline_generation"));
    assert!(PHASE3_LIVE.contains("explicit no-evidence bundle publication failed"));
    assert!(PHASE3_LIVE.contains("match_only did not complete without a candidate"));
    let active_worker = WORKER
        .split("\n#[cfg(test)]")
        .next()
        .expect("worker source");
    assert!(active_worker.contains("run_outline_generation"));
    assert!(active_worker.contains("run_content_agent"));
    assert!(active_worker.contains("exactly once"));
    for semantic_contract in [
        "kb_bid_v2_load_requirement_set_compile_input_v3",
        "kb_bid_v2_publish_requirement_set_v3",
        "kb_bid_v2_outline_semantics_valid",
        "section_obligation_bindings",
        "'system:requirement-set-compile-v3'",
        "\"map_schema\":4",
        "\"requirement_grouping_schema\":1",
        "\"fulfillment_group_schema\":1",
        "\"reduce_schema\":3",
        "\"draft_patch_schema\":1",
        "\"output_schema\":2",
    ] {
        assert!(
            SQL.contains(semantic_contract),
            "missing {semantic_contract}"
        );
    }
    assert!(SQL.contains("AND request_value.status='pending'"));
    assert!(!SQL.contains("request_value.status IN ('pending','succeeded')"));
    assert!(SQL.contains("AND state='proposed' AND id<>p_candidate_id"));
    assert!(SQL.contains(
        "ARRAY['schema_version','coverage','composition_spine',\n      'section_obligation_matrix','fulfillment_groups'"
    ));
    assert!(SQL.contains(
        "'reduce_plan_sha256','map_evidence_set_sha256','grouping_evidence_set_sha256','composition_spine'"
    ));
    assert!(
        SQL.contains(
            "'selected_evidence','selected_facts','nodes','patch_receipts','closure_facts'"
        )
    );
    assert!(
        SQL.matches("p_payload->'schema_version' IS DISTINCT FROM '3'::jsonb")
            .count()
            >= 3
    );
    assert!(SQL.contains("00000000-0000-5000-8000-000000000105"));
    assert!(SQL.contains("00000000-0000-5000-8000-000000000106"));
    assert!(SQL.contains("00000000-0000-5000-8000-000000000107"));
    assert!(SQL.contains("00000000-0000-5000-8000-000000000108"));
    assert!(SQL.contains("\"version\":8,\"map_schema\":4,\"requirement_grouping_schema\":1"));
    assert!(SQL.contains("\"progress_control\":\"semantic_closure_and_atomic_patch\""));
    assert!(SQL.contains("ORDER BY created_at DESC,checkpoint_ordinal DESC LIMIT 1"));
}

#[test]
fn active_queue_registry_is_v2_only_and_matches_implemented_workers() {
    assert!(!ACTIVE_QUEUE_REGISTRY.contains("bid:delivery:v1"));
    let registry = QueueRegistry::parse(ACTIVE_QUEUE_REGISTRY).expect("closed active registry");
    let expected = [
        (
            "bid:tender_document_process:v2",
            "TenderDocumentProcessV2Handler",
            LaunchMode::RequiredEnabled,
        ),
        (
            "bid:requirement_set_compile:v2",
            "RequirementSetCompileV2Handler",
            LaunchMode::RequiredEnabled,
        ),
        (
            "bid:outline_generate:v2",
            "OutlineGenerateV2Handler",
            LaunchMode::RequiredEnabled,
        ),
        (
            "bid:content_generate:v2",
            "ContentGenerateV2Handler",
            LaunchMode::RequiredEnabled,
        ),
        (
            "bid:submission_export:v2",
            "SubmissionExportV2Handler",
            LaunchMode::RequiredEnabled,
        ),
    ];
    let bid_entries: Vec<_> = registry
        .entries()
        .iter()
        .filter(|entry| entry.task_type.starts_with("bid:"))
        .collect();
    assert_eq!(bid_entries.len(), expected.len());
    for (task, handler, mode) in expected {
        let entry = registry.entry_for_task(task).expect("V2 task");
        assert_eq!(entry.physical_queue, BID_AUTHORING_V2_QUEUE);
        assert_eq!(entry.handler, handler);
        assert_eq!(entry.payload_schema, BID_AUTHORING_V2_PAYLOAD_SCHEMA);
        assert_eq!(
            entry.payload_version,
            u32::from(BID_AUTHORING_V2_PAYLOAD_VERSION)
        );
        assert_eq!(entry.launch_mode, mode);
    }
}
