//! Admission must not hide an existing immutable job after knowledge is disabled.
use super::*;
use sqlx::postgres::PgPoolOptions;

#[test]
fn web_document_identity_matches_python_golden() {
    let project = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    for (key, digest) in [
        (
            "repeat-1",
            "de0e08ea7a97df492f08ad29cda78a85f8e9017b48b8bf92dd8e0aef8cd9e4d3",
        ),
        (
            "repeat/\"quoted\"\\key-1",
            "6db44703d9875ca8055170a4f0d1de741a3a1acb8f7516fb92b391152547b843",
        ),
    ] {
        assert_eq!(
            document_execution_id(project, key),
            format!("web-document-{digest}")
        );
    }
}

#[tokio::test]
#[ignore = "requires an explicitly selected disposable PostgreSQL database"]
async fn existing_project_job_replay_bypasses_live_knowledge_admission_only() {
    let dsn =
        std::env::var("TENDER_TEST_DATABASE_URL").expect("explicit disposable PostgreSQL URL");
    let admin = PgPool::connect(&dsn).await.unwrap();
    let schema = format!("kb_tender_replay_{}", Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    let path = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(move |connection, _| {
            let path = path.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('search_path',$1,false)")
                    .bind(path)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&dsn)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE tender_authoring_jobs(execution_id text PRIMARY KEY,project_id uuid NOT NULL)")
        .execute(&pool).await.unwrap();
    let project = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let body =
        json!({"knowledge_selections":[{"product_id":Uuid::new_v4(),"version_id":Uuid::new_v4()}]});
    sqlx::query("INSERT INTO tender_authoring_jobs(execution_id,project_id) VALUES($1,$2)")
        .bind(document_execution_id(project, "lost-response"))
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tender_authoring_jobs(execution_id,project_id) VALUES($1,$2)")
        .bind(document_execution_id(project, "foreign-mapping"))
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    let existing = admit_authoring_start(&pool, owner, project, "lost-response", &body).await;
    let new_request = admit_authoring_start(&pool, owner, project, "new-request", &body).await;
    let other_project =
        admit_authoring_start(&pool, owner, Uuid::new_v4(), "lost-response", &body).await;
    let foreign_mapping =
        admit_authoring_start(&pool, owner, project, "foreign-mapping", &body).await;
    // Cleanup precedes assertions so a reproduced failure leaves no schema.
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    assert!(
        existing.is_ok(),
        "an immutable job must reach Python request CAS before live knowledge admission: {existing:?}"
    );
    assert!(new_request.is_err());
    assert!(other_project.is_err());
    assert!(foreign_mapping.is_err());
}
