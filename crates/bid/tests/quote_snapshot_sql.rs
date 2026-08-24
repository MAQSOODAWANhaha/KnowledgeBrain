//! Cross-check Rust and SQL QuoteSnapshotV1 canonical bytes when a V1 database is present.

use bid::quote::{
    CeilingBasis, CeilingIdentity, QuoteLineInput, QuoteSnapshotV1, TaxMode,
    build_quote_snapshot_v1, compute_line, sha256_hex,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn rust_and_sql_canonical_bytes_match() {
    let pool = match storage::connect().await {
        Ok(pool) => pool,
        Err(error) if std::env::var_os("DATABASE_URL").is_some() => {
            panic!("connect live QuoteSnapshotV1 contract database: {error}")
        }
        Err(_) => return,
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure(
          'kb_bid_build_quote_snapshot_v1(uuid,uuid,bigint,text,text,text,jsonb,numeric,numeric,numeric,jsonb,jsonb,bigint,bigint,kb_sha256)'
         ) IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe final QuoteSnapshotV1 schema");
    if !ready {
        if std::env::var_os("DATABASE_URL").is_some() {
            panic!("final QuoteSnapshotV1 schema unavailable");
        }
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
