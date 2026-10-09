//! D1+D2 regression tests: `replace_image_chunks` registers image OCR
//! artifacts atomically, replaces previous image chunks, tolerates
//! non-object source identities (Lenient), and relies on the D2 schema
//! change (mappings follow chunk deletes via ON DELETE CASCADE while
//! UPDATE stays rejected).
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

fn png(seed: Uuid) -> Vec<u8> {
    let mut pixels = image::RgbaImage::new(4, 4);
    for (pixel, byte) in pixels.pixels_mut().zip(seed.as_bytes()) {
        *pixel = image::Rgba([*byte, *byte ^ 0x5a, *byte ^ 0xa5, 255]);
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode PNG");
    bytes.into_inner()
}

fn image_chunk(
    id: Uuid,
    document_id: Uuid,
    version_id: Uuid,
    context_header: String,
    chunk_type: &str,
) -> knowledge::Chunk {
    knowledge::Chunk {
        id,
        document_id,
        product_version_id: version_id,
        chunk_type: chunk_type.into(),
        content: "image text".into(),
        context_header,
        start_at: 0,
        end_at: 10,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
        source_locator: None,
    }
}

async fn fixture(pool: &sqlx::PgPool) -> (Uuid, Uuid) {
    let workspace_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let slug = format!("image-replace-fixture-{}", workspace_id.simple());
    let source_bytes = format!("knowledge-image-replace-source-{document_id}").into_bytes();
    let source_sha = hex::encode(Sha256::digest(&source_bytes));
    let source_ref = platform::object_ref(&source_sha);
    let mut tx = pool.begin().await.expect("fixture transaction");
    sqlx::query("INSERT INTO workspaces(id,name,slug,kind) VALUES($1,$2,$2,'company')")
        .bind(workspace_id)
        .bind(&slug)
        .execute(&mut *tx)
        .await
        .expect("workspace fixture");
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'library',$3,$3)",
    )
    .bind(product_id)
    .bind(workspace_id)
    .bind(&slug)
    .execute(&mut *tx)
    .await
    .expect("product fixture");
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v1','active')",
    )
    .bind(version_id)
    .bind(product_id)
    .execute(&mut *tx)
    .await
    .expect("version fixture");
    sqlx::query("UPDATE products SET current_version_id=$1 WHERE id=$2")
        .bind(version_id)
        .bind(product_id)
        .execute(&mut *tx)
        .await
        .expect("current version fixture");
    sqlx::query(
        "SELECT kb_object_reference_add($1::kb_object_ref,$2::kb_sha256,'text/plain',$3,
          'knowledge_document',$4,'original',NULL)",
    )
    .bind(&source_ref)
    .bind(&source_sha)
    .bind(source_bytes.len() as i64)
    .bind(document_id)
    .execute(&mut *tx)
    .await
    .expect("source object fixture");
    sqlx::query(
        "INSERT INTO documents(id,product_version_id,title,parse_status,enable_status,index_ready,
          file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'replace fixture','completed','enabled',true,'replace-fixture.txt',$3,$4,$5)",
    )
    .bind(document_id)
    .bind(version_id)
    .bind(source_bytes.len() as i64)
    .bind(&source_sha)
    .bind(&source_ref)
    .execute(&mut *tx)
    .await
    .expect("document fixture");
    tx.commit().await.expect("publish fixture");
    (document_id, version_id)
}

async fn pool() -> Option<sqlx::PgPool> {
    let Ok(database_url) = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL") else {
        return None;
    };
    Some(
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database"),
    )
}

#[tokio::test]
async fn replace_image_chunks_registers_artifacts_and_replaces_atomically() {
    let Some(pool) = pool().await else { return };
    let (document_id, version_id) = fixture(&pool).await;

    // First publication of one image key.
    let first_id = Uuid::new_v4();
    let first_bytes = png(first_id);
    let first_sha = hex::encode(Sha256::digest(&first_bytes));
    let first_path = platform::write_blob(&first_sha, &first_bytes).expect("write first blob");
    let first_ref = platform::object_ref(&first_sha);
    let first = image_chunk(
        first_id,
        document_id,
        version_id,
        first_ref.clone(),
        "image_ocr",
    );
    knowledge::replace_image_chunks(&pool, document_id, &first_ref, &[first], &[])
        .await
        .expect("first replace");

    let mapping_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_image_ocr_chunk_artifact_mappings WHERE chunk_id=$1",
    )
    .bind(first_id)
    .fetch_one(&pool)
    .await
    .expect("first mapping state");
    assert_eq!(mapping_count, 1, "image_ocr chunk must be registered");

    // Re-run with a fresh chunk id for the same image key (same bytes).
    let second_id = Uuid::new_v4();
    let second = image_chunk(
        second_id,
        document_id,
        version_id,
        first_ref.clone(),
        "image_ocr",
    );
    knowledge::replace_image_chunks(&pool, document_id, &first_ref, &[second], &[])
        .await
        .expect("second replace");

    let state: (bool, bool, i64, i64, i64) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM chunks WHERE id=$1),
                EXISTS(SELECT 1 FROM chunks WHERE id=$2),
                (SELECT count(*) FROM knowledge_image_ocr_chunk_artifact_mappings WHERE chunk_id=$1),
                (SELECT count(*) FROM knowledge_image_ocr_chunk_artifact_mappings WHERE chunk_id=$2),
                (SELECT count(*) FROM knowledge_image_artifact_revisions WHERE object_ref=$3)",
    )
    .bind(first_id)
    .bind(second_id)
    .bind(&first_ref)
    .fetch_one(&pool)
    .await
    .expect("replace state");
    assert_eq!(
        state,
        (false, true, 0, 1, 2),
        "old chunk+mapping replaced, new mapping registered, old revision row retained as immutable audit trail"
    );

    let _ = std::fs::remove_file(first_path);
}

#[tokio::test]
async fn replace_image_chunks_lenient_skips_invalid_identity() {
    let Some(pool) = pool().await else { return };
    let (document_id, version_id) = fixture(&pool).await;

    let chunk_id = Uuid::new_v4();
    let bad = image_chunk(
        chunk_id,
        document_id,
        version_id,
        "images/not-an-object.png".into(),
        "image_ocr",
    );
    // Lenient: no error, chunk still persisted, no artifact registration.
    knowledge::replace_image_chunks(&pool, document_id, "images/not-an-object.png", &[bad], &[])
        .await
        .expect("lenient replace");

    let state: (bool, i64) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM chunks WHERE id=$1),
                (SELECT count(*) FROM knowledge_image_ocr_chunk_artifact_mappings WHERE chunk_id=$1)",
    )
    .bind(chunk_id)
    .fetch_one(&pool)
    .await
    .expect("lenient state");
    assert_eq!(state, (true, 0));
}

#[tokio::test]
async fn image_ocr_mappings_cascade_on_chunk_delete_but_reject_update() {
    let Some(pool) = pool().await else { return };
    let (document_id, version_id) = fixture(&pool).await;

    let chunk_id = Uuid::new_v4();
    let bytes = png(chunk_id);
    let sha = hex::encode(Sha256::digest(&bytes));
    let blob_path = platform::write_blob(&sha, &bytes).expect("write blob");
    let object_ref = platform::object_ref(&sha);
    let chunk = image_chunk(
        chunk_id,
        document_id,
        version_id,
        object_ref.clone(),
        "image_ocr",
    );
    knowledge::replace_image_chunks(&pool, document_id, &object_ref, &[chunk], &[])
        .await
        .expect("replace");

    // D2: DELETE of the chunk must cascade to mappings (no immutability error).
    sqlx::query("DELETE FROM chunks WHERE document_id=$1")
        .bind(document_id)
        .execute(&pool)
        .await
        .expect("chunk delete cascades to mappings");
    let mapping_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_image_ocr_chunk_artifact_mappings WHERE document_id=$1",
    )
    .bind(document_id)
    .fetch_one(&pool)
    .await
    .expect("mapping state");
    assert_eq!(mapping_count, 0);

    // Re-register, then confirm UPDATE is still rejected.
    let chunk_id2 = Uuid::new_v4();
    let chunk2 = image_chunk(
        chunk_id2,
        document_id,
        version_id,
        object_ref.clone(),
        "image_ocr",
    );
    knowledge::replace_image_chunks(&pool, document_id, &object_ref, &[chunk2], &[])
        .await
        .expect("re-replace");
    let update_err = sqlx::query(
        "UPDATE knowledge_image_ocr_chunk_artifact_mappings SET media_type='image/gif' WHERE chunk_id=$1",
    )
    .bind(chunk_id2)
    .execute(&pool)
    .await
    .expect_err("mapping update stays rejected");
    assert!(
        update_err
            .to_string()
            .contains("KNOWLEDGE_IMAGE_MEDIA_IMMUTABLE"),
        "unexpected error: {update_err}"
    );

    let _ = std::fs::remove_file(blob_path);
}
