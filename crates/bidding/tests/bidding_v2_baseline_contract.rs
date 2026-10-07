const SQL: &str = include_str!("../../../migrations/bidding_v2_baseline.sql");
const KNOWLEDGE_SQL: &str = include_str!("../../../migrations/knowledge_base_baseline.sql");
const SHARED_SQL: &str = include_str!("../../../migrations/shared_platform_baseline.sql");
const PLATFORM_DB: &str = include_str!("../../platform/src/db.rs");
const RETENTION: &str = include_str!("../../retention/src/main.rs");
const KNOWLEDGE_CLONE: &str = include_str!("../../knowledge/src/clone/mod.rs");
const KNOWLEDGE_SEARCH: &str = include_str!("../../knowledge/src/search/mod.rs");
const FRESH_SCHEMA_ACCEPTANCE: &str = include_str!("../../../scripts/fresh_schema_acceptance.sh");
const BIDDING_TEST_SUPPORT: &str = include_str!("support/mod.rs");

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
    assert!(!SQL.contains("bid_requirement_set_artifacts"));
    assert!(SQL.contains("bid_outline_artifacts"));
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
fn outline_and_response_replace_the_authoring_machine() {
    let tables = create_table_names();
    for table in [
        "bid_projects",
        "bid_documents",
        "bid_outline_runs",
        "bid_outline_artifacts",
        "bid_outline_chapters",
        "bid_outline_attachment_bindings",
        "bid_outline_template_slots",
        "bid_response_sets",
        "bid_response_slots",
        "bid_docx_versions",
        "bid_docx_current",
        "bid_submission_exports",
    ] {
        assert!(tables.contains(&table), "missing table {table}");
    }
    for removed in [
        "bid_requirement_set_artifacts",
        "bid_submission_workspaces",
        "bid_candidate_artifacts",
        "bid_evidence_bundle_artifacts",
        "bid_content_generation_request_identities",
        "kb_bid_v2_publish_requirement_set",
        "kb_bid_v2_advance_workspace_head",
        "system:",
    ] {
        assert!(
            !SQL.contains(removed),
            "old authoring flow still present: {removed}"
        );
    }
    assert!(SQL.contains("kb_bid_v2_publish_outline"));
    assert!(SQL.contains("kb_bid_v2_publish_response"));
    assert!(SQL.contains("【待人工补充】"));
    assert!(SQL.contains("DOCX_VERSION_CAS_MISMATCH"));
    assert!(SQL.contains("DOCX_SAVE_PENDING"));
    assert!(SQL.contains("PRIMARY KEY (outline_sha256, form_id)"));
    assert!(SQL.contains("editor_key uuid"));
    assert!(SQL.contains("p_actor IS NOT NULL AND p_actor IS DISTINCT FROM"));
}
