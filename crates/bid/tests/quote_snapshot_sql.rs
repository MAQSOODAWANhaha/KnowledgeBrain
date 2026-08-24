//! Cross-check Rust and SQL QuoteSnapshotV1 canonical bytes when a V1 database is present.

use bid::quote::{
    CeilingBasis, CeilingIdentity, QuoteLineInput, QuoteSnapshotV1, TaxMode,
    build_quote_snapshot_v1, compute_line, sha256_hex,
};
use serde_json::json;
use sqlx::PgPool;
use storage::{
    bid_quote::{FinalizeQuote, UpsertQuoteLine},
    bidding::MutationContext,
};
use uuid::Uuid;

mod support;

struct QuoteCasSeed {
    project_id: Uuid,
    actor: String,
    fact_revision: i64,
    ceiling_revision: i64,
    ceiling_sha256: String,
    pricing_revision: i64,
    pricing_sha256: String,
}

fn mutation_context(actor: &str, operation: &str) -> MutationContext {
    let request = json!({"operation": operation, "nonce": Uuid::new_v4()});
    MutationContext::new(
        actor,
        format!("quote-sql-{operation}-{}", Uuid::new_v4()),
        &request,
    )
    .unwrap()
}

fn assert_database_error(error: sqlx::Error, expected: &str) {
    let message = error
        .as_database_error()
        .map(|error| error.message().to_string())
        .unwrap_or_else(|| error.to_string());
    assert!(
        message.contains(expected),
        "expected database error {expected}, got {message}"
    );
}

async fn seed_quote_cas(pool: &PgPool) -> QuoteCasSeed {
    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let actor = format!("user:{user_id}");
    let fact_revision = 3;
    let ceiling_revision = 5;
    let ceiling_sha256 = "c".repeat(64);
    let pricing_revision = 7;
    let pricing_sha256 = "b".repeat(64);
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(user_id)
        .bind(format!("{user_id}@quote-cas.invalid"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects
         (id,title,owner_user_id,ends_at,fact_revision,fact_sha256,ceiling_price,
          ceiling_currency,ceiling_basis,ceiling_revision,ceiling_identity_sha256,created_by)
         VALUES($1,'Quote CAS contract',$2,clock_timestamp()+interval '30 days',$3,
          repeat('f',64),200.00,'CNY','tax_inclusive',$4,$5,$6)",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(fact_revision)
    .bind(ceiling_revision)
    .bind(&ceiling_sha256)
    .bind(&actor)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_clause_set_identities(project_id,set_kind,revision,content_sha256,updated_at)
         VALUES($1,'pricing',$2,$3,clock_timestamp())",
    )
    .bind(project_id)
    .bind(pricing_revision)
    .bind(&pricing_sha256)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    QuoteCasSeed {
        project_id,
        actor,
        fact_revision,
        ceiling_revision,
        ceiling_sha256,
        pricing_revision,
        pricing_sha256,
    }
}

#[allow(clippy::too_many_arguments)]
async fn finalize_attempt(
    pool: &PgPool,
    seed: &QuoteCasSeed,
    edit_version: i64,
    fact_revision: i64,
    ceiling_revision: i64,
    ceiling_sha256: &str,
    pricing_revision: i64,
    pricing_sha256: &str,
) -> Result<serde_json::Value, sqlx::Error> {
    let context = mutation_context(&seed.actor, "finalize");
    storage::bid_quote::finalize_quote(
        pool,
        FinalizeQuote {
            project_id: seed.project_id,
            expected_edit_version: edit_version,
            expected_fact_revision: fact_revision,
            expected_ceiling_revision: ceiling_revision,
            expected_ceiling_identity_sha256: ceiling_sha256,
            expected_pricing_revision: pricing_revision,
            expected_pricing_set_sha256: pricing_sha256,
            no_ceiling_reviewed: false,
            no_ceiling_reason: None,
        },
        &context,
    )
    .await
}

async fn reopen_attempt(
    pool: &PgPool,
    seed: &QuoteCasSeed,
    snapshot_id: Uuid,
    fact_revision: i64,
    pricing_revision: i64,
) -> Result<serde_json::Value, sqlx::Error> {
    let context = mutation_context(&seed.actor, "reopen");
    storage::bid_quote::reopen_quote(
        pool,
        seed.project_id,
        snapshot_id,
        fact_revision,
        pricing_revision,
        &context,
    )
    .await
}

#[tokio::test]
async fn rust_and_sql_canonical_bytes_match() {
    let Some(pool) = support::connect_postgres_contract("QuoteSnapshotV1").await else {
        return;
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure(
          'kb_bid_build_quote_snapshot_v1(uuid,uuid,bigint,text,text,text,jsonb,numeric,numeric,numeric,jsonb,jsonb,bigint,bigint,kb_sha256)'
         ) IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe final QuoteSnapshotV1 schema");
    if !support::require_final_schema("QuoteSnapshotV1", ready) {
        return;
    }

    let quote_id = Uuid::from_u128(0x11111111_1111_1111_1111_111111111111);
    let project_id = Uuid::from_u128(0x22222222_2222_2222_2222_222222222222);
    let line = compute_line(
        &QuoteLineInput {
            id: Uuid::from_u128(0x33333333_3333_3333_3333_333333333333),
            ordinal: 0,
            description: "含\"引号\"与\\反斜杠".into(),
            pricing_mode: bid::quote::PricingMode::LumpSum,
            quantity: None,
            unit: None,
            unit_price: None,
            entered_amount: Some("100.00".into()),
            tax_rate: "0.130000".into(),
            user_confirmed: true,
        },
        TaxMode::TaxExclusive,
    )
    .unwrap();
    let snapshot = QuoteSnapshotV1 {
        quote_id,
        project_id,
        revision: 1,
        currency_code: "CNY".into(),
        currency_scale: 2,
        tax_mode: TaxMode::TaxExclusive,
        title: "投标报价一览表".into(),
        notes: Some("备注\"A\"\\B".into()),
        lines: vec![line.clone()],
        net_total: "100.00".into(),
        tax_total: "13.00".into(),
        gross_total: "113.00".into(),
        ceiling: Some(CeilingIdentity {
            amount: "1000000.00".into(),
            currency_code: "CNY".into(),
            basis: CeilingBasis::TaxInclusive,
            ceiling_revision: 3,
            ceiling_identity_sha256: "aa".repeat(32),
        }),
        no_ceiling_review: None,
        fact_revision: 4,
        pricing_revision: 2,
        pricing_set_sha256: "bb".repeat(32),
    };
    let rust_bytes = build_quote_snapshot_v1(&snapshot).unwrap();
    let lines = json!([{
        "id": line.id,
        "ordinal": line.ordinal,
        "description": line.description,
        "pricing_mode": "lump_sum",
        "quantity": null,
        "unit": null,
        "unit_price": null,
        "entered_amount": "100.00",
        "tax_rate": "0.130000",
        "basis_amount": "100.00",
        "net_amount": "100.00",
        "tax_amount": "13.00",
        "gross_amount": "113.00",
        "user_confirmed": true
    }]);
    let ceiling = json!({
        "amount": "1000000.00",
        "currency_code": "CNY",
        "basis": "tax_inclusive",
        "ceiling_revision": 3,
        "ceiling_identity_sha256": "aa".repeat(32)
    });
    let sql_bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT kb_bid_build_quote_snapshot_v1($1,$2,1,'tax_exclusive',$3,$4,$5,100.00,13.00,113.00,$6,NULL,4,2,$7)",
    )
    .bind(quote_id)
    .bind(project_id)
    .bind("投标报价一览表")
    .bind("备注\"A\"\\B")
    .bind(lines)
    .bind(ceiling)
    .bind("bb".repeat(32))
    .fetch_one(&pool)
    .await
    .expect("sql snapshot builder");
    assert_eq!(
        String::from_utf8_lossy(&sql_bytes),
        String::from_utf8_lossy(&rust_bytes)
    );
    assert_eq!(sha256_hex(&sql_bytes), sha256_hex(&rust_bytes));
}

#[tokio::test]
async fn partial_unit_price_fields_persist_until_the_line_is_complete() {
    let Some(pool) = support::connect_postgres_contract("partial unit-price quote edit").await
    else {
        return;
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure(
          'kb_bid_upsert_quote_line(uuid,uuid,bigint,integer,text,text,text,text,text,text,text,boolean,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe final quote mutation schema");
    if !support::require_final_schema("partial unit-price quote edit", ready) {
        return;
    }

    let seed = seed_quote_cas(&pool).await;
    storage::bid_quote::create_quote_draft(
        &pool,
        seed.project_id,
        "tax_exclusive",
        "投标报价一览表",
        None,
        &mutation_context(&seed.actor, "create-partial-unit-price"),
    )
    .await
    .unwrap();

    let line_id = Uuid::new_v4();
    let response = storage::bid_quote::upsert_quote_line(
        &pool,
        UpsertQuoteLine {
            project_id: seed.project_id,
            line_id,
            expected_edit_version: 0,
            ordinal: 0,
            description: "防火墙设备",
            pricing_mode: "unit_price",
            quantity: Some("2.000000"),
            unit: None,
            unit_price: None,
            entered_amount: None,
            tax_rate: "0.130000",
            user_confirmed: false,
        },
        &mutation_context(&seed.actor, "enter-partial-unit-price"),
    )
    .await
    .unwrap();

    assert_eq!(response["edit_version"], json!(1));
    assert_eq!(response["complete"], json!(false));
    let state = storage::bid_quote::quote_state(&pool, seed.project_id)
        .await
        .unwrap();
    assert_eq!(state["lines"][0]["quantity"], json!("2.000000"));
    assert_eq!(state["lines"][0]["unit"], serde_json::Value::Null);
    assert_eq!(state["lines"][0]["unit_price"], serde_json::Value::Null);
    assert_eq!(state["lines"][0]["complete"], json!(false));
}

#[tokio::test]
async fn quote_finalize_enforces_all_three_ceiling_basis_cases_in_sql() {
    let Some(pool) = support::connect_postgres_contract("quote SQL ceiling basis matrix").await
    else {
        return;
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure(
          'kb_bid_finalize_quote(uuid,bigint,bigint,bigint,kb_sha256,bigint,kb_sha256,boolean,text,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe final quote mutation schema");
    if !support::require_final_schema("quote SQL ceiling basis matrix", ready) {
        return;
    }

    for (basis, amount, expected_error) in [
        ("tax_inclusive", "112.99", Some("QUOTE_CEILING_EXCEEDED")),
        ("tax_exclusive", "100.00", None),
        ("unspecified", "200.00", Some("CEILING_BASIS_UNSPECIFIED")),
    ] {
        let seed = seed_quote_cas(&pool).await;
        sqlx::query(
            "UPDATE bid_projects
                SET ceiling_price=CAST($2 AS numeric),ceiling_basis=$3
              WHERE id=$1",
        )
        .bind(seed.project_id)
        .bind(amount)
        .bind(basis)
        .execute(&pool)
        .await
        .unwrap();
        storage::bid_quote::create_quote_draft(
            &pool,
            seed.project_id,
            "tax_exclusive",
            "投标报价一览表",
            None,
            &mutation_context(&seed.actor, &format!("create-ceiling-{basis}")),
        )
        .await
        .unwrap();
        storage::bid_quote::upsert_quote_line(
            &pool,
            UpsertQuoteLine {
                project_id: seed.project_id,
                line_id: Uuid::new_v4(),
                expected_edit_version: 0,
                ordinal: 0,
                description: "实施服务",
                pricing_mode: "lump_sum",
                quantity: None,
                unit: None,
                unit_price: None,
                entered_amount: Some("100.00"),
                tax_rate: "0.130000",
                user_confirmed: true,
            },
            &mutation_context(&seed.actor, &format!("line-ceiling-{basis}")),
        )
        .await
        .unwrap();

        let result = finalize_attempt(
            &pool,
            &seed,
            1,
            seed.fact_revision,
            seed.ceiling_revision,
            &seed.ceiling_sha256,
            seed.pricing_revision,
            &seed.pricing_sha256,
        )
        .await;
        if let Some(expected_error) = expected_error {
            assert_database_error(result.unwrap_err(), expected_error);
        } else {
            let finalized = result.unwrap();
            assert_eq!(finalized["net_total"], json!("100.00"));
            assert_eq!(finalized["gross_total"], json!("113.00"));
            assert_eq!(finalized["eligibility"], json!("eligible"));
        }
    }
}

#[tokio::test]
async fn quote_finalize_and_reopen_enforce_all_cas_identities() {
    let Some(pool) = support::connect_postgres_contract("Quote finalize/reopen CAS").await else {
        return;
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure(
          'kb_bid_finalize_quote(uuid,bigint,bigint,bigint,kb_sha256,bigint,kb_sha256,boolean,text,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL
         AND to_regprocedure(
          'kb_bid_reopen_quote(uuid,uuid,bigint,bigint,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe final quote mutation schema");
    if !support::require_final_schema("Quote finalize/reopen CAS", ready) {
        return;
    }

    let seed = seed_quote_cas(&pool).await;
    let create_context = mutation_context(&seed.actor, "create");
    let created = storage::bid_quote::create_quote_draft(
        &pool,
        seed.project_id,
        "tax_inclusive",
        "投标报价一览表",
        Some("SQL CAS contract"),
        &create_context,
    )
    .await
    .unwrap();
    assert_eq!(created["edit_version"], json!(0));

    let line_id = Uuid::new_v4();
    let stale_edit_context = mutation_context(&seed.actor, "upsert-stale-edit");
    let error = storage::bid_quote::upsert_quote_line(
        &pool,
        UpsertQuoteLine {
            project_id: seed.project_id,
            line_id,
            expected_edit_version: 1,
            ordinal: 0,
            description: "防火墙设备",
            pricing_mode: "lump_sum",
            quantity: None,
            unit: None,
            unit_price: None,
            entered_amount: Some("100.00"),
            tax_rate: "0.130000",
            user_confirmed: true,
        },
        &stale_edit_context,
    )
    .await
    .unwrap_err();
    assert_database_error(error, "QUOTE_EDIT_VERSION_MISMATCH");

    let upsert_context = mutation_context(&seed.actor, "upsert-current-edit");
    let line = storage::bid_quote::upsert_quote_line(
        &pool,
        UpsertQuoteLine {
            project_id: seed.project_id,
            line_id,
            expected_edit_version: 0,
            ordinal: 0,
            description: "防火墙设备",
            pricing_mode: "lump_sum",
            quantity: None,
            unit: None,
            unit_price: None,
            entered_amount: Some("100.00"),
            tax_rate: "0.130000",
            user_confirmed: true,
        },
        &upsert_context,
    )
    .await
    .unwrap();
    assert_eq!(line["edit_version"], json!(1));
    assert_eq!(line["gross_amount"], json!("100.00"));

    let error = finalize_attempt(
        &pool,
        &seed,
        0,
        seed.fact_revision,
        seed.ceiling_revision,
        &seed.ceiling_sha256,
        seed.pricing_revision,
        &seed.pricing_sha256,
    )
    .await
    .unwrap_err();
    assert_database_error(error, "QUOTE_EDIT_VERSION_MISMATCH");

    let error = finalize_attempt(
        &pool,
        &seed,
        1,
        seed.fact_revision - 1,
        seed.ceiling_revision,
        &seed.ceiling_sha256,
        seed.pricing_revision,
        &seed.pricing_sha256,
    )
    .await
    .unwrap_err();
    assert_database_error(error, "FACT_REVISION_CAS_MISMATCH");

    let wrong_ceiling_sha256 = "d".repeat(64);
    for (revision, sha256) in [
        (seed.ceiling_revision - 1, seed.ceiling_sha256.as_str()),
        (seed.ceiling_revision, wrong_ceiling_sha256.as_str()),
    ] {
        let error = finalize_attempt(
            &pool,
            &seed,
            1,
            seed.fact_revision,
            revision,
            sha256,
            seed.pricing_revision,
            &seed.pricing_sha256,
        )
        .await
        .unwrap_err();
        assert_database_error(error, "CEILING_IDENTITY_CAS_MISMATCH");
    }

    let wrong_pricing_sha256 = "e".repeat(64);
    for (revision, sha256) in [
        (seed.pricing_revision - 1, seed.pricing_sha256.as_str()),
        (seed.pricing_revision, wrong_pricing_sha256.as_str()),
    ] {
        let error = finalize_attempt(
            &pool,
            &seed,
            1,
            seed.fact_revision,
            seed.ceiling_revision,
            &seed.ceiling_sha256,
            revision,
            sha256,
        )
        .await
        .unwrap_err();
        assert_database_error(error, "PRICING_IDENTITY_CAS_MISMATCH");
    }

    let finalized = finalize_attempt(
        &pool,
        &seed,
        1,
        seed.fact_revision,
        seed.ceiling_revision,
        &seed.ceiling_sha256,
        seed.pricing_revision,
        &seed.pricing_sha256,
    )
    .await
    .unwrap();
    let snapshot_id = Uuid::parse_str(finalized["snapshot_id"].as_str().unwrap()).unwrap();
    assert_eq!(finalized["eligibility"], json!("eligible"));
    assert_eq!(finalized["gross_total"], json!("100.00"));
    let finalized_state = storage::bid_quote::quote_state(&pool, seed.project_id)
        .await
        .unwrap();
    assert_eq!(finalized_state["pointer"], json!("finalized"));
    assert_eq!(finalized_state["snapshot_id"], json!(snapshot_id));
    let quote_markdown: String =
        sqlx::query_scalar("SELECT kb_bid_build_part_markdown($1,'6:quote')")
            .bind(seed.project_id)
            .fetch_one(&pool)
            .await
            .expect("render eligible quote part markdown");
    assert!(quote_markdown.contains("| 序号 | 说明 | 计价方式 |"));
    assert!(quote_markdown.contains("| 1 | 防火墙设备 | lump_sum |"));
    assert!(quote_markdown.contains("| 0.130000 | 88.50 | 11.50 | 100.00 |"));

    let error = reopen_attempt(
        &pool,
        &seed,
        Uuid::new_v4(),
        seed.fact_revision,
        seed.pricing_revision,
    )
    .await
    .unwrap_err();
    assert_database_error(error, "QUOTE_SNAPSHOT_CAS_MISMATCH");
    let error = reopen_attempt(
        &pool,
        &seed,
        snapshot_id,
        seed.fact_revision - 1,
        seed.pricing_revision,
    )
    .await
    .unwrap_err();
    assert_database_error(error, "FACT_REVISION_CAS_MISMATCH");
    let error = reopen_attempt(
        &pool,
        &seed,
        snapshot_id,
        seed.fact_revision,
        seed.pricing_revision - 1,
    )
    .await
    .unwrap_err();
    assert_database_error(error, "PRICING_IDENTITY_CAS_MISMATCH");

    let reopened = reopen_attempt(
        &pool,
        &seed,
        snapshot_id,
        seed.fact_revision,
        seed.pricing_revision,
    )
    .await
    .unwrap();
    assert_eq!(reopened["revision"], json!(2));
    assert_eq!(reopened["edit_version"], json!(0));
    assert_eq!(reopened["based_on_snapshot_id"], json!(snapshot_id));
    let reopened_state = storage::bid_quote::quote_state(&pool, seed.project_id)
        .await
        .unwrap();
    assert_eq!(reopened_state["pointer"], json!("draft"));
    assert_eq!(reopened_state["based_on_snapshot_id"], json!(snapshot_id));
    assert_eq!(
        reopened_state["lines"][0]["description"],
        json!("防火墙设备")
    );
    assert_eq!(
        reopened_state["lines"][0]["entered_amount"],
        json!("100.00")
    );
    assert_eq!(reopened_state["lines"][0]["user_confirmed"], json!(true));
    let eligibility: String =
        sqlx::query_scalar("SELECT eligibility FROM bid_quote_snapshots WHERE id=$1")
            .bind(snapshot_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(eligibility, "superseded");
}
