use domain::knowledge_retrieval::{
    CompanyEvidenceRequestV1, KNOWLEDGE_EVIDENCE_SCHEMA_V1, KnowledgeRetrievalError,
    KnowledgeRetrievalPort, ProductEvidenceRequestV1, RetrievalPolicyIdentityV1,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use storage::knowledge_retrieval::PostgresKnowledgeRetrievalAdapter;
use uuid::Uuid;

mod support;

struct SelectionFixture {
    product_workspace_id: Uuid,
    product_ids: Vec<Uuid>,
    product_version_ids: Vec<Uuid>,
    noncurrent_version_id: Uuid,
    company_product_id: Uuid,
    company_version_id: Uuid,
}

struct V1GoldenFixture {
    workspace_id: Uuid,
    product_id: Uuid,
    version_id: Uuid,
    document_id: Uuid,
    trusted_chunk_id: Uuid,
    derived_chunk_id: Uuid,
    filtered_chunk_id: Uuid,
    object_ref: String,
}

fn golden_uuid(suffix: u8) -> Uuid {
    Uuid::parse_str(&format!("d1000000-0000-4000-8000-{suffix:012x}"))
        .expect("fixed V1 golden UUID")
}

async fn remove_v1_golden_fixture(pool: &PgPool, fixture: &V1GoldenFixture) {
    sqlx::query("DELETE FROM chunks WHERE document_id=$1")
        .bind(fixture.document_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM object_owner_references
          WHERE owner_kind='knowledge_document' AND owner_id=$1",
    )
    .bind(fixture.document_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM documents WHERE id=$1")
        .bind(fixture.document_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM object_registry WHERE object_ref=$1")
        .bind(&fixture.object_ref)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE products SET current_version_id=NULL WHERE id=$1")
        .bind(fixture.product_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM product_versions WHERE id=$1")
        .bind(fixture.version_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM products WHERE id=$1")
        .bind(fixture.product_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workspaces WHERE id=$1")
        .bind(fixture.workspace_id)
        .execute(pool)
        .await
        .unwrap();
}

async fn seed_v1_golden_fixture(pool: &PgPool) -> V1GoldenFixture {
    let document_id = golden_uuid(4);
    let fixture = V1GoldenFixture {
        workspace_id: golden_uuid(1),
        product_id: golden_uuid(2),
        version_id: golden_uuid(3),
        document_id,
        trusted_chunk_id: golden_uuid(5),
        derived_chunk_id: golden_uuid(6),
        filtered_chunk_id: golden_uuid(7),
        object_ref: format!("objects/{}", domain::sha256_hex(document_id.as_bytes())),
    };
    remove_v1_golden_fixture(pool, &fixture).await;

    sqlx::query(
        "INSERT INTO workspaces(id,name,slug,kind)
         VALUES($1,'V1 adapter byte golden','v1-adapter-byte-golden','product_line')",
    )
    .bind(fixture.workspace_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug)
         VALUES($1,$2,'product','V1 adapter byte golden','v1-adapter-byte-golden')",
    )
    .bind(fixture.product_id)
    .bind(fixture.workspace_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status)
         VALUES($1,$2,'v1-byte-golden','active')",
    )
    .bind(fixture.version_id)
    .bind(fixture.product_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(fixture.product_id)
        .bind(fixture.version_id)
        .execute(pool)
        .await
        .unwrap();

    let file_hash = domain::sha256_hex(fixture.document_id.as_bytes());
    sqlx::query(
        "INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state)
         VALUES($1,$2,'text/plain',1,'available')",
    )
    .bind(&fixture.object_ref)
    .bind(&file_hash)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO object_owner_references(
             object_ref,owner_kind,owner_id,occurrence,created_by)
         VALUES($1,'knowledge_document',$2,'original','system:knowledge-document-ingest')",
    )
    .bind(&fixture.object_ref)
    .bind(fixture.document_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO documents(
             id,product_version_id,type,title,parse_status,enable_status,index_ready,
             file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'file','V1 adapter byte golden','completed','enabled',true,
                'v1-golden.txt',1,$3,$4)",
    )
    .bind(fixture.document_id)
    .bind(fixture.version_id)
    .bind(&file_hash)
    .bind(&fixture.object_ref)
    .execute(pool)
    .await
    .unwrap();
    for (chunk_id, chunk_type, content) in [
        (
            fixture.trusted_chunk_id,
            "text",
            "selection contract requirement trusted",
        ),
        (
            fixture.derived_chunk_id,
            "question",
            "selection contract requirement derived",
        ),
        (
            fixture.filtered_chunk_id,
            "parent_text",
            "unrelated filtered bytes",
        ),
    ] {
        sqlx::query(
            "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content)
             VALUES($1,$2,$3,$4,$5)",
        )
        .bind(chunk_id)
        .bind(fixture.version_id)
        .bind(fixture.document_id)
        .bind(chunk_type)
        .bind(content)
        .execute(pool)
        .await
        .unwrap();
    }
    fixture
}

fn retrieval_policy() -> RetrievalPolicyIdentityV1 {
    RetrievalPolicyIdentityV1 {
        contract_version: "knowledge-evidence-v1".into(),
        policy_sha256: domain::sha256_hex(b"knowledge-evidence-v1:selection-contract-test"),
        max_hits: 4,
        max_chunk_bytes: 262_144,
        max_total_bytes: 1_048_576,
    }
}

async fn seed_selection_fixture(pool: &PgPool) -> SelectionFixture {
    let product_workspace_id = Uuid::new_v4();
    let company_workspace_id = storage::ensure_company_workspace(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'selection test',$2,'product_line')",
    )
    .bind(product_workspace_id)
    .bind(format!("selection-test-{product_workspace_id}"))
    .execute(pool)
    .await
    .unwrap();

    let mut product_ids = Vec::new();
    let mut product_version_ids = Vec::new();
    for ordinal in 0..2 {
        let product_id = Uuid::new_v4();
        let version_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO products(id,workspace_id,kind,name,slug)
             VALUES($1,$2,'product',$3,$4)",
        )
        .bind(product_id)
        .bind(product_workspace_id)
        .bind(format!("selection product {ordinal}"))
        .bind(format!("selection-product-{product_id}"))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO product_versions(id,product_id,label,status)
             VALUES($1,$2,'v1','active')",
        )
        .bind(version_id)
        .bind(product_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
            .bind(product_id)
            .bind(version_id)
            .execute(pool)
            .await
            .unwrap();
        product_ids.push(product_id);
        product_version_ids.push(version_id);
    }

    let noncurrent_version_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status)
         VALUES($1,$2,'noncurrent','active')",
    )
    .bind(noncurrent_version_id)
    .bind(product_ids[0])
    .execute(pool)
    .await
    .unwrap();

    let company_product_id = Uuid::new_v4();
    let company_version_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug)
         VALUES($1,$2,'library','selection company',$3)",
    )
    .bind(company_product_id)
    .bind(company_workspace_id)
    .bind(format!("selection-company-{company_product_id}"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status)
         VALUES($1,$2,'v1','active')",
    )
    .bind(company_version_id)
    .bind(company_product_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(company_product_id)
        .bind(company_version_id)
        .execute(pool)
        .await
        .unwrap();

    SelectionFixture {
        product_workspace_id,
        product_ids,
        product_version_ids,
        noncurrent_version_id,
        company_product_id,
        company_version_id,
    }
}

async fn remove_selection_fixture(pool: &PgPool, fixture: &SelectionFixture) {
    let mut product_ids = fixture.product_ids.clone();
    product_ids.push(fixture.company_product_id);
    let mut version_ids = fixture.product_version_ids.clone();
    version_ids.push(fixture.noncurrent_version_id);
    version_ids.push(fixture.company_version_id);
    sqlx::query("UPDATE products SET current_version_id=NULL WHERE id=ANY($1)")
        .bind(&product_ids)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM product_versions WHERE id=ANY($1)")
        .bind(&version_ids)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM products WHERE id=ANY($1)")
        .bind(&product_ids)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workspaces WHERE id=$1")
        .bind(fixture.product_workspace_id)
        .execute(pool)
        .await
        .unwrap();
}

fn product_request(version_ids: Vec<Uuid>) -> ProductEvidenceRequestV1 {
    ProductEvidenceRequestV1 {
        schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
        requirement_identity_sha256: domain::sha256_hex(b"selection contract requirement"),
        requirement_text: "selection contract requirement".into(),
        product_version_ids: version_ids,
        retrieval_policy: retrieval_policy(),
    }
}

fn company_request(version_ids: Vec<Uuid>) -> CompanyEvidenceRequestV1 {
    CompanyEvidenceRequestV1 {
        schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
        requirement_identity_sha256: domain::sha256_hex(b"selection contract requirement"),
        requirement_text: "selection contract requirement".into(),
        library_version_ids: version_ids,
        retrieval_policy: retrieval_policy(),
    }
}

fn frozen_product(product_id: Uuid, version_id: Uuid, kind: &str) -> Value {
    json!({
        "id": Uuid::new_v4(),
        "product_id": product_id,
        "product_version_id": version_id,
        "workspace_kind": kind,
        "frozen_display_name": version_id.to_string(),
        "identity_sha256": domain::sha256_hex(
            format!("ProductVersionEvidenceV1:{product_id}:{version_id}:{kind}").as_bytes()
        )
    })
}

async fn attest(pool: &PgPool, scope: &Value) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_knowledge_attest_matching_scope_v1($1)")
        .bind(scope)
        .fetch_one(pool)
        .await
}

fn assert_database_error(error: sqlx::Error, expected: &str) {
    let message = error
        .as_database_error()
        .map(|error| error.message().to_string())
        .unwrap_or_else(|| error.to_string());
    assert!(
        message.contains(expected),
        "expected {expected}, got {message}"
    );
}

#[tokio::test]
async fn adapter_enforces_exact_unique_current_workspace_kind_selection() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeSelectionAdapter").await else {
        return;
    };
    let fixture = seed_selection_fixture(&pool).await;
    let adapter = PostgresKnowledgeRetrievalAdapter::new(pool.clone());
    let selected = fixture.product_version_ids[0];

    let subset = adapter
        .retrieve_product_evidence(product_request(vec![selected]))
        .await;
    let company_subset = adapter
        .retrieve_company_evidence(company_request(vec![fixture.company_version_id]))
        .await;
    let duplicate = adapter
        .retrieve_product_evidence(product_request(vec![selected, selected]))
        .await;
    let missing = adapter
        .retrieve_product_evidence(product_request(vec![Uuid::new_v4()]))
        .await;
    let noncurrent = adapter
        .retrieve_product_evidence(product_request(vec![fixture.noncurrent_version_id]))
        .await;
    let wrong_product_kind = adapter
        .retrieve_product_evidence(product_request(vec![fixture.company_version_id]))
        .await;
    let wrong_company_kind = adapter
        .retrieve_company_evidence(company_request(vec![selected]))
        .await;
    remove_selection_fixture(&pool, &fixture).await;

    let subset = subset.unwrap();
    assert_eq!(subset.eligible_versions.len(), 1);
    assert_eq!(subset.eligible_versions[0].product_version_id, selected);
    let company_subset = company_subset.unwrap();
    assert_eq!(company_subset.eligible_versions.len(), 1);
    assert_eq!(
        company_subset.eligible_versions[0].product_version_id,
        fixture.company_version_id
    );
    for result in [duplicate, missing, noncurrent, wrong_product_kind] {
        assert!(matches!(
            result,
            Err(KnowledgeRetrievalError::InvalidRequest(_))
        ));
    }
    assert!(matches!(
        wrong_company_kind,
        Err(KnowledgeRetrievalError::InvalidRequest(_))
    ));
}

#[tokio::test]
async fn populated_v1_adapter_output_bytes_and_behavior_are_frozen() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeSelectionV1ByteGolden").await
    else {
        return;
    };
    let fixture = seed_v1_golden_fixture(&pool).await;
    let adapter = PostgresKnowledgeRetrievalAdapter::new(pool.clone());
    let batch = adapter
        .retrieve_product_evidence(product_request(vec![fixture.version_id]))
        .await
        .unwrap();

    let serialized = serde_json::to_string(&batch).unwrap();
    let expected = r#"{"schema_version":1,"eligible_versions":[{"product_id":"d1000000-0000-4000-8000-000000000002","product_version_id":"d1000000-0000-4000-8000-000000000003","workspace_kind":"product_line","frozen_display_name":"d1000000-0000-4000-8000-000000000003"}],"hits":[{"schema_version":1,"document_id":"d1000000-0000-4000-8000-000000000004","source_chunk_id":"d1000000-0000-4000-8000-000000000005","product_id":"d1000000-0000-4000-8000-000000000002","product_version_id":"d1000000-0000-4000-8000-000000000003","workspace_kind":"product_line","frozen_document_display_name":"v1-golden.txt","chunk_utf8":"selection contract requirement trusted","chunk_sha256":"485acdf0997030c13cf75504ec98b5a46205f13f4d40077110b057d92d99f91a","chunk_byte_length":38,"quote_start_offset":0,"quote_end_offset":38,"offset_unit":"utf8_byte","retrieval_rank":1,"retrieval_raw_score":"1.000000","retrieval_contract_version":"knowledge-evidence-v1"},{"schema_version":1,"document_id":"d1000000-0000-4000-8000-000000000004","source_chunk_id":"d1000000-0000-4000-8000-000000000006","product_id":"d1000000-0000-4000-8000-000000000002","product_version_id":"d1000000-0000-4000-8000-000000000003","workspace_kind":"product_line","frozen_document_display_name":"v1-golden.txt","chunk_utf8":"selection contract requirement derived","chunk_sha256":"bd79bb21b33035dbcbb87551ba46f7573cc84ebd5fe983473bc22ced364c0052","chunk_byte_length":38,"quote_start_offset":0,"quote_end_offset":38,"offset_unit":"utf8_byte","retrieval_rank":2,"retrieval_raw_score":"1.000000","retrieval_contract_version":"knowledge-evidence-v1"}]}"#;
    assert_eq!(serialized.as_bytes(), expected.as_bytes());

    let returned_chunk_ids = batch
        .hits
        .iter()
        .map(|hit| hit.source_chunk_id)
        .collect::<Vec<_>>();
    assert_eq!(
        returned_chunk_ids,
        vec![fixture.trusted_chunk_id, fixture.derived_chunk_id]
    );
    assert!(!returned_chunk_ids.contains(&fixture.filtered_chunk_id));
    let returned_source_types: Vec<String> =
        sqlx::query_scalar("SELECT chunk_type FROM chunks WHERE id=ANY($1::uuid[]) ORDER BY id")
            .bind(&returned_chunk_ids)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(returned_source_types, vec!["text", "question"]);

    remove_v1_golden_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn attestation_freezes_empty_all_or_nonempty_exact_per_kind_selection() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeSelectionAttestation").await
    else {
        return;
    };
    let final_schema: bool = sqlx::query_scalar(
        "SELECT position('version_selections' IN pg_get_functiondef(
             'kb_knowledge_attest_matching_scope_v1(jsonb)'::regprocedure))>0",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);
    if !support::require_final_schema("KnowledgeSelectionAttestation", final_schema) {
        return;
    }
    let fixture = seed_selection_fixture(&pool).await;
    let selected = fixture.product_version_ids[0];
    let selected_product = frozen_product(fixture.product_ids[0], selected, "product_line");
    let exact_scope = json!({
        "schema_version": 1,
        "workspace_kinds": ["product_line"],
        "version_selections": {"product_line": [selected], "company": []},
        "products": [selected_product],
        "frozen_hits": []
    });
    let exact = attest(&pool, &exact_scope).await;

    let duplicate_scope = json!({
        "schema_version": 1,
        "workspace_kinds": ["product_line"],
        "version_selections": {"product_line": [selected, selected], "company": []},
        "products": [frozen_product(fixture.product_ids[0], selected, "product_line")],
        "frozen_hits": []
    });
    let duplicate = attest(&pool, &duplicate_scope).await;
    let mut unsorted_versions = fixture.product_version_ids.clone();
    unsorted_versions.sort();
    unsorted_versions.reverse();
    let unsorted_scope = json!({
        "schema_version": 1,
        "workspace_kinds": ["product_line"],
        "version_selections": {"product_line": unsorted_versions, "company": []},
        "products": [],
        "frozen_hits": []
    });
    let unsorted = attest(&pool, &unsorted_scope).await;
    let missing_scope = json!({
        "schema_version": 1,
        "workspace_kinds": ["product_line"],
        "version_selections": {"product_line": [Uuid::new_v4()], "company": []},
        "products": [],
        "frozen_hits": []
    });
    let missing = attest(&pool, &missing_scope).await;
    let wrong_kind_scope = json!({
        "schema_version": 1,
        "workspace_kinds": ["product_line"],
        "version_selections": {"product_line": [fixture.company_version_id], "company": []},
        "products": [],
        "frozen_hits": []
    });
    let wrong_kind = attest(&pool, &wrong_kind_scope).await;
    let extra_product_scope = json!({
        "schema_version": 1,
        "workspace_kinds": ["product_line"],
        "version_selections": {"product_line": [selected], "company": []},
        "products": [
            frozen_product(fixture.product_ids[0], selected, "product_line"),
            frozen_product(
                fixture.product_ids[1],
                fixture.product_version_ids[1],
                "product_line"
            )
        ],
        "frozen_hits": []
    });
    let extra_product = attest(&pool, &extra_product_scope).await;

    let eligible_rows = sqlx::query(
        "SELECT product.id AS product_id,version_value.id AS product_version_id
           FROM workspaces workspace_value
           JOIN products product ON product.workspace_id=workspace_value.id
           JOIN product_versions version_value ON version_value.product_id=product.id
            AND product.current_version_id=version_value.id
          WHERE workspace_value.kind='product_line' AND product.kind='product'
            AND version_value.status='active' AND version_value.deleted_at IS NULL
          ORDER BY product.id,version_value.id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let all_products = eligible_rows
        .iter()
        .map(|row| {
            frozen_product(
                row.get("product_id"),
                row.get("product_version_id"),
                "product_line",
            )
        })
        .collect::<Vec<_>>();
    let all_scope = json!({
        "schema_version": 1,
        "workspace_kinds": ["product_line"],
        "version_selections": {"product_line": [], "company": []},
        "products": all_products,
        "frozen_hits": []
    });
    let all = attest(&pool, &all_scope).await;
    remove_selection_fixture(&pool, &fixture).await;

    exact.unwrap();
    all.unwrap();
    assert_database_error(
        duplicate.unwrap_err(),
        "KNOWLEDGE_MATCHING_SCOPE_V1_INVALID",
    );
    assert_database_error(unsorted.unwrap_err(), "KNOWLEDGE_MATCHING_SCOPE_V1_INVALID");
    assert_database_error(missing.unwrap_err(), "KNOWLEDGE_MATCHING_SCOPE_V1_MISMATCH");
    assert_database_error(
        wrong_kind.unwrap_err(),
        "KNOWLEDGE_MATCHING_SCOPE_V1_MISMATCH",
    );
    assert_database_error(
        extra_product.unwrap_err(),
        "KNOWLEDGE_MATCHING_SCOPE_V1_MISMATCH",
    );
}
