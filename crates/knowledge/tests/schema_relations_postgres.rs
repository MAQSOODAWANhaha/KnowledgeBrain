//! Relational identity checks use only a caller-provided isolated database.
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL") else {
        return None;
    };
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    assert!(
        matches!(options.get_host(), "127.0.0.1" | "localhost" | "::1")
            && options
                .get_database()
                .is_some_and(|name| name.starts_with("knowledgebrain_test_"))
    );
    Some(PgPool::connect(&url).await.unwrap())
}

async fn seed(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let owner = Uuid::new_v4();
    knowledge::insert_user(pool, owner, &format!("{owner}@relations.test"), None)
        .await
        .unwrap();
    let workspace =
        knowledge::create_workspace_with_library(pool, owner, "relations", &owner.to_string())
            .await
            .unwrap();
    let doc = Uuid::new_v4();
    let hash = platform::sha256_hex(doc.as_bytes());
    knowledge::insert_document(
        pool,
        knowledge::NewDocument {
            id: doc,
            product_version_id: workspace.library_version_id,
            title: "source",
            file_name: "source.txt",
            file_size: 8,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    (workspace.library_id, workspace.library_version_id, doc)
}

fn foreign_key(error: sqlx::Error) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(|value| value.code())
            .as_deref(),
        Some("23503"),
        "{error}"
    );
}

#[tokio::test]
async fn source_rows_cannot_mix_independently_valid_document_and_version_ids() {
    let Some(pool) = pool().await else { return };
    let (_, version, doc) = seed(&pool).await;
    let (_, other_version, _) = seed(&pool).await;
    for sql in [
        "INSERT INTO knowledge_chunks(id,product_version_id,document_id,chunk_type,content) VALUES($1,$2,$3,'text','wrong source')",
        "INSERT INTO knowledge_graph_nodes(product_version_id,document_id,name) VALUES($2,$3,$1::text)",
        "INSERT INTO knowledge_graph_relations(product_version_id,document_id,source_node_name,target_node_name,relation_type) VALUES($2,$3,$1::text,'target','relation')",
        "INSERT INTO knowledge_job_outbox(document_id,product_version_id,attempt,job_key,payload) VALUES($3,$2,1,$1::text,'{}')",
    ] {
        foreign_key(
            sqlx::query(sql)
                .bind(Uuid::new_v4())
                .bind(other_version)
                .bind(doc)
                .execute(&pool)
                .await
                .unwrap_err(),
        );
    }
    let chunk = Uuid::new_v4();
    sqlx::query("INSERT INTO knowledge_chunks(id,product_version_id,document_id,chunk_type,content) VALUES($1,$2,$3,'text','valid source')")
        .bind(chunk).bind(version).bind(doc).execute(&pool).await.unwrap();
    foreign_key(sqlx::query("INSERT INTO knowledge_chunk_embeddings(chunk_id,product_version_id,document_id,content) VALUES($1,$2,$3,'mismatched source')")
        .bind(chunk).bind(other_version).bind(doc).execute(&pool).await.unwrap_err());
}

#[tokio::test]
async fn current_and_clone_pointers_stay_within_their_product() {
    let Some(pool) = pool().await else { return };
    let (product, _, _) = seed(&pool).await;
    let (_, other_version, _) = seed(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE knowledge_products SET current_version_id=$2 WHERE id=$1")
        .bind(product)
        .bind(other_version)
        .execute(&mut *tx)
        .await
        .unwrap();
    foreign_key(tx.commit().await.unwrap_err());
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO knowledge_product_versions(id,product_id,label,status,cloned_from_version_id) VALUES($1,$2,'invalid clone','cloning',$3)")
        .bind(Uuid::new_v4()).bind(product).bind(other_version).execute(&mut *tx).await.unwrap();
    foreign_key(tx.commit().await.unwrap_err());
}

#[tokio::test]
async fn children_can_precede_parents_and_survive_parent_removal() {
    let Some(pool) = pool().await else { return };
    let (_, version, doc) = seed(&pool).await;
    let (parent, child) = (Uuid::new_v4(), Uuid::new_v4());
    let (folder, subfolder, page) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO knowledge_chunks(id,product_version_id,document_id,chunk_type,content,parent_chunk_id) VALUES($1,$2,$3,'text','child content',$4)")
        .bind(child).bind(version).bind(doc).bind(parent).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO knowledge_chunks(id,product_version_id,document_id,chunk_type,content) VALUES($1,$2,$3,'parent_text','parent content')")
        .bind(parent).bind(version).bind(doc).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO knowledge_wiki_pages(id,product_version_id,slug,title,status,folder_id) VALUES($1,$2,'page','page','draft',$3)")
        .bind(page).bind(version).bind(folder).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO knowledge_wiki_folders(id,product_version_id,parent_folder_id,name,path) VALUES($1,$2,$3,'child','parent/child')")
        .bind(subfolder).bind(version).bind(folder).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO knowledge_wiki_folders(id,product_version_id,name,path) VALUES($1,$2,'parent','parent')")
        .bind(folder).bind(version).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("DELETE FROM knowledge_chunks WHERE id=$1")
        .bind(parent)
        .execute(&pool)
        .await
        .unwrap();
    let retained: (String, Option<Uuid>, Uuid) = sqlx::query_as(
        "SELECT content,parent_chunk_id,product_version_id FROM knowledge_chunks WHERE id=$1",
    )
    .bind(child)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained, ("child content".into(), None, version));
    sqlx::query("DELETE FROM knowledge_wiki_folders WHERE id=$1")
        .bind(folder)
        .execute(&pool)
        .await
        .unwrap();
    let retained: (Option<Uuid>, Uuid) = sqlx::query_as(
        "SELECT parent_folder_id,product_version_id FROM knowledge_wiki_folders WHERE id=$1",
    )
    .bind(subfolder)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained, (None, version));
    let retained: (Option<Uuid>, Uuid) =
        sqlx::query_as("SELECT folder_id,product_version_id FROM knowledge_wiki_pages WHERE id=$1")
            .bind(page)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(retained, (None, version));
}
