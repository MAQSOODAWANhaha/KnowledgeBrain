//! Independent generation/source-set regression probes. Isolated DB only.
use knowledge::{Chunk, ChunkEmbedding, workflow};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let url =
        std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").expect("isolated test database required");
    assert!(!url.contains(":15432/"), "refusing development database");
    PgPool::connect(&url).await.unwrap()
}
async fn seed(pool: &PgPool) -> (Uuid, Uuid) {
    let wid = Uuid::new_v4();
    let pid = Uuid::new_v4();
    let vid = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'audit',$2,'product_line')")
        .bind(wid)
        .bind(wid.to_string())
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','audit','audit')").bind(pid).bind(wid).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO product_versions(id,product_id,label,status,indexing_strategy,embedding_model_id) VALUES($1,$2,'v1','active','{\"vector\":false,\"keyword\":true,\"wiki\":true,\"graph\":false}','invalid-no-provider')").bind(vid).bind(pid).execute(pool).await.unwrap();
    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(pid)
        .bind(vid)
        .execute(pool)
        .await
        .unwrap();
    (pid, vid)
}
async fn document(pool: &PgPool, vid: Uuid) -> Uuid {
    let did = Uuid::new_v4();
    let digest = platform::sha256_hex(did.as_bytes());
    knowledge::insert_document(
        pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: vid,
            title: "audit",
            file_name: "audit.txt",
            file_size: 10,
            file_hash: &digest,
            object_ref: &format!("objects/{digest}"),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE documents SET parse_status='processing' WHERE id=$1")
        .bind(did)
        .execute(pool)
        .await
        .unwrap();
    did
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
        end_at: content.chars().count() as i32,
        parent_chunk_id: None,
        generated_questions: vec![],
        source_locator: None,
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn clone_must_not_promote_retained_old_generation_to_current() {
    let pool = pool().await;
    let (_, vid) = seed(&pool).await;
    let source = document(&pool, vid).await;
    workflow::stage_index(&pool, source, 1, &[chunk(source, vid, "old")], &[], true)
        .await
        .unwrap();
    workflow::publish_empty(&pool, source, 1).await.unwrap();
    workflow::retry_document(&pool, source).await.unwrap();
    sqlx::query("UPDATE documents SET parse_status='processing' WHERE id=$1")
        .bind(source)
        .execute(&pool)
        .await
        .unwrap();
    workflow::stage_index(&pool, source, 2, &[chunk(source, vid, "new")], &[], true)
        .await
        .unwrap();
    workflow::publish_empty(&pool, source, 2).await.unwrap();
    let target = document(&pool, vid).await;
    knowledge::copy_document_index(&pool, source, target, vid)
        .await
        .unwrap();
    let copied: Vec<String> =
        sqlx::query_scalar("SELECT content FROM chunks WHERE document_id=$1 ORDER BY content")
            .bind(target)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        copied,
        vec!["new".to_owned()],
        "old generation was promoted by copy"
    );
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn tombstone_must_hide_wiki_text_hosted_on_another_live_document() {
    let pool = pool().await;
    let (_, vid) = seed(&pool).await;
    let source = document(&pool, vid).await;
    let host = document(&pool, vid).await;
    workflow::publish_empty(&pool, source, 1).await.unwrap();
    let mut wiki = chunk(host, vid, "needle needle needle");
    wiki.chunk_type = "wiki_page".into();
    wiki.context_header = "shared".into();
    let embedding = ChunkEmbedding {
        chunk_id: wiki.id,
        document_id: host,
        product_version_id: vid,
        content: wiki.content.clone(),
        vector: vec![],
        tsv: String::new(),
    };
    sqlx::query("INSERT INTO wiki_pages(id,product_version_id,slug,title,status,content,source_refs) VALUES($1,$2,'shared','Shared','published',$3,$4)").bind(Uuid::new_v4()).bind(vid).bind(&wiki.content).bind(serde_json::json!([source.to_string(),host.to_string()])).execute(&pool).await.unwrap();
    workflow::stage_index(&pool, host, 1, &[wiki], &[embedding], true)
        .await
        .unwrap();
    workflow::publish_empty(&pool, host, 1).await.unwrap();
    let query_vector = knowledge::vector_literal(&[]);
    let before = knowledge::hybrid_search_pg(&pool, vid, "needle", &query_vector, &[], true, 10)
        .await
        .unwrap();
    assert_eq!(before.len(), 1);
    sqlx::query("UPDATE documents SET parse_status='deleting' WHERE id=$1")
        .bind(source)
        .execute(&pool)
        .await
        .unwrap();
    let after = knowledge::hybrid_search_pg(&pool, vid, "needle", &query_vector, &[], true, 10)
        .await
        .unwrap();
    assert!(
        after.is_empty(),
        "Wiki leaks a tombstoned source through its live storage host"
    );
}
