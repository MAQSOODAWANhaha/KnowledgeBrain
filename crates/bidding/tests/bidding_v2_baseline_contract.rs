const SQL: &str = include_str!("../../../migrations/bidding_v2_baseline.sql");
const KNOWLEDGE_SQL: &str = include_str!("../../../migrations/knowledge_base_baseline.sql");
const SHARED_SQL: &str = include_str!("../../../migrations/shared_platform_baseline.sql");
const PLATFORM_DB: &str = include_str!("../../platform/src/db.rs");
const ACTIVE_QUEUE_REGISTRY: &str = include_str!("../../../deploy/queue-registry.toml");
const PHASE1_ACCEPTANCE: &str = include_str!("sql/phase1_acceptance.sql");
const API_ROUTER: &str = include_str!("../../api/src/routes.rs");
const BID_API_ROUTER: &str = include_str!("../../api/src/bid_v2_routes.rs");
const RETENTION: &str = include_str!("../../retention/src/main.rs");
const KNOWLEDGE_CLONE: &str = include_str!("../../knowledge/src/clone/mod.rs");
const KNOWLEDGE_SEARCH: &str = include_str!("../../knowledge/src/search/mod.rs");
const KNOWLEDGE_INDEX_V2: &str = include_str!("../../knowledge/src/knowledge_index_v2.rs");
const FRESH_SCHEMA_ACCEPTANCE: &str = include_str!("../../../scripts/fresh_schema_acceptance.sh");
const BIDDING_TEST_SUPPORT: &str = include_str!("support/mod.rs");
const CONTENT_ERROR_REGISTRY: &str = include_str!("../schemas/content-error-codes-v1.json");
const REQUEST_HANDLER_ERROR_REGISTRY: &str =
    include_str!("../schemas/request-handler-error-codes-v1.json");

use platform::{
    BID_AUTHORING_V2_PAYLOAD_SCHEMA, BID_AUTHORING_V2_PAYLOAD_VERSION, BID_AUTHORING_V2_QUEUE,
};
use platform::{LaunchMode, QueueRegistry};

fn create_table_names() -> Vec<&'static str> {
    SQL.lines()
        .filter_map(|line| line.trim().strip_prefix("CREATE TABLE "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect()
}

#[test]
fn destructive_postgres_tests_require_an_isolated_non_live_database() {
    for (name, source) in [
        ("knowledge clone", KNOWLEDGE_CLONE),
        ("knowledge search", KNOWLEDGE_SEARCH),
    ] {
        assert!(source.contains("DROP SCHEMA public CASCADE"), "{name}");
        assert!(
            source.contains("KNOWLEDGEBRAIN_TEST_DATABASE_URL"),
            "{name}"
        );
        assert!(source.contains(":15432/"), "{name}");
    }
    assert!(FRESH_SCHEMA_ACCEPTANCE.contains("urlparse"));
    assert!(FRESH_SCHEMA_ACCEPTANCE.contains("url.port != 25433"));
    assert!(FRESH_SCHEMA_ACCEPTANCE.contains("knowledgebrain_test_"));
    assert!(BIDDING_TEST_SUPPORT.contains("PgConnectOptions"));
    assert!(BIDDING_TEST_SUPPORT.contains("get_port() == 25433"));
    assert!(BIDDING_TEST_SUPPORT.contains("knowledgebrain_test_"));
    assert!(!BIDDING_TEST_SUPPORT.contains("platform::database_url()"));
}

#[test]
fn platform_order_and_knowledge_object_ownership_are_frozen() {
    let owner = PLATFORM_DB.find("SET LOCAL ROLE kb_app_owner").unwrap();
    let shared = PLATFORM_DB
        .find("sqlx::raw_sql(SHARED_PLATFORM_BASELINE)")
        .unwrap();
    let knowledge = PLATFORM_DB
        .find("sqlx::raw_sql(KNOWLEDGE_BASE_BASELINE)")
        .unwrap();
    let bidding = PLATFORM_DB.find("sqlx::raw_sql(BIDDING_BASELINE)").unwrap();
    assert!(owner < shared && shared < knowledge && knowledge < bidding);
    for symbol in [
        "kb_register_knowledge_image_object",
        "kb_register_knowledge_document_object",
        "kb_release_knowledge_document_object",
        "kb_validate_knowledge_document_object_reference",
        "kb_guard_knowledge_document_delete",
        "documents_object_reference_contract",
        "documents_object_reference_delete_guard",
    ] {
        assert!(!SHARED_SQL.contains(symbol), "Shared still owns {symbol}");
        assert!(
            KNOWLEDGE_SQL.contains(symbol),
            "Knowledge is missing {symbol}"
        );
    }
}

#[test]
fn bidding_never_alters_shared_or_knowledge_owned_inventory() {
    for foreign_object in [
        "object_registry",
        "knowledge_image_artifact_revisions",
        "knowledge_matching_scope_attestations_v2",
    ] {
        assert!(
            !SQL.contains(&format!("ALTER TABLE {foreign_object}")),
            "Bidding alters foreign-owned object {foreign_object}"
        );
    }
    assert!(SHARED_SQL.contains("UNIQUE(object_ref,digest,state)"));
    assert!(SHARED_SQL.contains("UNIQUE(object_ref,digest,media_type,state)"));
    assert!(SHARED_SQL.contains("UNIQUE(object_ref,digest,media_type,byte_length,state)"));
    assert!(
        KNOWLEDGE_SQL.contains("FOREIGN KEY(object_ref,content_sha256,media_type,object_state)")
    );
    assert!(KNOWLEDGE_SQL.contains("UNIQUE NULLS NOT DISTINCT(id,object_ref,content_sha256,media_type,object_state,width,height,page_ordinal,bounding_region)"));
    assert!(KNOWLEDGE_SQL.contains("UNIQUE(id,content_sha256)"));
    assert!(SQL.contains("ALTER TABLE bid_tender_source_image_revision_artifacts"));
}

#[test]
fn retention_tests_require_the_isolated_migrated_shared_schema_only() {
    assert!(RETENTION.contains("KNOWLEDGEBRAIN_TEST_DATABASE_URL"));
    assert!(RETENTION.contains(":15432/"));
    assert!(!RETENTION.contains("platform::database_url()"));
    assert!(!RETENTION.contains("platform::apply_fresh_baseline"));
    assert!(RETENTION.contains("kb_object_upload_expiry_candidates()"));
    assert!(RETENTION.contains("kb_object_upload_expire_one(uuid)"));
    assert!(RETENTION.contains("kb_retention_preflight(uuid,kb_object_ref,kb_sha256,bigint)"));
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
    assert!(SQL.contains("requiredness IN ('mandatory','optional','informational','unknown')"));
    assert!(SQL.contains(
        "compliance_policy IN ('must_comply','explicit_response','deviation_allowed','scored','unknown')"
    ));
    assert!(SQL.contains("lifecycle IN ('current','superseded','withdrawn','unresolved')"));
    assert!(SQL.contains("status IN ('ready','has_warnings','has_critical_warnings')"));
    assert!(SQL.contains("mode_options jsonb NOT NULL"));
    for function in [
        "kb_bid_v2_get_requirement_projection",
        "kb_bid_v2_get_requirement_set_compile_request",
        "kb_bid_v2_refresh_requirement_projection",
        "kb_bid_v2_retire_workspace_asset",
        "kb_bid_v2_create_node_evidence_pick_set",
        "kb_bid_v2_get_node_evidence",
        "kb_bid_v2_prepare_workspace_attachment",
        "kb_bid_v2_load_user_pick_evidence",
        "kb_bid_v2_publish_quote_snapshot",
        "kb_bid_v2_publish_submission_export",
        "kb_bid_v2_load_submission_export_source",
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
        "/requirement-set-compilations/{request_id}",
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
fn owner_projection_publication_and_worker_terminal_contracts_are_frozen() {
    let patch = &SQL[SQL
        .find("CREATE FUNCTION kb_bid_v2_patch_requirement")
        .unwrap()
        ..SQL
            .find("CREATE FUNCTION kb_bid_v2_publish_requirement_supersession")
            .unwrap()];
    let supersession = &SQL[SQL
        .find("CREATE FUNCTION kb_bid_v2_publish_requirement_supersession")
        .unwrap()
        ..SQL
            .find("CREATE FUNCTION kb_bid_v2_list_requirements")
            .unwrap()];
    assert!(!patch.contains("PERFORM kb_bid_v2_advance_workspace_projection"));
    assert!(!supersession.contains("PERFORM kb_bid_v2_advance_workspace_projection"));
    assert!(patch.contains("'workspace_apply_required',true"));
    assert!(supersession.contains("'workspace_apply_required',true"));

    let tender_failure = &SQL[SQL
        .find("CREATE FUNCTION kb_bid_v2_mark_tender_document_failed")
        .unwrap()..SQL.find("REVOKE ALL ON ALL TABLES").unwrap()];
    let request_lock = tender_failure.find("FOR UPDATE").unwrap();
    let typed_fence = tender_failure
        .find("FROM bid_tender_document_process_request_identities")
        .unwrap();
    let terminal_guard = tender_failure
        .find("status<>'pending' THEN RETURN")
        .unwrap();
    let document_update = tender_failure.find("UPDATE bid_documents").unwrap();
    assert!(request_lock < typed_fence && typed_fence < terminal_guard);
    assert!(terminal_guard < document_update);
    assert!(SQL.contains("page_count integer CHECK (page_count > 0 AND page_count <= 1000)"));
}

#[test]
fn v4_requirement_publication_never_advances_workspace() {
    let v4 = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_publish_requirement_set_v4(")
        .expect("V4 publisher")
        .1
        .split_once("END $$;")
        .expect("V4 publisher fence")
        .0;
    assert!(v4.contains("publication_status='superseded'"));
    assert!(!v4.contains("publication_status='obsolete'"));
    assert!(!v4.contains("kb_bid_v2_advance_workspace_projection"));
    assert!(v4.contains("'published_current',false"));
    assert!(v4.contains("'workspace_apply_required',false"));
    assert!(v4.contains("'published_current',true"));
    // Analysis publishes a source basis; DOCX rounds are created explicitly.
    assert!(!v4.contains("'workspace_apply_required',true"));
    assert!(!SQL.contains("CREATE FUNCTION kb_bid_v2_load_requirement_set_compile_input_v3("));
    assert!(!SQL.contains("CREATE FUNCTION kb_bid_v2_publish_requirement_set_v3("));
    assert!(v4.contains("publication_status<>'published'"));
}

#[test]
fn typed_compile_status_and_candidate_obsolete_api_contracts_are_frozen() {
    assert!(SQL.contains("request_value.request_kind='requirement_set_compile'"));
    assert!(SQL.contains("identity_value.project_id=p_project_id"));
    assert!(
        SQL.contains("kb_bid_v2_get_requirement_set_compile_request(uuid,uuid,kb_actor_identity)")
    );
    assert!(BID_API_ROUTER.contains("get_requirement_set_compile_request_v2"));
    let candidate_accept = BID_API_ROUTER
        .split_once("async fn accept_candidate(")
        .expect("candidate accept handler")
        .1
        .split_once("async fn reject_candidate(")
        .expect("candidate accept fence")
        .0;
    assert_eq!(candidate_accept.matches("CANDIDATE_OBSOLETE").count(), 2);
    assert_eq!(
        candidate_accept
            .matches("candidate_obsolete_error(")
            .count(),
        2
    );
    assert!(BID_API_ROUTER.contains("fn candidate_obsolete_error(receipt: &Value)"));
    assert!(BID_API_ROUTER.contains("current_workspace_revision_id"));
    assert!(BID_API_ROUTER.contains("current_workspace_sha256"));
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
    assert!(requirement_publish.contains("RETURN 'superseded'"));
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
    assert!(SQL.contains("p_docx->>'sha256' IS DISTINCT FROM typed.docx_sha256::text"));
    assert!(!SQL.contains("include_assessment_notices"));
    assert!(!SQL.contains("include_knowledge_sources"));
    assert!(
        SQL.contains("status text NOT NULL CHECK (status IN ('pending','succeeded','failed'))")
    );
    assert!(
        SQL.contains("state text NOT NULL CHECK (state IN ('proposed','accepted','rejected'))")
    );
    assert!(SQL.contains("kb_bid_v2_apply_quote_snapshot"));
    assert!(SQL.contains("kb_bid_v2_fulfillment_evidence_is_current"));
    assert!(SQL.contains("kb_bid_v2_record_accepted_candidate_evidence"));
    let generic_mutation = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_commit_workspace_mutation(")
        .unwrap()
        .1
        .split_once("CREATE FUNCTION kb_bid_v2_record_accepted_candidate_evidence(")
        .unwrap()
        .0;
    assert!(!generic_mutation.contains("bid_submission_fulfillment_evidence_revision_artifacts"));
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
    assert!(SQL.contains("FOREIGN KEY(project_id,workspace_id,round_id,version_id,docx_sha256)"));
    assert!(
        SQL.contains("FOREIGN KEY(project_id,workspace_id,manifest_id,docx_output_id,docx_format)")
    );
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
    assert!(
        KNOWLEDGE_SQL.contains("FOREIGN KEY(object_ref,content_sha256,media_type,object_state)")
    );
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
    assert!(SQL.contains("insertion anchor is outside the frozen target subtree"));
    assert!(SQL.contains("insertion block is outside the frozen anchor node"));
    assert!(SQL.contains(
        "SELECT identity.request_operation FROM bid_content_generation_request_identities identity"
    ));
    assert!(SQL.contains("kb_bid_v2_verify_request_typed_projection"));
    assert!(SQL.contains("async request must have exactly one matching typed projection"));
    assert!(SQL.contains("kb_bid_v2_guard_async_request_initial_state"));
    assert!(SQL.contains("async request initial status must be pending"));
    assert!(SQL.contains("kb_bid_v2_guard_async_request_transition"));
    assert!(SQL.contains("kb_bid_v2_validate_candidate_request_identity"));
    assert!(SQL.contains("kb_bid_v2_guard_candidate_initial_state"));
    assert!(SQL.contains("candidate initial state must be proposed and undecided"));
    assert!(SQL.contains("kb_bid_v2_guard_candidate_transition"));
    assert!(SQL.contains("ARRAY['state','decided_at','canonical_payload_sha256']"));
    assert!(!SQL.contains("ARRAY['state','decided_at','canonical_payload']"));
    assert!(
        SQL.contains("request_operation text NOT NULL CHECK (request_operation IN ('generate'))")
    );
    assert!(SQL.contains("evidence_selection_sha256 kb_sha256 NOT NULL"));
    assert!(SQL.contains("pick_set_matching_report_id uuid"));
    assert!(SQL.contains("matching_policy_id uuid"));
    assert!(SQL.contains("prompt_contract_id IS NOT NULL AND prompt_contract_sha256 IS NOT NULL"));
    assert!(
        SQL.contains("template_contract_id IS NOT NULL AND template_contract_sha256 IS NOT NULL")
    );
    assert!(SQL.contains("model_contract_id IS NOT NULL AND model_contract_sha256 IS NOT NULL"));
    assert!(SQL.contains("agent_contract_id IS NOT NULL AND agent_contract_sha256 IS NOT NULL"));
    assert!(SQL.contains("CREATE TABLE bid_tender_document_process_request_identities"));
    assert!(SQL.contains("converter_contract_id uuid NOT NULL"));
    assert!(SQL.contains("ADD FOREIGN KEY(converter_contract_id,converter_contract_sha256)"));
    assert!(SQL.contains("TenderDocumentProcess publication set digest mismatch"));
    assert!(SQL.contains(
        "(p_source->>'converter_contract_id')::uuid<>typed_request.converter_contract_id"
    ));
    assert!(SQL.contains("'source_unit_set_sha256',computed_source_unit_set_sha"));
    assert!(SQL.contains("CREATE TABLE bid_requirement_set_compile_request_identities"));
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
    assert!(SQL.contains("('docx_version','version_id','docx_sha256')"));
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
        "retry_count",
        "dispatch_head",
        "dispatch_intent",
        "fan_out",
        "fan_in",
        "initial_enqueue_reserved_at",
        "automatic_reconcile_reserved_at",
        "operator_requeue_required",
        "request_delivery_state",
        "claim_stale_request_deliveries",
        "reserve_request_delivery",
    ] {
        assert!(
            !normalized.contains(forbidden),
            "forbidden V2 SQL: {forbidden}"
        );
    }
    assert!(SQL.contains("request_kind IN ('tender_document_process','requirement_set_compile','content_generate','submission_export','docx_compose','docx_layout')"));
    assert!(SQL.contains(
        "stage_kind IN ('analysis_checkpoint','export_review_checkpoint','layout_checkpoint')"
    ));
    assert!(SQL.contains("CREATE FUNCTION kb_bid_v2_layout_checkpoint_put"));
    assert!(SQL.contains("WHERE id=p_request_artifact_id AND request_kind='submission_export'"));
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
        "kb_bid_v2_get_tender_outline",
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
    assert!(PHASE1_ACCEPTANCE.contains("cross-owner tender read accepted"));
    assert!(PHASE1_ACCEPTANCE.contains("idempotency payload mismatch accepted"));
    assert!(PHASE1_ACCEPTANCE.contains("stale document set CAS accepted"));
    assert!(PHASE1_ACCEPTANCE.contains("source unit lacks exactly one requirement disposition"));
    assert!(API_ROUTER.contains("merge(crate::bid_v2_routes::router())"));
    let registry = QueueRegistry::load().expect("queue registry");
    for task in [
        platform::BID_TENDER_DOCUMENT_PROCESS_V2_TASK,
        platform::BID_REQUIREMENT_SET_COMPILE_V2_TASK,
        platform::BID_DOCX_COMPOSE_V2_TASK,
        platform::BID_CONTENT_GENERATE_V2_TASK,
        platform::BID_SUBMISSION_EXPORT_V2_TASK,
    ] {
        let entry = registry.entry_for_task(task).expect(task);
        assert_eq!(entry.physical_queue, "bid-authoring-v2");
        assert_eq!(entry.launch_mode, LaunchMode::RequiredEnabled);
    }
}

#[test]
fn content_terminal_error_allowlist_matches_the_separate_registry() {
    let registry: serde_json::Value = serde_json::from_str(CONTENT_ERROR_REGISTRY).unwrap();
    let expected = registry["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["code"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    let failure = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_mark_content_generation_failed(")
        .unwrap()
        .1
        .split_once("CREATE FUNCTION kb_bid_v2_create_evidence_pick_set(")
        .unwrap()
        .0;
    let allowlist = failure
        .split_once("IF p_error_code NOT IN (")
        .unwrap()
        .1
        .split_once(") THEN")
        .unwrap()
        .0;
    let actual = allowlist
        .split('\'')
        .enumerate()
        .filter_map(|(index, value)| (index % 2 == 1).then_some(value))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert!(SQL.contains("CREATE TABLE bid_content_agent_run_artifacts"));
    assert!(SQL.contains("CREATE TABLE bid_content_agent_boundary_attempts"));
    assert!(SQL.contains("kb_bid_v2_content_lock_owner"));
    assert!(failure.contains("kb_bid_v2_content_lock_owner"));
}

#[test]
fn non_agent_timeout_allowlists_match_the_separate_handler_registry() {
    let registry: serde_json::Value = serde_json::from_str(REQUEST_HANDLER_ERROR_REGISTRY).unwrap();
    let codes = registry["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["code"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        codes,
        std::collections::BTreeSet::from([
            "REQUIREMENT_COMPILE_TIMEOUT",
            "SUBMISSION_EXPORT_TIMEOUT",
            "TENDER_DOCUMENT_PROCESS_TIMEOUT",
        ])
    );
    for code in codes {
        assert!(SQL.contains(&format!("'{code}'")), "SQL omits {code}");
    }
    assert!(!CONTENT_ERROR_REGISTRY.contains("SUBMISSION_EXPORT_TIMEOUT"));
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
            "bid:docx_compose:v2",
            "DocxComposeV2Handler",
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

#[test]
fn remediation_fences_are_present() {
    assert!(SQL.contains("QUOTE_SNAPSHOT_NOT_CURRENT"));
    assert!(SQL.contains("current_quote bid_quote_snapshot_current%ROWTYPE"));
    assert!(SQL.contains("p_new_projection_sha256 kb_sha256,p_actor kb_actor_identity"));
    assert!(SQL.contains("payload,digest,p_actor"));
    assert!(SQL.contains("owned.owner_kind='bid_docx_version'"));
    assert!(SQL.contains("WHERE id=p_request_artifact_id FOR UPDATE"));
    assert!(SQL.contains("IF request_value.status<>'pending' THEN RETURN"));
    assert!(SQL.contains("SELECT status INTO STRICT project_status FROM bid_projects WHERE id=p_project_id FOR UPDATE"));
}

#[test]
fn runtime_vector_retrieval_and_image_snapshot_contracts_are_frozen() {
    assert!(KNOWLEDGE_SQL.contains(
        "GRANT EXECUTE ON FUNCTION vector_in(cstring,oid,integer),\n    cosine_distance(vector,vector)\nTO kb_runtime_api, kb_runtime_worker"
    ));
    assert!(KNOWLEDGE_SQL.contains(
        "GRANT EXECUTE ON FUNCTION kb_knowledge_rebuild_keyword_indexes_v2(uuid),\n    kb_knowledge_has_pending_derived_v2(uuid)"
    ));
    assert_eq!(
        KNOWLEDGE_SQL
            .matches("chunk.chunk_type<>'image_ocr' OR EXISTS")
            .count(),
        3,
        "source freeze and both reconciliation passes must exclude unattested OCR"
    );
    assert!(KNOWLEDGE_INDEX_V2.contains("FOR SHARE OF version\""));
    assert!(!KNOWLEDGE_INDEX_V2.contains("FOR SHARE OF version,binding,revision"));
}

#[test]
fn request_delivery_uses_oxana_without_a_postgres_reconciler() {
    assert!(SQL.contains("kb_bid_v2_authoring_job_payload"));
    assert!(SQL.contains("REQUEST_JOB_PAYLOAD_MISMATCH"));
    assert!(SQL.contains("kb_bid_v2_load_authoring_job_payload"));
    assert!(!SQL.contains("automatic_reconcile"));
    assert!(!SQL.contains("kb_bid_v2_claim_stale_request_deliveries"));
    assert!(BID_API_ROUTER.contains("load_authoring_job_payload_v2(pool, &request)"));
    assert!(BID_API_ROUTER.contains("platform::enqueue_bid_authoring_v2(payload)"));
    assert!(!BID_API_ROUTER.contains("reserve_request_delivery_v2(pool, &request, \"api\")"));
}

/// 填章跑在 `docx_compose` 请求轨道的 `draft-fill` 模式上。这条用例钉住那条轨道
/// 上「谁能放行 draft、谁必须拒 draft」的两个方向，以及出稿的三道闸门：检查点攻
/// 证、字节摘要、编辑器会话未关时不得入稿。正式编制入口已删除，只保留 draft-fill。
#[test]
fn fill_has_no_mode_and_preserves_seed_and_publication_guards() {
    let identities = SQL
        .split_once("CREATE TABLE bid_docx_composition_request_identities (")
        .unwrap()
        .1
        .split_once("\n);")
        .unwrap()
        .0;
    assert!(identities.contains("'seed_plan_sha256'"));
    assert!(!identities.contains("'mode'"));
    assert!(identities.contains("kb_bid_v2_sha256_text(frozen_input->>'seed_plan_sha256')"));
    assert!(!identities.contains("'official','draft-fill'"));
    for function in [
        "CREATE FUNCTION kb_bid_v2_load_docx_composition_source(",
        "CREATE FUNCTION kb_bid_v2_prepare_docx_composition_source(",
    ] {
        let body = SQL
            .split_once(function)
            .unwrap()
            .1
            .split_once("END $$;")
            .unwrap()
            .0;
        assert!(
            !body.contains("p_mode"),
            "{function} must not retain mode routing"
        );
    }
    let load = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_load_docx_composition_source(")
        .unwrap()
        .1
        .split_once("END $$;")
        .unwrap()
        .0;
    assert!(
        load.contains("DOCX_COMPOSITION_INPUT_INVALID: draft fill requires a draft analysis"),
        "填章不能跑在已复核终稿上"
    );
    let create = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_create_docx_composition_request(")
        .unwrap()
        .1
        .split_once("END $$;")
        .unwrap()
        .0;
    assert!(create.contains("DOCX_COMPOSITION_INPUT_INVALID: draft fill contract"));
    assert!(create.contains("DOCX_COMPOSITION_INPUT_INVALID: draft fill budgets required"));
    assert!(create.contains("'max_draft_docx_bytes'"));
    assert!(!create.contains("p_snapshot->>'mode'"));
    assert!(!create.contains("p_snapshot->>'mode'='official'"));
    for function in [
        "CREATE FUNCTION kb_bid_v2_tender_agent_runtime(",
        "CREATE FUNCTION kb_bid_v2_tender_agent_source_input(",
    ] {
        let body = SQL
            .split_once(function)
            .unwrap()
            .1
            .split_once("END $$;")
            .unwrap()
            .0;
        assert!(
            body.contains("bid_requirement_set_compile_request_identities"),
            "{function}"
        );
        assert!(
            body.contains("bid_docx_composition_request_identities")
                && !body.contains("frozen_input->>'mode'"),
            "{function}"
        );
    }
    // 填章的第一个检查点带着整棵回读章树，只能由请求里冻的 seed 摘要放行。
    let checkpoint = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_tender_agent_checkpoint_put(")
        .unwrap()
        .1
        .split_once("END $$;")
        .unwrap()
        .0;
    assert!(checkpoint.contains("frozen_input->>'seed_plan_sha256' INTO seed_plan"));
    assert!(checkpoint.contains(
        "(seed_plan IS NOT NULL AND kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(\n        p_state#>'{analysis,draft_plan}'),'UTF8'))::text IS DISTINCT FROM seed_plan)"
    ));
    // 没冻 seed 的 run（大纲）第一个检查点的 draft_plan 必须是空的。
    assert!(checkpoint.contains("WHEN seed_plan IS NULL THEN '[]'::jsonb"));
    // 出稿：草稿填章不出 composition manifest，但编辑器会话未关时一律拒绝入稿。
    let publish = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_publish_docx_fill(")
        .unwrap()
        .1
        .split_once("END $$;")
        .unwrap()
        .0;
    assert!(!publish.contains("frozen_input->>'mode'"));
    assert!(publish.contains("AGENT_OUTPUT_INVALID: finished draft fill checkpoint required"));
    assert!(publish.contains("checkpoint#>'{review,draft}' IS DISTINCT FROM 'true'::jsonb"));
    assert!(publish.contains("draft_compile_object_id"));
    assert!(
        publish.contains("head.editor_key IS NOT NULL OR head.pending_save_id IS NOT NULL")
            && publish.contains("DOCX_VERSION_CAS_MISMATCH"),
        "编辑器还开着就入稿会把用户正在写的内容顶掉"
    );
    assert!(publish.contains("kb_bid_v2_create_docx_round"));
    assert!(!publish.contains("composition_manifest"));
    assert!(SQL.contains("CREATE FUNCTION kb_bid_v2_replay_docx_fill("));
    // 「停止填充」只记意向：API 记，worker 读，读的人必须持租约。
    assert!(SQL.contains("CREATE TABLE bid_docx_fill_stop_requests ("));
    let stop = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_tender_agent_stop_requested(")
        .unwrap()
        .1
        .split_once("END $$;")
        .unwrap()
        .0;
    assert!(stop.contains("kb_bid_v2_tender_agent_lock_owner"));
    let request_stop = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_request_docx_fill_stop(")
        .unwrap()
        .1
        .split_once("END $$;")
        .unwrap()
        .0;
    assert!(request_stop.contains("kb_bid_v2_require_project_owner"));
    assert!(request_stop.contains("ON CONFLICT (request_artifact_id) DO NOTHING"));
    // 角色分工：出稿与停止查询归 worker，发起停止归 api。
    let grants = SQL
        .split_once("GRANT EXECUTE ON FUNCTION kb_bid_v2_load_docx_composition_request(")
        .unwrap()
        .1;
    assert!(
        grants.contains("kb_bid_v2_publish_docx_fill(uuid,kb_sha256,integer,uuid,kb_sha256,uuid)")
    );
    assert!(grants.contains("kb_bid_v2_tender_agent_stop_requested(uuid,kb_sha256,integer,uuid)"));
    assert!(SQL.contains("kb_bid_v2_request_docx_fill_stop(uuid,uuid,kb_actor_identity),\n  kb_bid_v2_create_docx_composition_request"));
}

#[test]
fn formal_export_freezes_saved_docx_and_atomically_binds_both_outputs() {
    let export = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_create_submission_export_request(")
        .unwrap()
        .1
        .split_once("REVOKE ALL ON ALL TABLES")
        .unwrap()
        .0;
    assert!(export.contains("analysis_identity"));
    assert!(export.contains("frozen_context"));
    assert!(export.contains("DOCX_VERSION_CAS_MISMATCH"));
    assert!(export.contains("DOCX_SAVE_PENDING"));
    assert!(export.contains("DOCX_SAVE_ERROR"));
    assert!(!export.contains("official export rejects draft analysis"));
    assert!(export.contains("review_checkpoint->'done' = 'true'::jsonb"));
    assert!(export.contains("unfrozen semantic review cannot be published"));
    assert!(
        export.contains(
            "IF request_value.status='succeeded' THEN RETURN request_value.result_identity"
        )
    );
    assert!(export.contains("p_report->'source' IS DISTINCT FROM typed.source"));
    assert!(export.contains("p_report->'outputs' IS DISTINCT FROM jsonb_build_object('docx',docx_identity,'pdf',pdf_identity)"));
    assert!(!export.contains("bid_workspace_heads"));
    assert!(!export.contains("bid_render_document_snapshot_artifacts"));
    assert!(!SQL.contains("CREATE FUNCTION kb_bid_v2_prepare_submission_export("));
    assert!(!SQL.contains("CREATE FUNCTION kb_bid_v2_load_submission_manifest_render_input("));
}

#[test]
fn frozen_deadline_is_accessible_to_worker_and_uses_frozen_budget() {
    let grants = SQL
        .split_once("GRANT EXECUTE ON FUNCTION kb_bid_v2_tender_agent_claim")
        .unwrap()
        .1
        .split_once("TO kb_runtime_worker")
        .unwrap()
        .0;
    assert!(grants.contains("kb_bid_v2_tender_agent_frozen_deadline(uuid)"));
    assert!(grants.contains("kb_bid_v2_tender_agent_runtime(uuid)"));
    let guard = SQL
        .split_once("CREATE FUNCTION kb_bid_v2_tender_agent_run_deadline_guard")
        .unwrap()
        .1
        .split_once("END $$;")
        .unwrap()
        .0;
    assert!(guard.contains("budget,total_timeout_secs"));
    assert!(!guard.contains("46 minutes"));
}

#[test]
fn standalone_composer_runtime_and_sql_entrypoints_are_removed() {
    let module = include_str!("../src/docx_composition/mod.rs");
    assert!(!module.contains("pub mod agent;"));
    assert!(!module.contains("pub mod tools;"));
    for function in [
        "kb_bid_v2_docx_composition_checkpoint_get",
        "kb_bid_v2_docx_composition_checkpoint_put",
        "kb_bid_v2_docx_composition_reserve",
        "kb_bid_v2_publish_docx_composition",
        "kb_bid_v2_replay_docx_composition(",
    ] {
        assert!(!SQL.contains(function), "obsolete entrypoint {function}");
    }
    for stage in [
        "'composition_checkpoint'",
        "'composition_main'",
        "'composition_review'",
    ] {
        assert!(!SQL.contains(stage), "obsolete stage {stage}");
    }
    assert!(SQL.contains("CREATE FUNCTION kb_bid_v2_publish_docx_fill("));
    assert!(SQL.contains("CREATE FUNCTION kb_bid_v2_replay_docx_fill("));
}

#[test]
fn outline_creation_calibrates_both_entrypoints_from_a_consistent_input() {
    let source = include_str!("../src/bid_authoring_v2.rs");
    let helper = source
        .split("async fn outline_request_config(")
        .nth(1)
        .unwrap()
        .split("pub async fn freeze_document_set_v2")
        .next()
        .unwrap();
    assert!(helper.contains("kb_bid_v2_tender_budget_input"));
    assert!(helper.contains("Config::from_environment_for(&input)"));
    for name in ["freeze_document_set_v2", "publish_disposition_set_v2"] {
        let body = source
            .split(&format!("pub async fn {name}("))
            .nth(1)
            .unwrap()
            .split("pub async fn ")
            .next()
            .unwrap();
        assert!(body.contains("REPEATABLE READ"), "{name}");
        assert!(body.contains("outline_request_config("), "{name}");
        assert!(!body.contains("Config::from_environment()"), "{name}");
    }
    assert!(SQL.contains("CREATE FUNCTION kb_bid_v2_tender_budget_input("));
}

#[test]
fn fill_seed_receipts_are_frozen_without_fake_read_coverage() {
    let sql = include_str!("../../../migrations/bidding_v2_baseline.sql");
    assert!(sql.contains("immutable fill seed receipt changed"));
    assert!(sql.contains("prior#>'{analysis,fill_seed_chapters}'"));
    assert!(sql.contains("node->'grounds',node->'format_refs',node->'preserved'"));
    assert!(!sql.contains("checkpoint_contract_version' IS DISTINCT FROM '3'"));
}

#[test]
fn compilation_checkpoint_is_an_immutable_host_boundary() {
    let checkpoint = SQL
        .split("CREATE FUNCTION kb_bid_v2_tender_agent_checkpoint_put")
        .nth(1)
        .unwrap()
        .split("END $$;")
        .next()
        .unwrap();
    assert!(checkpoint.contains("invalid compilation checkpoint"));
    assert!(checkpoint.contains(
        "coalesce(prior->'draft_docx_base64','null'::jsonb) IS DISTINCT FROM 'null'::jsonb"
    ));
    assert!(checkpoint.contains("(p_state->'journal')-'sequence'"));
    assert!(checkpoint.contains("analysis,outline,checked_sha256"));
    assert!(
        checkpoint
            .contains("sequence_value<>coalesce((prior#>>'{journal,sequence}')::integer,0)+1")
    );
    assert!(SQL.contains("checkpoint_contract_version' IS DISTINCT FROM '9'"));
}
