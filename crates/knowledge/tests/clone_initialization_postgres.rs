//! Clone-generation and atomic-initialization regressions. Isolated DB only.
use knowledge::{Chunk, workflow};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let url =
        std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").expect("isolated test database required");
    assert!(!url.contains(":15432/"), "refusing development database");
    PgPool::connect(&url).await.unwrap()
}

async fn seed(pool: &PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    let owner = Uuid::new_v4();
    knowledge::insert_user(pool, owner, &format!("{owner}@clone.test"), None)
        .await
        .unwrap();
    let workspace = knowledge::create_workspace_with_library(
        pool,
        owner,
        "Clone regression",
        &Uuid::new_v4().to_string(),
    )
    .await
    .unwrap();
    let source = Uuid::new_v4();
    let digest = platform::sha256_hex(source.as_bytes());
    knowledge::insert_document(
        pool,
        knowledge::NewDocument {
            id: source,
            product_version_id: workspace.library_version_id,
            title: "source",
            file_name: "source.txt",
            file_size: 8,
            file_hash: &digest,
            object_ref: &format!("objects/{digest}"),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE documents SET parse_status='processing' WHERE id=$1")
        .bind(source)
        .execute(pool)
        .await
        .unwrap();
    let target = Uuid::new_v4();
    knowledge::insert_version_cloning(
        pool,
        target,
        workspace.library_id,
        "clone",
        workspace.library_version_id,
    )
    .await
    .unwrap();
    (
        workspace.library_id,
        workspace.library_version_id,
        source,
        target,
    )
}

fn chunk(did: Uuid, vid: Uuid, content: &str) -> Chunk {
    Chunk {
        id: Uuid::new_v4(),
        document_id: did,
        product_version_id: vid,
        chunk_type: "text".into(),
        content: content.into(),
        context_header: String::new(),
        start_at: 0,
        end_at: content.len() as i32,
        parent_chunk_id: None,
        generated_questions: vec![],
        source_locator: None,
    }
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn keep_copies_only_active_generation_and_rebuilds_wiki() {
    let pool = pool().await;
    let (_, source_version, source, target_version) = seed(&pool).await;
    let old = chunk(source, source_version, "retained old text");
    workflow::stage_index(&pool, source, 1, std::slice::from_ref(&old), &[], true)
        .await
        .unwrap();
    sqlx::query("INSERT INTO graph_nodes(product_version_id,document_id,name,chunk_ids) VALUES($1,$2,'current',ARRAY[$3::uuid])")
        .bind(source_version).bind(source).bind(old.id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO graph_relations(product_version_id,document_id,node1,node2,rel_type) VALUES($1,$2,'current','current','old')")
        .bind(source_version).bind(source).execute(&pool).await.unwrap();
    workflow::publish_empty(&pool, source, 1).await.unwrap();
    workflow::retry_document(&pool, source).await.unwrap();
    sqlx::query("UPDATE documents SET parse_status='processing' WHERE id=$1")
        .bind(source)
        .execute(&pool)
        .await
        .unwrap();
    let current = chunk(source, source_version, "current text");
    let mut wiki = chunk(source, source_version, "old source-set wiki");
    wiki.chunk_type = "wiki_page".into();
    wiki.context_header = "source-wiki".into();
    workflow::stage_index(&pool, source, 2, &[current.clone(), wiki], &[], true)
        .await
        .unwrap();
    sqlx::query("INSERT INTO graph_nodes(product_version_id,document_id,name,chunk_ids) VALUES($1,$2,'current',ARRAY[$3::uuid])")
        .bind(source_version).bind(source).bind(current.id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO graph_relations(product_version_id,document_id,node1,node2,rel_type) VALUES($1,$2,'current','current','current')")
        .bind(source_version).bind(source).execute(&pool).await.unwrap();
    workflow::publish_empty(&pool, source, 2).await.unwrap();
    sqlx::query("INSERT INTO wiki_pages(id,product_version_id,slug,title,status,content,source_refs) VALUES($1,$2,'source-wiki','Source wiki','published','old source-set wiki',$3)")
        .bind(Uuid::new_v4()).bind(source_version).bind(serde_json::json!([source])).execute(&pool).await.unwrap();

    let follow = knowledge::clone::run_clone(&pool, source_version, target_version, &[], false)
        .await
        .unwrap();
    let target = follow
        .iter()
        .find(|task| task.clone_keep)
        .unwrap()
        .document_id;
    let chunks: Vec<(Uuid, String, i32)> = sqlx::query_as(
        "SELECT id,content,generation FROM chunks WHERE document_id=$1 ORDER BY content",
    )
    .bind(target)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].1, "current text");
    assert_eq!(chunks[0].2, 1);
    let graph: Vec<(String, Vec<Uuid>, i32)> =
        sqlx::query_as("SELECT name,chunk_ids,generation FROM graph_nodes WHERE document_id=$1")
            .bind(target)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(graph, vec![("current".into(), vec![chunks[0].0], 1)]);
    let relation: Vec<String> =
        sqlx::query_scalar("SELECT rel_type FROM graph_relations WHERE document_id=$1")
            .bind(target)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(relation, vec!["current"]);
    let obligations: Vec<String> = sqlx::query_scalar(
        "SELECT job_key FROM knowledge_job_outbox WHERE document_id=$1 ORDER BY job_key",
    )
    .bind(target)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(obligations, vec!["postprocess", "wiki"]);
    assert!(
        follow
            .iter()
            .any(|task| task.task_type == platform::TYPE_WIKI_INGEST)
    );
    let source_chunks: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id=$1")
        .bind(source)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        source_chunks, 3,
        "copy must leave retained source generations intact"
    );
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn reparse_captures_manual_source_in_the_initial_convert_obligation() {
    let pool = pool().await;
    let (_, source_version, source, target_version) = seed(&pool).await;
    let passages = vec!["first passage".to_owned(), "second passage".to_owned()];
    knowledge::set_document_source(&pool, source, "manual", &passages)
        .await
        .unwrap();
    sqlx::query("UPDATE product_versions SET embedding_model_id='different-model' WHERE id=$1")
        .bind(target_version)
        .execute(&pool)
        .await
        .unwrap();
    let follow = knowledge::clone::run_clone(&pool, source_version, target_version, &[], false)
        .await
        .unwrap();
    assert_eq!(follow.len(), 1);
    assert!(!follow[0].clone_keep);
    let payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM knowledge_job_outbox WHERE document_id=$1 AND job_key='convert'",
    )
    .bind(follow[0].document_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        payload,
        serde_json::json!({"kind":"convert", "manual":true, "passages":passages})
    );
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn failed_follow_up_insert_rolls_back_clone_index_and_object_owner() {
    let pool = pool().await;
    let (product, source_version, source, target_version) = seed(&pool).await;
    workflow::stage_index(
        &pool,
        source,
        1,
        &[chunk(source, source_version, "source text")],
        &[],
        true,
    )
    .await
    .unwrap();
    let suffix = Uuid::new_v4().simple().to_string();
    let trigger = format!("clone_reject_{suffix}");
    // SQL identifiers and literals below contain only generated UUIDs and integers.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {trigger}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected clone obligation failure'; END $$;
         CREATE TRIGGER {trigger} BEFORE INSERT ON knowledge_job_outbox FOR EACH ROW
         WHEN (NEW.product_version_id='{target_version}'::uuid AND NEW.job_key='postprocess') EXECUTE FUNCTION {trigger}();"
    ))).execute(&pool).await.unwrap();
    let result =
        knowledge::clone::run_clone(&pool, source_version, target_version, &[], true).await;
    // SQL identifiers and literals below contain only generated UUIDs and integers.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {trigger} ON knowledge_job_outbox; DROP FUNCTION {trigger}();"
    )))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        result
            .unwrap_err()
            .contains("injected clone obligation failure")
    );
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM documents WHERE product_version_id=$1),
                (SELECT count(*) FROM chunks WHERE product_version_id=$1),
                (SELECT count(*) FROM knowledge_job_outbox WHERE product_version_id=$1)",
    )
    .bind(target_version)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0));
    let owners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references WHERE object_ref=(SELECT object_ref FROM documents WHERE id=$1)",
    ).bind(source).fetch_one(&pool).await.unwrap();
    assert_eq!(owners, 1);
    let version: (String, Option<Uuid>) = sqlx::query_as(
        "SELECT v.status,p.current_version_id FROM product_versions v JOIN products p ON p.id=v.product_id WHERE v.id=$1 AND p.id=$2",
    ).bind(target_version).bind(product).fetch_one(&pool).await.unwrap();
    assert_eq!(version, ("cloning".into(), Some(source_version)));
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn target_and_convert_cannot_be_observed_before_clone_initialization_commits() {
    let pool = pool().await;
    let (_, source_version, source, target_version) = seed(&pool).await;
    workflow::stage_index(
        &pool,
        source,
        1,
        &[chunk(source, source_version, "source text")],
        &[],
        true,
    )
    .await
    .unwrap();
    let lock_key = (Uuid::new_v4().as_u128() & i32::MAX as u128) as i64;
    let suffix = Uuid::new_v4().simple().to_string();
    let trigger = format!("clone_pause_{suffix}");
    // SQL identifiers and literals below contain only generated UUIDs and integers.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {trigger}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock({lock_key}); RETURN NEW; END $$;
         CREATE TRIGGER {trigger} BEFORE INSERT ON knowledge_job_outbox FOR EACH ROW
         WHEN (NEW.product_version_id='{target_version}'::uuid AND NEW.job_key='postprocess') EXECUTE FUNCTION {trigger}();"
    ))).execute(&pool).await.unwrap();
    let mut gate = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *gate)
        .await
        .unwrap();
    let clone_pool = pool.clone();
    let task = tokio::spawn(async move {
        knowledge::clone::run_clone(&clone_pool, source_version, target_version, &[], false).await
    });
    let waiting = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND objid=$1::bigint::oid AND NOT granted)",
            ).bind(lock_key).fetch_one(&pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await;
    let visible: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM documents WHERE product_version_id=$1),
                (SELECT count(*) FROM chunks WHERE product_version_id=$1),
                (SELECT count(*) FROM knowledge_job_outbox WHERE product_version_id=$1)",
    )
    .bind(target_version)
    .fetch_one(&pool)
    .await
    .unwrap();
    gate.commit().await.unwrap();
    let result = task.await.unwrap();
    // SQL identifiers and literals below contain only generated UUIDs and integers.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {trigger} ON knowledge_job_outbox; DROP FUNCTION {trigger}();"
    )))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        waiting.is_ok(),
        "clone must reach the paused follow-up insert"
    );
    assert_eq!(
        visible,
        (0, 0, 0),
        "no worker may consume partially initialized clone state"
    );
    let follow = result.unwrap();
    let jobs: Vec<String> = sqlx::query_scalar(
        "SELECT job_key FROM knowledge_job_outbox WHERE document_id=$1 ORDER BY job_key",
    )
    .bind(follow[0].document_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(jobs, vec!["postprocess"]);
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn keep_copies_ocr_artifact_mapping_and_object_ownership() {
    let pool = pool().await;
    let (_, source_version, source, target_version) = seed(&pool).await;
    let original = chunk(source, source_version, "image evidence");
    workflow::stage_index(&pool, source, 1, std::slice::from_ref(&original), &[], true)
        .await
        .unwrap();
    let artifact = Uuid::new_v4();
    let digest = platform::sha256_hex(artifact.as_bytes());
    let object_ref = format!("objects/{digest}");
    sqlx::query("UPDATE chunks SET chunk_type='image_ocr',context_header=$2 WHERE id=$1")
        .bind(original.id)
        .bind(&object_ref)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("SELECT kb_register_knowledge_image_object($1,$2::kb_object_ref,$3::kb_sha256,'image/png',16,NULL::kb_actor_identity)")
        .bind(artifact).bind(&object_ref).bind(&digest).execute(&pool).await.unwrap();
    let payload = serde_json::to_vec(&serde_json::json!({
        "schema_version":1,"image_artifact_revision_id":artifact,
        "product_version_id":source_version,"document_id":source,"revision":1,
        "object_ref":object_ref,"sha256":digest,"media_type":"image/png",
        "width":1,"height":1,"page_ordinal":0,"bounding_region":null,
        "source_image_key":object_ref,
    }))
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_image_artifact_revisions(
            id,product_version_id,document_id,revision,object_ref,content_sha256,
            media_type,width,height,page_ordinal,source_image_key,canonical_payload,artifact_sha256)
         VALUES($1,$2,$3,1,$4,$5,'image/png',1,1,0,$4,$6,encode(public.digest($6::bytea,'sha256'),'hex'))",
    ).bind(artifact).bind(source_version).bind(source).bind(&object_ref).bind(&digest)
        .bind(payload).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO knowledge_image_ocr_chunk_artifact_mappings(
            chunk_id,product_version_id,document_id,image_artifact_revision_id,object_ref,content_sha256,media_type)
         VALUES($1,$2,$3,$4,$5,$6,'image/png')",
    ).bind(original.id).bind(source_version).bind(source).bind(artifact)
        .bind(&object_ref).bind(&digest).execute(&pool).await.unwrap();

    let worker_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE kb_runtime_worker")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let follow =
        knowledge::clone::run_clone(&worker_pool, source_version, target_version, &[], false)
            .await
            .unwrap();
    let target = follow[0].document_id;
    let (copied_chunk, copied_artifact, copied_payload, page): (Uuid, Uuid, Vec<u8>, Option<i32>) = sqlx::query_as(
        "SELECT mapping.chunk_id,artifact.id,artifact.canonical_payload,artifact.page_ordinal
         FROM knowledge_image_ocr_chunk_artifact_mappings mapping
         JOIN knowledge_image_artifact_revisions artifact ON artifact.id=mapping.image_artifact_revision_id
         WHERE mapping.document_id=$1 AND mapping.product_version_id=$2",
    ).bind(target).bind(target_version).fetch_one(&pool).await.unwrap();
    assert_ne!(copied_chunk, original.id);
    assert_ne!(copied_artifact, artifact);
    assert_eq!(page, Some(0));
    let copied_payload: serde_json::Value = serde_json::from_slice(&copied_payload).unwrap();
    assert_eq!(
        copied_payload["image_artifact_revision_id"],
        serde_json::json!(copied_artifact)
    );
    assert_eq!(copied_payload["document_id"], serde_json::json!(target));
    assert_eq!(
        copied_payload["product_version_id"],
        serde_json::json!(target_version)
    );
    assert_eq!(copied_payload["sha256"], serde_json::json!(digest));
    let owners: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_owner_references WHERE object_ref=$1")
            .bind(object_ref)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(owners, 2);
}
