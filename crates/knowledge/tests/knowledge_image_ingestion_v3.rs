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

fn chunk(
    id: Uuid,
    document_id: Uuid,
    product_version_id: Uuid,
    object_ref: String,
) -> knowledge::Chunk {
    knowledge::Chunk {
        id,
        document_id,
        product_version_id,
        chunk_type: "image_ocr".into(),
        content: "frozen image text".into(),
        context_header: object_ref,
        start_at: 0,
        end_at: 17,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
    }
}

#[tokio::test]
async fn image_ocr_publication_is_atomic_idempotent_and_retained() {
    let Ok(database_url) = std::env::var("BIDDING_V2_TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("test database");
    let workspace_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let slug = format!("image-fixture-{}", workspace_id.simple());
    let source_bytes = format!("knowledge-image-source-{document_id}").into_bytes();
    let source_sha = hex::encode(Sha256::digest(&source_bytes));
    let source_ref = platform::object_ref(&source_sha);
    let mut fixture = pool.begin().await.expect("fixture transaction");
    sqlx::query("INSERT INTO workspaces(id,name,slug,kind) VALUES($1,$2,$2,'company')")
        .bind(workspace_id)
        .bind(&slug)
        .execute(&mut *fixture)
        .await
        .expect("workspace fixture");
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'library',$3,$3)",
    )
    .bind(product_id)
    .bind(workspace_id)
    .bind(&slug)
    .execute(&mut *fixture)
    .await
    .expect("product fixture");
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v1','active')",
    )
    .bind(version_id)
    .bind(product_id)
    .execute(&mut *fixture)
    .await
    .expect("version fixture");
    sqlx::query("UPDATE products SET current_version_id=$1 WHERE id=$2")
        .bind(version_id)
        .bind(product_id)
        .execute(&mut *fixture)
        .await
        .expect("current version fixture");
    sqlx::query(
        "SELECT kb_object_reference_add($1::kb_object_ref,$2::kb_sha256,'text/plain',$3,
          'knowledge_document',$4,'original','system:knowledge-document-ingest')",
    )
    .bind(&source_ref)
    .bind(&source_sha)
    .bind(source_bytes.len() as i64)
    .bind(document_id)
    .execute(&mut *fixture)
    .await
    .expect("source object fixture");
    sqlx::query(
        "INSERT INTO documents(id,product_version_id,title,parse_status,enable_status,index_ready,
          file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'verified source','completed','enabled',true,'verified-source.txt',$3,$4,$5)",
    )
    .bind(document_id)
    .bind(version_id)
    .bind(source_bytes.len() as i64)
    .bind(&source_sha)
    .bind(&source_ref)
    .execute(&mut *fixture)
    .await
    .expect("document fixture");
    fixture.commit().await.expect("publish fixture");

    let chunk_id = Uuid::new_v4();
    let first_bytes = png(chunk_id);
    let first_sha = hex::encode(Sha256::digest(&first_bytes));
    let first_path = platform::write_blob(&first_sha, &first_bytes).expect("write first blob");
    let first = chunk(
        chunk_id,
        document_id,
        version_id,
        platform::object_ref(&first_sha),
    );
    knowledge::append_document_chunks(&pool, std::slice::from_ref(&first), &[])
        .await
        .expect("first publication");
    knowledge::append_document_chunks(&pool, std::slice::from_ref(&first), &[])
        .await
        .expect("byte-identical replay");

    let published:(i64,i64,i64,i64,i32,i32)=sqlx::query_as(
        "SELECT registry.byte_length,
          (SELECT count(*) FROM knowledge_image_artifact_revisions artifact WHERE artifact.object_ref=registry.object_ref),
          (SELECT count(*) FROM knowledge_image_ocr_chunk_artifact_mappings mapping WHERE mapping.chunk_id=$1),
          (SELECT count(*) FROM object_owner_references owner WHERE owner.object_ref=registry.object_ref AND owner.owner_kind='knowledge_image_artifact'),
          artifact.width,artifact.height
         FROM object_registry registry JOIN knowledge_image_artifact_revisions artifact ON artifact.object_ref=registry.object_ref
         WHERE registry.object_ref=$2")
        .bind(chunk_id).bind(platform::object_ref(&first_sha)).fetch_one(&pool).await.expect("published identities");
    assert_eq!(published, (first_bytes.len() as i64, 1, 1, 1, 4, 4));

    let second_bytes = png(Uuid::new_v4());
    let second_sha = hex::encode(Sha256::digest(&second_bytes));
    let second_path = platform::write_blob(&second_sha, &second_bytes).expect("write second blob");
    let conflicting = chunk(
        chunk_id,
        document_id,
        version_id,
        platform::object_ref(&second_sha),
    );
    let ordinary_id = Uuid::new_v4();
    let ordinary = knowledge::Chunk {
        id: ordinary_id,
        document_id,
        product_version_id: version_id,
        chunk_type: "text".into(),
        content: "must roll back".into(),
        context_header: String::new(),
        start_at: 0,
        end_at: 14,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
    };
    let error = knowledge::append_document_chunks(&pool, &[ordinary, conflicting], &[])
        .await
        .expect_err("identity conflict");
    assert!(error.to_string().contains("idempotency conflict"));
    let rollback:(bool,bool)=sqlx::query_as("SELECT EXISTS(SELECT 1 FROM chunks WHERE id=$1),EXISTS(SELECT 1 FROM object_registry WHERE object_ref=$2)")
        .bind(ordinary_id).bind(platform::object_ref(&second_sha)).fetch_one(&pool).await.expect("rollback state");
    assert_eq!(rollback, (false, false));

    let invalid_id = Uuid::new_v4();
    let invalid = chunk(
        invalid_id,
        document_id,
        version_id,
        "images/not-an-object.png".into(),
    );
    assert!(
        knowledge::append_document_chunks(&pool, &[invalid], &[])
            .await
            .expect_err("closed input contract")
            .to_string()
            .contains("objects/{sha256}")
    );
    let invalid_persisted: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chunks WHERE id=$1)")
            .bind(invalid_id)
            .fetch_one(&pool)
            .await
            .expect("invalid input state");
    assert!(!invalid_persisted);

    let original:(String,String,String,i64)=sqlx::query_as("SELECT registry.object_ref,registry.digest,registry.media_type,registry.byte_length
        FROM documents document JOIN object_registry registry ON registry.object_ref=document.object_ref WHERE document.id=$1")
        .bind(document_id).fetch_one(&pool).await.expect("original source identity");
    let mut retirement = pool.begin().await.expect("retirement transaction");
    sqlx::query("SELECT kb_object_reference_remove($1,'knowledge_document',$2,'original')")
        .bind(&original.0)
        .bind(document_id)
        .execute(&mut *retirement)
        .await
        .expect("release live owner");
    sqlx::query("UPDATE documents SET deleted_at=clock_timestamp() WHERE id=$1")
        .bind(document_id)
        .execute(&mut *retirement)
        .await
        .expect("retire source document");
    sqlx::query("SET CONSTRAINTS documents_object_reference_contract IMMEDIATE")
        .execute(&mut *retirement)
        .await
        .expect("validate source retirement");
    let owner_retained: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM object_owner_references owner
        WHERE owner.object_ref=$1 AND owner.owner_kind='knowledge_image_artifact')",
    )
    .bind(platform::object_ref(&first_sha))
    .fetch_one(&mut *retirement)
    .await
    .expect("owner retention");
    assert!(owner_retained);
    retirement.rollback().await.expect("restore source fixture");

    let _ = std::fs::remove_file(first_path);
    let _ = std::fs::remove_file(second_path);
}
