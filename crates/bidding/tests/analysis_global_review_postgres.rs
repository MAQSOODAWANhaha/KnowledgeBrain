//! Rust/SQL agreement for frozen v2 global-review evidence.
//! These tests never call a model or use a business database.
#[allow(dead_code)]
mod support;

use bidding::tender_analysis::{
    Analysis, Coverage, Finding, FrozenInput, Record, RecordData, Review, Source, Span, digest,
    rule_contract::{self, ANALYSIS_GLOBAL_CHECK_KEYS, GlobalCheck, GlobalCheckConclusion},
};
use serde_json::json;
use sqlx::PgPool;

fn fixture() -> (FrozenInput, Analysis, Review) {
    let source = Source {
        source_unit_revision_id: "source".into(),
        document_id: "document".into(),
        text: "项目名称：网络安全服务。".into(),
        locator: json!({}),
        ordinal: 0,
    };
    let span = Span {
        source_id: source.source_unit_revision_id.clone(),
        start: 0,
        end: source.text.len(),
        view_id: None,
        grid_cell: None,
    };
    let input = FrozenInput {
        schema_version: 1,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        decisions: vec![],
        structured_forms: vec![],
        source_units: vec![source],
    };
    let record = Record {
        id: "fact".into(),
        sources: vec![span.clone()],
        data: RecordData::Fact {
            name: "项目名称".into(),
            value: "网络安全服务".into(),
            scope: "本项目".into(),
        },
    };
    let mut analysis = Analysis::default();
    analysis.records.insert(record.id.clone(), record.clone());
    let mut coverage = Coverage::default();
    coverage
        .text
        .insert(span.source_id.clone(), vec![(span.start, span.end)]);
    coverage
        .candidate
        .insert(format!("record:{}", record.id), digest(&record).unwrap());
    let mut review = Review {
        analysis_sha256: digest(&analysis).unwrap(),
        coverage,
        findings: vec![],
        contract_sha256: rule_contract::contract_sha256().unwrap(),
        global_checks: ANALYSIS_GLOBAL_CHECK_KEYS
            .iter()
            .map(|key| GlobalCheck {
                key: (*key).into(),
                scope_sha256: rule_contract::scope_sha256(&input, &analysis).unwrap(),
                conclusion: GlobalCheckConclusion::Pass,
                grounds: vec![span.clone()],
                record_ids: vec![record.id.clone()],
                finding_ids: vec![],
            })
            .collect(),
        omitted_sources: Default::default(),
        draft: false,
    };
    bind_checks(&mut analysis, &mut review);
    (input, analysis, review)
}

fn bind_checks(analysis: &mut Analysis, review: &mut Review) {
    analysis.review_global_checks = review
        .global_checks
        .iter()
        .map(|c| (c.key.clone(), c.clone()))
        .collect();
    review.analysis_sha256 = digest(analysis).unwrap();
}

async fn agree(
    pool: &PgPool,
    label: &str,
    input: &FrozenInput,
    analysis: &Analysis,
    review: &Review,
    expected: bool,
) {
    let rust = rule_contract::validate_review(input, analysis, review).is_ok();
    assert_eq!(rust, expected, "Rust: {label}");
    let sql: bool = sqlx::query_scalar("SELECT kb_bid_v2_analysis_global_review_valid($1,$2,$3)")
        .bind(json!(input))
        .bind(json!(analysis))
        .bind(json!(review))
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| panic!("SQL {label}: {error}"));
    assert_eq!(sql, expected, "SQL: {label}");
}

#[tokio::test]
#[ignore = "requires KNOWLEDGEBRAIN_TEST_DATABASE_URL for an isolated fresh baseline"]
async fn global_review_sql_rejects_forged_evidence_and_matches_rust() {
    let pool = support::connect_postgres_contract("global review")
        .await
        .expect("explicit isolated database required");
    let (input, analysis, review) = fixture();
    agree(&pool, "valid", &input, &analysis, &review, true).await;
    for field in ["record_ids", "finding_ids"] {
        let mut raw_review = json!(review);
        raw_review["global_checks"][0][field] = json!([7]);
        let mut raw_analysis = json!(analysis);
        let key = raw_review["global_checks"][0]["key"]
            .as_str()
            .unwrap()
            .to_owned();
        raw_analysis["review_global_checks"][&key] = raw_review["global_checks"][0].clone();
        raw_review["analysis_sha256"] = json!(digest(&raw_analysis).unwrap());
        assert!(serde_json::from_value::<Review>(raw_review.clone()).is_err());
        let accepted: bool =
            sqlx::query_scalar("SELECT kb_bid_v2_analysis_global_review_valid($1,$2,$3)")
                .bind(json!(input))
                .bind(raw_analysis)
                .bind(raw_review)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            !accepted,
            "numeric {field} must not be coerced into a string"
        );
    }
    for case in [
        "missing_key",
        "duplicate_key",
        "stale_scope",
        "wrong_contract",
        "wrong_analysis",
        "unread_ground",
        "invalid_utf8",
        "unknown_source",
        "old_candidate",
        "unknown_record",
        "fake_finding",
        "unsupported_source_limited",
    ] {
        let mut changed = review.clone();
        match case {
            "missing_key" => {
                changed.global_checks.pop();
            }
            "duplicate_key" => {
                changed.global_checks[1] = changed.global_checks[0].clone();
            }
            "stale_scope" => changed.global_checks[0].scope_sha256 = "a".repeat(64),
            "wrong_contract" => changed.contract_sha256 = "b".repeat(64),
            "wrong_analysis" => changed.analysis_sha256 = "c".repeat(64),
            "unread_ground" => changed.coverage.text.clear(),
            "invalid_utf8" => changed.global_checks[0].grounds[0].start = 1,
            "unknown_source" => changed.global_checks[0].grounds[0].source_id = "unknown".into(),
            "old_candidate" => {
                changed
                    .coverage
                    .candidate
                    .insert("record:fact".into(), "d".repeat(64));
            }
            "unknown_record" => changed.global_checks[0].record_ids = vec!["unknown".into()],
            "fake_finding" => {
                changed.global_checks[0].conclusion = GlobalCheckConclusion::Findings;
                changed.global_checks[0].finding_ids = vec!["e".repeat(64)];
            }
            "unsupported_source_limited" => {
                changed.global_checks[0].conclusion = GlobalCheckConclusion::SourceLimited
            }
            _ => unreachable!(),
        }
        let mut changed_analysis = analysis.clone();
        let wrong_analysis = changed.analysis_sha256.clone();
        bind_checks(&mut changed_analysis, &mut changed);
        if case == "wrong_analysis" {
            changed.analysis_sha256 = wrong_analysis;
        }
        agree(&pool, case, &input, &changed_analysis, &changed, false).await;
    }
    let mut finding_review = review.clone();
    let finding = Finding {
        code: "inconsistency".into(),
        message: "原文存在待确认项".into(),
        correction: "核对原文".into(),
        affected: vec![],
        sources: review.global_checks[0].grounds.clone(),
    };
    finding_review.global_checks[0].conclusion = GlobalCheckConclusion::Findings;
    finding_review.global_checks[0].finding_ids = vec![digest(&finding).unwrap()];
    finding_review.findings.push(finding);
    let mut finding_analysis = analysis.clone();
    bind_checks(&mut finding_analysis, &mut finding_review);
    agree(
        &pool,
        "saved finding",
        &input,
        &finding_analysis,
        &finding_review,
        true,
    )
    .await;
    finding_review.findings[0].correction.push_str(" changed");
    agree(
        &pool,
        "changed finding",
        &input,
        &finding_analysis,
        &finding_review,
        false,
    )
    .await;

    let mut limited_input = input.clone();
    limited_input
        .documents
        .push(json!({"document_id":"missing","disposition":"unresolved"}));
    let mut limited_review = review.clone();
    for check in &mut limited_review.global_checks {
        check.scope_sha256 = rule_contract::scope_sha256(&limited_input, &analysis).unwrap();
    }
    limited_review.global_checks[0].grounds.clear();
    limited_review.global_checks[0].conclusion = GlobalCheckConclusion::SourceLimited;
    let mut limited_analysis = analysis.clone();
    bind_checks(&mut limited_analysis, &mut limited_review);
    agree(
        &pool,
        "unread missing-document inventory",
        &limited_input,
        &limited_analysis,
        &limited_review,
        false,
    )
    .await;
    limited_review
        .coverage
        .metadata
        .insert("documents".into(), vec![(0, 1)]);
    agree(
        &pool,
        "read missing-document inventory",
        &limited_input,
        &limited_analysis,
        &limited_review,
        true,
    )
    .await;

    // A legacy Review still deserializes independently; it must not satisfy v2.
    let legacy: Review = serde_json::from_value(json!({
        "analysis_sha256":review.analysis_sha256,"coverage":review.coverage,"findings":[]
    }))
    .unwrap();
    agree(
        &pool,
        "legacy v1 is not v2 evidence",
        &input,
        &analysis,
        &legacy,
        false,
    )
    .await;
    pool.close().await;
}
