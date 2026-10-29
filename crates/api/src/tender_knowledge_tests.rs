//! Real fresh DDL and synthetic resources in an isolated test schema.
use super::*;
use axum::body::to_bytes;
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
#[ignore = "requires an explicitly selected disposable PostgreSQL database"]
async fn persisted_job_reads_only_its_owners_selected_live_versions_and_documents() {
    let dsn = std::env::var("DATABASE_URL").expect("explicit disposable PostgreSQL URL");
    let admin = PgPool::connect(&dsn).await.unwrap();
    sqlx::raw_sql("CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public; CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;")
        .execute(&admin).await.unwrap();
    let schema = format!("kb_worker_knowledge_{}", Uuid::new_v4().simple());
    // Identifier contains a fixed prefix and a host-generated hexadecimal UUID.
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    let path = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let path = path.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('search_path',$1,false)")
                    .bind(format!("{path},public,pg_catalog"))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&dsn)
        .await
        .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    for migration in [
        include_str!("../../../migrations/20261028000001_shared_platform.sql"),
        include_str!("../../../migrations/20261028000002_knowledge_base.sql"),
        include_str!("../../../migrations/20261028000003_tender.sql"),
    ] {
        // Scope the real initialization guard and DDL to this empty namespace;
        // extension objects remain shared in public and are never removed here.
        let sql = migration
            .replace(
                "SET LOCAL search_path = public, pg_catalog;",
                &format!("SET LOCAL search_path = {schema},public,pg_catalog;"),
            )
            .replace("n.nspname = 'public'", &format!("n.nspname = '{schema}'"))
            .replace(
                "to_regclass('public._sqlx_migrations')",
                &format!("to_regclass('{schema}._sqlx_migrations')"),
            );
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
    let owner = Uuid::new_v4();
    let other = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let foreign_workspace = Uuid::new_v4();
    let product = Uuid::new_v4();
    let foreign_product = Uuid::new_v4();
    let version = Uuid::new_v4();
    let current = Uuid::new_v4();
    let foreign_version = Uuid::new_v4();
    let document = Uuid::new_v4();
    let foreign_document = Uuid::new_v4();
    let project = Uuid::new_v4();
    let chosen = Selection {
        product_id: product,
        version_id: version,
    };
    let foreign = Selection {
        product_id: foreign_product,
        version_id: foreign_version,
    };
    for user in [owner, other] {
        sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
            .bind(user)
            .bind(user.to_string())
            .execute(&pool)
            .await
            .unwrap();
    }
    for (id, user) in [(workspace, owner), (foreign_workspace, other)] {
        sqlx::query("INSERT INTO knowledge_workspaces(id,owner_user_id,name,slug) VALUES($1,$2,'fixture',$3)")
            .bind(id).bind(user).bind(id.to_string()).execute(&pool).await.unwrap();
    }
    for (id, parent) in [(product, workspace), (foreign_product, foreign_workspace)] {
        sqlx::query("INSERT INTO knowledge_products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','fixture',$3)")
            .bind(id).bind(parent).bind(id.to_string()).execute(&pool).await.unwrap();
    }
    for (id, parent) in [
        (version, product),
        (current, product),
        (foreign_version, foreign_product),
    ] {
        sqlx::query("INSERT INTO knowledge_product_versions(id,product_id,label,status,indexing_strategy) VALUES($1,$2,$3,'active','{\"vector\":false,\"keyword\":true}'::jsonb)")
            .bind(id).bind(parent).bind(id.to_string()).execute(&pool).await.unwrap();
    }
    // A new current pointer cannot replace this job's explicitly selected version.
    sqlx::query("UPDATE knowledge_products SET current_version_id=$1 WHERE id=$2")
        .bind(current)
        .bind(product)
        .execute(&pool)
        .await
        .unwrap();
    let hash = "a".repeat(64);
    let object = format!("objects/{hash}");
    sqlx::query("INSERT INTO platform_objects(object_ref,content_sha256,media_type,byte_length,state) VALUES($1,$2,'application/pdf',12,'available')")
        .bind(&object).bind(&hash).execute(&pool).await.unwrap();
    for (id, parent) in [(document, version), (foreign_document, foreign_version)] {
        sqlx::query("INSERT INTO knowledge_documents(id,product_version_id,title,parse_status,enable_status,index_ready,file_name,byte_length,content_sha256,object_ref) VALUES($1,$2,'fixture','completed','enabled',true,'fixture.pdf',12,$3,$4)")
            .bind(id).bind(parent).bind(&hash).bind(&object).execute(&pool).await.unwrap();
    }
    sqlx::query(
        "INSERT INTO tender_projects(id,owner_user_id,title,status) VALUES($1,$2,'fixture','open')",
    )
    .bind(project)
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO tender_analysis_runs(run_id,request_key) VALUES('run','run')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tender_project_analyses(run_id,project_id,request_key) VALUES('run',$1,'run')",
    )
    .bind(project)
    .execute(&pool)
    .await
    .unwrap();
    let payload = json!({"owner_user_id":owner,"knowledge_selections":[{"product_id":product,"version_id":version}]});
    sqlx::query("INSERT INTO tender_authoring_jobs(execution_id,project_id,run_id,payload) VALUES('job',$1,'run',$2)")
        .bind(project).bind(&payload).execute(&pool).await.unwrap();
    require_selection(&pool, owner, &chosen).await.unwrap();
    assert_eq!(
        require_selection(&pool, other, &chosen)
            .await
            .unwrap_err()
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        require_selection(&pool, owner, &foreign)
            .await
            .unwrap_err()
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        require_selection(
            &pool,
            owner,
            &Selection {
                product_id: product,
                version_id: foreign_version
            }
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::NOT_FOUND
    );
    let metadata = run_command(
        &pool,
        "job",
        Command::Document {
            product_id: product,
            version_id: version,
            document_id: document,
        },
    )
    .await
    .unwrap();
    let value: Value =
        serde_json::from_slice(&to_bytes(metadata.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(value["product_version_id"], version.to_string());
    assert_eq!(value["object_ref"], object);
    for command in [
        Command::Document {
            product_id: foreign_product,
            version_id: foreign_version,
            document_id: foreign_document,
        },
        Command::Document {
            product_id: product,
            version_id: current,
            document_id: document,
        },
        Command::Document {
            product_id: product,
            version_id: version,
            document_id: foreign_document,
        },
        Command::File {
            product_id: product,
            version_id: version,
            key: format!("objects/{}", "b".repeat(64)),
        },
    ] {
        assert_eq!(
            run_command(&pool, "job", command).await.unwrap_err().0,
            StatusCode::NOT_FOUND
        );
    }
    sqlx::query("UPDATE knowledge_documents SET index_ready=false WHERE id=$1")
        .bind(document)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        require_document(&pool, &chosen, document)
            .await
            .unwrap_err()
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE knowledge_product_versions SET status='archived' WHERE id=$1")
        .bind(version)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        require_selection(&pool, owner, &chosen)
            .await
            .unwrap_err()
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE knowledge_product_versions SET status='active' WHERE id=$1")
        .bind(version)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE tender_projects SET owner_user_id=$1 WHERE id=$2")
        .bind(other)
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        job_scope(&pool, "job").await.unwrap_err().0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE tender_projects SET owner_user_id=$1 WHERE id=$2")
        .bind(owner)
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE tender_authoring_jobs SET status='cancelled' WHERE execution_id='job'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        job_scope(&pool, "job").await.unwrap_err().0,
        StatusCode::NOT_FOUND
    );
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
