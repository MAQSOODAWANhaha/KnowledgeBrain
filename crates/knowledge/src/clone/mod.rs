//! version:clone — Postgres + caller-enqueued follow-ups. No in-memory Store path.

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneDiff {
    pub op: String,
    pub source_document_id: Option<Uuid>,
}

/// Jobs the caller (worker / runtime) must enqueue after the SQL commit.
#[derive(Debug, Clone)]
pub struct FollowUp {
    pub task_type: &'static str,
    pub queue: &'static str,
    pub document_id: Uuid,
    pub product_version_id: Uuid,
    pub clone_keep: bool,
}

pub async fn run_clone(
    pool: &PgPool,
    source_version_id: Uuid,
    target_version_id: Uuid,
    diffs: &[CloneDiff],
    make_current: bool,
) -> Result<Vec<FollowUp>, String> {
    let product_id: Uuid =
        sqlx::query_scalar("SELECT product_id FROM product_versions WHERE id = $1")
            .bind(target_version_id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

    sqlx::query("UPDATE product_versions SET status = 'cloning', updated_at = now() WHERE id = $1")
        .bind(target_version_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let src_emb: Option<String> =
        sqlx::query_scalar("SELECT embedding_model_id FROM product_versions WHERE id = $1")
            .bind(source_version_id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    let dst_emb: Option<String> =
        sqlx::query_scalar("SELECT embedding_model_id FROM product_versions WHERE id = $1")
            .bind(target_version_id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    let same_embedding = src_emb.unwrap_or_default() == dst_emb.unwrap_or_default();
    let schema_ready = crate::embeddings_schema_ready(pool)
        .await
        .map_err(|e| e.to_string())?;
    let keep_copy = same_embedding && schema_ready;

    let src_docs = sqlx::query(
        "SELECT id, title, file_name, file_size, file_hash, object_ref,
                COALESCE(type, 'file') AS doc_type, source_passages,
                COALESCE(description, '') AS description,
                COALESCE(summary_status, 'none') AS summary_status
         FROM documents WHERE product_version_id = $1 AND deleted_at IS NULL",
    )
    .bind(source_version_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let ops: Vec<CloneDiff> = if diffs.is_empty() {
        src_docs
            .iter()
            .map(|r| CloneDiff {
                op: "keep".into(),
                source_document_id: r.try_get("id").ok(),
            })
            .collect()
    } else {
        diffs.to_vec()
    };

    let mut follow = Vec::new();

    for d in ops {
        match d.op.as_str() {
            "delete" => {}
            "add" | "replace" | "keep" => {
                let Some(sid) = d.source_document_id else {
                    continue;
                };
                let Some(src) = src_docs.iter().find(|r| r.get::<Uuid, _>("id") == sid) else {
                    continue;
                };
                let nid = Uuid::new_v4();
                let title: String = src.try_get("title").unwrap_or_default();
                let file_name: String = src.try_get("file_name").unwrap_or_default();
                let file_size: i64 = src.try_get("file_size").unwrap_or(0);
                let file_hash: String = src.try_get("file_hash").unwrap_or_default();
                let object_ref: String = src.try_get("object_ref").unwrap_or_default();
                crate::insert_document(
                    pool,
                    crate::NewDocument {
                        id: nid,
                        product_version_id: target_version_id,
                        title: &title,
                        file_name: &file_name,
                        file_size,
                        file_hash: &file_hash,
                        object_ref: &object_ref,
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
                let kind: String = src.try_get("doc_type").unwrap_or_else(|_| "file".into());
                let passages: Vec<String> = src
                    .try_get::<Option<serde_json::Value>, _>("source_passages")
                    .ok()
                    .flatten()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                let _ = crate::set_document_source(pool, nid, &kind, &passages).await;
                sqlx::query(
                    "INSERT INTO document_tags (document_id, tag_id)
                     SELECT $1, tag_id FROM document_tags WHERE document_id = $2",
                )
                .bind(nid)
                .bind(sid)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
                let copy_keep = d.op == "keep" && keep_copy;
                if copy_keep {
                    crate::copy_document_index(pool, sid, nid, target_version_id)
                        .await
                        .map_err(|e| e.to_string())?;
                    let desc: String = src.try_get("description").unwrap_or_default();
                    let sum_st: String = src
                        .try_get("summary_status")
                        .unwrap_or_else(|_| "none".into());
                    sqlx::query(
                        "UPDATE documents SET parse_status = 'processing',
                                enable_status = 'enabled',
                                description = $2, summary_status = $3,
                                updated_at = now()
                         WHERE id = $1",
                    )
                    .bind(nid)
                    .bind(&desc)
                    .bind(&sum_st)
                    .execute(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                    follow.push(FollowUp {
                        task_type: crate::TYPE_POST_PROCESS,
                        queue: crate::QUEUE_POSTPROCESS,
                        document_id: nid,
                        product_version_id: target_version_id,
                        clone_keep: true,
                    });
                } else {
                    follow.push(FollowUp {
                        task_type: crate::TYPE_DOCUMENT_PROCESS,
                        queue: crate::QUEUE_DEFAULT,
                        document_id: nid,
                        product_version_id: target_version_id,
                        clone_keep: false,
                    });
                }
            }
            _ => {}
        }
    }

    sqlx::query("UPDATE product_versions SET status = 'active', updated_at = now() WHERE id = $1")
        .bind(target_version_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    if make_current {
        sqlx::query("UPDATE products SET current_version_id = $1 WHERE id = $2")
            .bind(target_version_id)
            .bind(product_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(follow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TEST_PG_SERIAL, create_workspace_with_library, insert_document, insert_user};
    use platform::apply_fresh_baseline;
    use tokio::sync::{OnceCell, SemaphorePermit};

    async fn db_lock() -> SemaphorePermit<'static> {
        TEST_PG_SERIAL
            .acquire()
            .await
            .expect("test semaphore closed")
    }

    async fn connect_test_pool() -> Result<sqlx::PgPool, sqlx::Error> {
        let database_url = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").map_err(|_| {
            sqlx::Error::Configuration(
                "KNOWLEDGEBRAIN_TEST_DATABASE_URL is required for destructive PostgreSQL tests"
                    .into(),
            )
        })?;
        if database_url.contains(":15432/") {
            return Err(sqlx::Error::Configuration(
                "destructive PostgreSQL tests refuse the live :15432 database".into(),
            ));
        }
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(32)
            .connect(&database_url)
            .await
    }

    async fn reset_fresh_schema(pool: &sqlx::PgPool) {
        for statement in [
            "DROP SCHEMA public CASCADE",
            "CREATE SCHEMA public",
            "GRANT ALL ON SCHEMA public TO CURRENT_USER",
        ] {
            sqlx::query(statement)
                .execute(pool)
                .await
                .expect("reset fresh test schema");
        }
        apply_fresh_baseline(pool).await.expect("migrate");
    }

    async fn reset_fresh_schema_once(pool: &sqlx::PgPool) {
        static RESET: OnceCell<()> = OnceCell::const_new();
        RESET.get_or_init(|| reset_fresh_schema(pool)).await;
    }

    #[tokio::test]
    async fn keep_new_document_id_adds_registry_owner_and_leaves_source() {
        let _g = db_lock().await;
        let Ok(pool) = connect_test_pool().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_fresh_schema_once(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Acme", "acme")
            .await
            .unwrap();
        let src_ver = seeded.library_version_id;
        let src_doc = Uuid::new_v4();
        insert_document(
            &pool,
            crate::NewDocument {
                id: src_doc,
                product_version_id: src_ver,
                title: "iso",
                file_name: "iso.txt",
                file_size: 3,
                file_hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                object_ref: "objects/ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            },
        )
        .await
        .unwrap();
        let dst = Uuid::new_v4();
        crate::insert_version_cloning(&pool, dst, seeded.library_id, "2026", src_ver)
            .await
            .unwrap();
        let follow = run_clone(&pool, src_ver, dst, &[], false).await.unwrap();
        assert_eq!(follow.len(), 1);
        assert_eq!(follow[0].task_type, crate::TYPE_POST_PROCESS);
        assert!(follow[0].clone_keep);
        assert_eq!(follow[0].product_version_id, dst);
        let src_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM documents WHERE product_version_id = $1")
                .bind(src_ver)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(src_count, 1);
        let dst_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM documents WHERE product_version_id = $1")
                .bind(dst)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(dst_count, 1);
        let dst_id: Uuid =
            sqlx::query_scalar("SELECT id FROM documents WHERE product_version_id = $1")
                .bind(dst)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(dst_id, src_doc);
        let owner_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM object_owner_references
             WHERE object_ref = 'objects/ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(owner_count, 2);
        let status: String =
            sqlx::query_scalar("SELECT status FROM product_versions WHERE id = $1")
                .bind(dst)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "active");
        let current: Option<Uuid> =
            sqlx::query_scalar("SELECT current_version_id FROM products WHERE id = $1")
                .bind(seeded.library_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(current, Some(src_ver));
        reset_fresh_schema(&pool).await;
    }

    #[tokio::test]
    async fn keep_copies_chunks_when_embedding_matches() {
        let _g = db_lock().await;
        let Ok(pool) = connect_test_pool().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_fresh_schema_once(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Copy", "copy")
            .await
            .unwrap();
        sqlx::query("UPDATE product_versions SET embedding_model_id = 'stub-emb' WHERE id = $1")
            .bind(seeded.library_version_id)
            .execute(&pool)
            .await
            .unwrap();
        let src_doc = Uuid::new_v4();
        insert_document(
            &pool,
            crate::NewDocument {
                id: src_doc,
                product_version_id: seeded.library_version_id,
                title: "spec",
                file_name: "spec.txt",
                file_size: 8,
                file_hash: "5c58b7bd315c5c6c396aaedfe66d523f6f19ad5c4cd0d180cb4574e215cc0148",
                object_ref: "objects/5c58b7bd315c5c6c396aaedfe66d523f6f19ad5c4cd0d180cb4574e215cc0148",
            },
        )
        .await
        .unwrap();
        let cid = Uuid::new_v4();
        let ch = crate::Chunk {
            id: cid,
            document_id: src_doc,
            product_version_id: seeded.library_version_id,
            chunk_type: "text".into(),
            content: "throughput 99".into(),
            context_header: "H".into(),
            start_at: 0,
            end_at: 13,
            parent_chunk_id: None,
            generated_questions: vec!["q?".into()],
        };
        let emb = crate::ChunkEmbedding {
            chunk_id: cid,
            product_version_id: seeded.library_version_id,
            document_id: src_doc,
            content: "throughput 99".into(),
            vector: vec![0.1; crate::models::EMBEDDING_DIM],
            tsv: String::new(),
        };
        crate::replace_document_chunks(&pool, src_doc, &[ch], &[emb])
            .await
            .unwrap();
        let dst = Uuid::new_v4();
        crate::insert_version_cloning(
            &pool,
            dst,
            seeded.library_id,
            "v2",
            seeded.library_version_id,
        )
        .await
        .unwrap();
        let follow = run_clone(&pool, seeded.library_version_id, dst, &[], false)
            .await
            .unwrap();
        assert_eq!(follow.len(), 1);
        assert_eq!(follow[0].task_type, crate::TYPE_POST_PROCESS);
        assert!(follow[0].clone_keep);
        let dst_id = follow[0].document_id;
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
            .bind(dst_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        let copied: String =
            sqlx::query_scalar("SELECT content FROM chunks WHERE document_id = $1")
                .bind(dst_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(copied, "throughput 99");
        let q: serde_json::Value =
            sqlx::query_scalar("SELECT generated_questions FROM chunks WHERE document_id = $1")
                .bind(dst_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(q, serde_json::json!(["q?"]));
        let emb_n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings WHERE document_id = $1")
                .bind(dst_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(emb_n, 1);
        let src_n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
            .bind(src_doc)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(src_n, 1);
        let st: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
            .bind(dst_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(st, "processing");
        reset_fresh_schema(&pool).await;
    }

    #[tokio::test]
    async fn keep_reparses_when_embedding_models_differ() {
        let _g = db_lock().await;
        let Ok(pool) = connect_test_pool().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_fresh_schema_once(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Mismatch", "mismatch")
            .await
            .unwrap();
        sqlx::query("UPDATE product_versions SET embedding_model_id = 'emb-a' WHERE id = $1")
            .bind(seeded.library_version_id)
            .execute(&pool)
            .await
            .unwrap();
        let src_doc = Uuid::new_v4();
        insert_document(
            &pool,
            crate::NewDocument {
                id: src_doc,
                product_version_id: seeded.library_version_id,
                title: "iso",
                file_name: "iso.txt",
                file_size: 3,
                file_hash: "90ff7f6f30beadcbfb07c27b3f7059a97b568b23f82ca30bb2d8fc1fd0a53c0e",
                object_ref: "objects/90ff7f6f30beadcbfb07c27b3f7059a97b568b23f82ca30bb2d8fc1fd0a53c0e",
            },
        )
        .await
        .unwrap();
        let cid = Uuid::new_v4();
        crate::replace_document_chunks(
            &pool,
            src_doc,
            &[crate::Chunk {
                id: cid,
                document_id: src_doc,
                product_version_id: seeded.library_version_id,
                chunk_type: "text".into(),
                content: "hello".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 5,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[crate::ChunkEmbedding {
                chunk_id: cid,
                product_version_id: seeded.library_version_id,
                document_id: src_doc,
                content: "hello".into(),
                vector: vec![0.2; crate::models::EMBEDDING_DIM],
                tsv: String::new(),
            }],
        )
        .await
        .unwrap();
        let dst = Uuid::new_v4();
        crate::insert_version_cloning(
            &pool,
            dst,
            seeded.library_id,
            "v2",
            seeded.library_version_id,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE product_versions SET embedding_model_id = 'emb-b' WHERE id = $1")
            .bind(dst)
            .execute(&pool)
            .await
            .unwrap();
        let follow = run_clone(&pool, seeded.library_version_id, dst, &[], false)
            .await
            .unwrap();
        assert_eq!(follow.len(), 1);
        assert_eq!(follow[0].task_type, crate::TYPE_DOCUMENT_PROCESS);
        assert!(!follow[0].clone_keep);
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
            .bind(follow[0].document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
        reset_fresh_schema(&pool).await;
    }
}
