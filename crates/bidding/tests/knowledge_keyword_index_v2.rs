use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

mod support;

const TOKENIZER: &str = "latin-numeric-cjk-bigram";
const TOKENIZER_VERSION: &str = "v1";

type SidecarSnapshot = BTreeMap<Uuid, String>;

#[derive(Clone)]
struct Fixture {
    workspace_id: Uuid,
    product_id: Uuid,
    version_id: Uuid,
    document_id: Uuid,
    object_ref: String,
    chunk_ids: Vec<Uuid>,
}

async fn final_schema(pool: &PgPool) -> bool {
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('kb_knowledge_rebuild_keyword_indexes_v2(uuid)') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    support::require_final_schema("KnowledgeKeywordIndexV2", ready)
}

async fn seed_fixture(pool: &PgPool) -> Fixture {
    let document_id = Uuid::new_v4();
    let file_hash = knowledge::sha256_hex(document_id.as_bytes());
    let fixture = Fixture {
        workspace_id: Uuid::new_v4(),
        product_id: Uuid::new_v4(),
        version_id: Uuid::new_v4(),
        document_id,
        object_ref: format!("objects/{file_hash}"),
        chunk_ids: (0..4).map(|_| Uuid::new_v4()).collect(),
    };
    sqlx::query(
        "INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'keyword v2',$2,'product_line')",
    )
    .bind(fixture.workspace_id)
    .bind(format!("keyword-v2-{}", fixture.workspace_id))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug)
         VALUES($1,$2,'product','keyword v2',$3)",
    )
    .bind(fixture.product_id)
    .bind(fixture.workspace_id)
    .bind(format!("keyword-v2-{}", fixture.product_id))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status)
         VALUES($1,$2,'v2','active')",
    )
    .bind(fixture.version_id)
    .bind(fixture.product_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state)
         VALUES($1,$2,'text/plain',0,'available')",
    )
    .bind(&fixture.object_ref)
    .bind(&file_hash)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO object_owner_references(
             object_ref,owner_kind,owner_id,occurrence,created_by)
         VALUES($1,'knowledge_document',$2,'original','system:knowledge-document-ingest')",
    )
    .bind(&fixture.object_ref)
    .bind(fixture.document_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO documents(
             id,product_version_id,title,parse_status,file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'keyword v2','completed',$3,0,$4,$5)",
    )
    .bind(fixture.document_id)
    .bind(fixture.version_id)
    .bind(format!("{}.txt", fixture.document_id))
    .bind(&file_hash)
    .bind(&fixture.object_ref)
    .execute(pool)
    .await
    .unwrap();

    for (chunk_id, chunk_type, content) in [
        (fixture.chunk_ids[0], "text", "Router42 中国"),
        (fixture.chunk_ids[1], "summary", "知识大脑"),
        (fixture.chunk_ids[2], "question", "!!!🙂é"),
        (fixture.chunk_ids[3], "image_caption", ""),
    ] {
        sqlx::query(
            "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content)
             VALUES($1,$2,$3,$4,$5)",
        )
        .bind(chunk_id)
        .bind(fixture.version_id)
        .bind(fixture.document_id)
        .bind(chunk_type)
        .bind(content)
        .execute(pool)
        .await
        .unwrap();
    }
    fixture
}

async fn snapshot(pool: &PgPool, version_id: Uuid) -> SidecarSnapshot {
    sqlx::query_as::<_, (Uuid, String)>(
        "SELECT keyword_index.chunk_id,
                jsonb_build_array(
                    keyword_index.tokenizer,keyword_index.tokenizer_version,
                    keyword_index.indexed_content,keyword_index.indexed_content_sha256,
                    keyword_index.tsv::text)::text
           FROM chunk_keyword_indexes_v2 keyword_index
           JOIN chunks chunk ON chunk.id=keyword_index.chunk_id
          WHERE chunk.product_version_id=$1
          ORDER BY keyword_index.chunk_id",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .collect()
}

async fn remove_fixture(pool: &PgPool, fixture: &Fixture) {
    sqlx::query("DELETE FROM chunks WHERE document_id=$1")
        .bind(fixture.document_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM object_owner_references WHERE object_ref=$1")
        .bind(&fixture.object_ref)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM documents WHERE id=$1")
        .bind(fixture.document_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM object_registry WHERE object_ref=$1")
        .bind(&fixture.object_ref)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM product_versions WHERE id=$1")
        .bind(fixture.version_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM products WHERE id=$1")
        .bind(fixture.product_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workspaces WHERE id=$1")
        .bind(fixture.workspace_id)
        .execute(pool)
        .await
        .unwrap();
}

async fn assert_sidecar_check_violation(
    pool: &PgPool,
    fixture: &Fixture,
    case_name: &str,
    statement: &'static str,
) {
    let original = snapshot(pool, fixture.version_id).await;
    let mut transaction = pool.begin().await.unwrap();
    let error = sqlx::query(statement)
        .bind(fixture.chunk_ids[0])
        .execute(&mut *transaction)
        .await
        .expect_err(case_name);
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514"),
        "SQLSTATE for {case_name}"
    );
    transaction.rollback().await.unwrap();
    assert_eq!(
        snapshot(pool, fixture.version_id).await,
        original,
        "failed {case_name} must leave the valid sidecar unchanged"
    );
}

async fn assert_seeded_fixture(pool: PgPool, fixture: Fixture) {
    let first_count =
        knowledge::knowledge_index_v2::rebuild_keyword_indexes_v2(&pool, fixture.version_id)
            .await
            .unwrap();
    assert_eq!(first_count, 4);

    let rows = sqlx::query(
        "SELECT keyword_index.chunk_id,keyword_index.tokenizer,
                keyword_index.tokenizer_version,keyword_index.indexed_content,
                keyword_index.indexed_content_sha256,keyword_index.tsv::text,
                kb_knowledge_keyword_token_stream_v2(chunk.content) AS token_stream,
                keyword_index.indexed_content=chunk.content AS raw_content,
                keyword_index.indexed_content_sha256=
                    encode(digest(convert_to(chunk.content,'UTF8'),'sha256'),'hex') AS exact_digest,
                keyword_index.tsv=to_tsvector(
                    'simple',kb_knowledge_keyword_token_stream_v2(chunk.content)) AS exact_tsv
           FROM chunk_keyword_indexes_v2 keyword_index
           JOIN chunks chunk ON chunk.id=keyword_index.chunk_id
          WHERE chunk.product_version_id=$1
          ORDER BY keyword_index.chunk_id",
    )
    .bind(fixture.version_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 4);
    for row in &rows {
        assert_eq!(row.get::<String, _>("tokenizer"), TOKENIZER);
        assert_eq!(row.get::<String, _>("tokenizer_version"), TOKENIZER_VERSION);
        assert!(row.get::<bool, _>("raw_content"));
        assert!(row.get::<bool, _>("exact_digest"));
        assert!(row.get::<bool, _>("exact_tsv"));
    }
    for chunk_id in [fixture.chunk_ids[2], fixture.chunk_ids[3]] {
        let empty_tsv: bool = sqlx::query_scalar(
            "SELECT tsv=''::tsvector FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1",
        )
        .bind(chunk_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(empty_tsv);
    }

    let first_snapshot = snapshot(&pool, fixture.version_id).await;
    let malformed_inserts = [
        (
            "wrong tokenizer",
            "WITH deleted AS (
                 DELETE FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1 RETURNING chunk_id
             )
             INSERT INTO chunk_keyword_indexes_v2(
                 chunk_id,tokenizer,tokenizer_version,indexed_content,
                 indexed_content_sha256,tsv)
             SELECT chunk.id,'wrong-tokenizer','v1',chunk.content,
                    encode(digest(convert_to(chunk.content,'UTF8'),'sha256'),'hex'),
                    to_tsvector('simple',kb_knowledge_keyword_token_stream_v2(chunk.content))
               FROM chunks chunk JOIN deleted ON deleted.chunk_id=chunk.id",
        ),
        (
            "wrong tokenizer version",
            "WITH deleted AS (
                 DELETE FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1 RETURNING chunk_id
             )
             INSERT INTO chunk_keyword_indexes_v2(
                 chunk_id,tokenizer,tokenizer_version,indexed_content,
                 indexed_content_sha256,tsv)
             SELECT chunk.id,'latin-numeric-cjk-bigram','wrong-version',chunk.content,
                    encode(digest(convert_to(chunk.content,'UTF8'),'sha256'),'hex'),
                    to_tsvector('simple',kb_knowledge_keyword_token_stream_v2(chunk.content))
               FROM chunks chunk JOIN deleted ON deleted.chunk_id=chunk.id",
        ),
        (
            "wrong indexed content SHA-256",
            "WITH deleted AS (
                 DELETE FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1 RETURNING chunk_id
             )
             INSERT INTO chunk_keyword_indexes_v2(
                 chunk_id,tokenizer,tokenizer_version,indexed_content,
                 indexed_content_sha256,tsv)
             SELECT chunk.id,'latin-numeric-cjk-bigram','v1',chunk.content,'not-a-sha256',
                    to_tsvector('simple',kb_knowledge_keyword_token_stream_v2(chunk.content))
               FROM chunks chunk JOIN deleted ON deleted.chunk_id=chunk.id",
        ),
        (
            "wrong tsvector",
            "WITH deleted AS (
                 DELETE FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1 RETURNING chunk_id
             )
             INSERT INTO chunk_keyword_indexes_v2(
                 chunk_id,tokenizer,tokenizer_version,indexed_content,
                 indexed_content_sha256,tsv)
             SELECT chunk.id,'latin-numeric-cjk-bigram','v1',chunk.content,
                    encode(digest(convert_to(chunk.content,'UTF8'),'sha256'),'hex'),
                    '''forged'':1'::tsvector
               FROM chunks chunk JOIN deleted ON deleted.chunk_id=chunk.id",
        ),
        (
            "raw content and digest mismatch",
            "WITH deleted AS (
                 DELETE FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1 RETURNING chunk_id
             )
             INSERT INTO chunk_keyword_indexes_v2(
                 chunk_id,tokenizer,tokenizer_version,indexed_content,
                 indexed_content_sha256,tsv)
             SELECT chunk.id,'latin-numeric-cjk-bigram','v1','forged raw content',
                    encode(digest(convert_to(chunk.content,'UTF8'),'sha256'),'hex'),
                    to_tsvector('simple',
                        kb_knowledge_keyword_token_stream_v2('forged raw content'))
               FROM chunks chunk JOIN deleted ON deleted.chunk_id=chunk.id",
        ),
    ];
    for (case_name, statement) in malformed_inserts {
        assert_sidecar_check_violation(&pool, &fixture, case_name, statement).await;
    }
    for (case_name, statement) in [
        (
            "UPDATE forged digest",
            "UPDATE chunk_keyword_indexes_v2
                SET indexed_content_sha256=repeat('0',64)
              WHERE chunk_id=$1",
        ),
        (
            "UPDATE forged tsvector",
            "UPDATE chunk_keyword_indexes_v2
                SET tsv='''forged'':1'::tsvector
              WHERE chunk_id=$1",
        ),
    ] {
        assert_sidecar_check_violation(&pool, &fixture, case_name, statement).await;
    }

    let second_count =
        knowledge::knowledge_index_v2::rebuild_keyword_indexes_v2(&pool, fixture.version_id)
            .await
            .unwrap();
    assert_eq!(second_count, 4);
    assert_eq!(snapshot(&pool, fixture.version_id).await, first_snapshot);

    sqlx::query("UPDATE chunks SET content='Changed 中国内容' WHERE id=$1")
        .bind(fixture.chunk_ids[1])
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        knowledge::knowledge_index_v2::rebuild_keyword_indexes_v2(&pool, fixture.version_id)
            .await
            .unwrap(),
        4
    );
    let changed_snapshot = snapshot(&pool, fixture.version_id).await;
    for chunk_id in &fixture.chunk_ids {
        assert_eq!(
            changed_snapshot[chunk_id] != first_snapshot[chunk_id],
            *chunk_id == fixture.chunk_ids[1]
        );
    }

    sqlx::query("DELETE FROM chunks WHERE id=$1")
        .bind(fixture.chunk_ids[0])
        .execute(&pool)
        .await
        .unwrap();
    let deleted_sidecar_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1")
            .bind(fixture.chunk_ids[0])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(deleted_sidecar_count, 0, "chunk delete must cascade");
    assert_eq!(
        knowledge::knowledge_index_v2::rebuild_keyword_indexes_v2(&pool, fixture.version_id)
            .await
            .unwrap(),
        3
    );

    let missing_id = Uuid::new_v4();
    let missing_error = knowledge::knowledge_index_v2::rebuild_keyword_indexes_v2(&pool, missing_id)
        .await
        .unwrap_err();
    let database_error = missing_error.as_database_error().unwrap();
    assert_eq!(database_error.code().as_deref(), Some("23503"));
    assert!(
        database_error
            .message()
            .contains("KNOWLEDGE_PRODUCT_VERSION_V2_NOT_FOUND")
    );

    let serialized_expected = snapshot(&pool, fixture.version_id).await;
    let (first_started_tx, first_started_rx) = tokio::sync::oneshot::channel();
    let first_pool = pool.clone();
    let version_id = fixture.version_id;
    let mut first_rebuild = tokio::spawn(async move {
        let mut transaction = first_pool.begin().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT kb_knowledge_rebuild_keyword_indexes_v2($1)")
            .bind(version_id)
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
        first_started_tx.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        transaction.commit().await.unwrap();
        count
    });
    match tokio::time::timeout(Duration::from_secs(5), first_started_rx).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let first_result = first_rebuild.await;
            panic!("first rebuild dropped its start signal ({error}): {first_result:?}");
        }
        Err(_) => {
            first_rebuild.abort();
            let _ = first_rebuild.await;
            panic!("first rebuild did not start within timeout");
        }
    }
    let second_pool = pool.clone();
    let mut second_rebuild = tokio::spawn(async move {
        knowledge::knowledge_index_v2::rebuild_keyword_indexes_v2(&second_pool, version_id)
            .await
            .unwrap()
    });
    let (first_result, second_result) = match tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(&mut first_rebuild, &mut second_rebuild)
    })
    .await
    {
        Ok(results) => results,
        Err(_) => {
            first_rebuild.abort();
            second_rebuild.abort();
            let _ = tokio::join!(first_rebuild, second_rebuild);
            panic!("concurrent rebuilds did not complete within timeout");
        }
    };
    let (first_concurrent_count, second_concurrent_count) =
        (first_result.unwrap(), second_result.unwrap());
    assert_eq!((first_concurrent_count, second_concurrent_count), (3, 3));
    let concurrent_snapshot = snapshot(&pool, fixture.version_id).await;
    assert_eq!(concurrent_snapshot, serialized_expected);

    // The rebuild's version-row FOR UPDATE lock conflicts with the key-share
    // lock acquired by the chunks.product_version_id FK check.
    let mut locking_rebuild = pool.begin().await.unwrap();
    let locked_count: i64 =
        sqlx::query_scalar("SELECT kb_knowledge_rebuild_keyword_indexes_v2($1)")
            .bind(fixture.version_id)
            .fetch_one(&mut *locking_rebuild)
            .await
            .unwrap();
    assert_eq!(locked_count, 3);
    let concurrent_chunk_id = Uuid::new_v4();
    let insert_pool = pool.clone();
    let document_id = fixture.document_id;
    let insert_version_id = fixture.version_id;
    let mut concurrent_insert = tokio::spawn(async move {
        sqlx::query(
            "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content)
             VALUES($1,$2,$3,'text','insert waits for rebuild')",
        )
        .bind(concurrent_chunk_id)
        .bind(insert_version_id)
        .bind(document_id)
        .execute(&insert_pool)
        .await
        .unwrap();
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    if concurrent_insert.is_finished() {
        locking_rebuild.rollback().await.unwrap();
        let insert_result = concurrent_insert.await;
        panic!("the FK insert did not wait for the rebuild transaction: {insert_result:?}");
    }
    locking_rebuild.commit().await.unwrap();
    let insert_result =
        match tokio::time::timeout(Duration::from_secs(5), &mut concurrent_insert).await {
            Ok(result) => result,
            Err(_) => {
                concurrent_insert.abort();
                let _ = concurrent_insert.await;
                panic!("waiting chunk insert did not complete after rebuild commit");
            }
        };
    insert_result.unwrap();
    let pre_rebuild_sidecar: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1")
            .bind(concurrent_chunk_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pre_rebuild_sidecar, 0);
    assert_eq!(
        knowledge::knowledge_index_v2::rebuild_keyword_indexes_v2(&pool, fixture.version_id)
            .await
            .unwrap(),
        4
    );
    sqlx::query("DELETE FROM chunks WHERE id=$1")
        .bind(concurrent_chunk_id)
        .execute(&pool)
        .await
        .unwrap();

    for (role, expected_keyword, expected_vector) in [
        (
            "kb_runtime_api",
            (true, false, false, false),
            (true, false, false, false),
        ),
        (
            "kb_runtime_worker",
            (true, false, false, false),
            (true, false, false, false),
        ),
        (
            "public",
            (false, false, false, false),
            (false, false, false, false),
        ),
    ] {
        let keyword_acl: (bool, bool, bool, bool) = sqlx::query_as(
            "SELECT has_table_privilege($1,'chunk_keyword_indexes_v2','SELECT'),
                    has_table_privilege($1,'chunk_keyword_indexes_v2','INSERT'),
                    has_table_privilege($1,'chunk_keyword_indexes_v2','UPDATE'),
                    has_table_privilege($1,'chunk_keyword_indexes_v2','DELETE')",
        )
        .bind(role)
        .fetch_one(&pool)
        .await
        .unwrap();
        let vector_acl: (bool, bool, bool, bool) = sqlx::query_as(
            "SELECT has_table_privilege($1,'chunk_vector_indexes_v2','SELECT'),
                    has_table_privilege($1,'chunk_vector_indexes_v2','INSERT'),
                    has_table_privilege($1,'chunk_vector_indexes_v2','UPDATE'),
                    has_table_privilege($1,'chunk_vector_indexes_v2','DELETE')",
        )
        .bind(role)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(keyword_acl, expected_keyword, "keyword ACL for {role}");
        assert_eq!(vector_acl, expected_vector, "vector ACL for {role}");
    }
    let hardened_helpers: bool = sqlx::query_scalar(
        "SELECT bool_and(
             proconfig @> ARRAY['search_path=pg_catalog, public, pg_temp']::text[]
             AND prosecdef
             AND pg_get_functiondef(proc_value.oid) !~ '(FROM|JOIN|INTO|UPDATE|DELETE FROM) (chunks|documents|product_versions|chunk_keyword_indexes_v2|chunk_vector_indexes_v2)([^a-zA-Z0-9_]|$)')
           FROM pg_proc proc_value
           JOIN pg_namespace namespace_value ON namespace_value.oid=proc_value.pronamespace
          WHERE namespace_value.nspname='public'
            AND proc_value.proname=ANY($1::text[])"
    )
    .bind([
        "kb_knowledge_keyword_token_stream_v2",
        "kb_knowledge_rebuild_keyword_indexes_v2",
        "kb_knowledge_reconcile_vector_indexes_v2",
        "kb_knowledge_lock_rerank_revision_v2",
        "kb_knowledge_normalize_matching_text_v2",
    ])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hardened_helpers);

    for role in ["kb_runtime_api", "kb_runtime_worker"] {
        let has_temporary: bool =
            sqlx::query_scalar("SELECT has_database_privilege($1,current_database(),'TEMPORARY')")
                .bind(role)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            !has_temporary,
            "runtime role {role} must not create temp shadows"
        );
    }

    for (role, rebuild, tokenizer, vector_reconcile) in [
        ("kb_runtime_api", false, true, false),
        ("kb_runtime_worker", true, true, true),
        ("public", false, false, false),
    ] {
        let privileges: (bool, bool, bool) = sqlx::query_as(
            "SELECT has_function_privilege(
                        $1,'kb_knowledge_rebuild_keyword_indexes_v2(uuid)','EXECUTE'),
                    has_function_privilege(
                        $1,'kb_knowledge_keyword_token_stream_v2(text)','EXECUTE'),
                    has_function_privilege(
                        $1,'kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb)','EXECUTE')",
        )
        .bind(role)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            privileges,
            (rebuild, tokenizer, vector_reconcile),
            "function ACL for {role}"
        );
    }
}

#[tokio::test]
async fn keyword_tokenizer_rebuild_concurrency_and_acl_are_exact() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeKeywordIndexV2").await else {
        return;
    };
    if !final_schema(&pool).await {
        return;
    }

    assert_eq!(knowledge::index::KEYWORD_TOKENIZER_V2, TOKENIZER);
    assert_eq!(knowledge::index::KEYWORD_TOKENIZER_VERSION_V2, TOKENIZER_VERSION);
    for (input, expected) in [
        ("Router42 ABC123", "router42 abc123"),
        ("alpha,beta...gamma", "alpha beta gamma"),
        ("知识大脑", "知识 识大 大脑"),
        ("中", "中"),
        ("A\t B\n\rC", "a b c"),
        ("abc中国XYZ", "abc 中国 xyz"),
        ("café🙂naïve", "caf na ve"),
        ("𠀀𠀁", "𠀀𠀁"),
        ("\u{2ebef}\u{2ebf0}", "\u{2ebef}\u{2ebf0}"),
        ("\u{2ee5f}\u{2ee60}", "\u{2ee5f}"),
        ("A中B", "a 中 b"),
        ("", ""),
    ] {
        let rust_stream = knowledge::index::keyword_token_stream_v2(input);
        let sql_stream: String =
            sqlx::query_scalar("SELECT kb_knowledge_keyword_token_stream_v2($1)")
                .bind(input)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rust_stream, expected, "Rust input={input:?}");
        assert_eq!(sql_stream, expected, "SQL input={input:?}");
    }

    let fixture = seed_fixture(&pool).await;
    let outcome = tokio::spawn(assert_seeded_fixture(pool.clone(), fixture.clone())).await;
    remove_fixture(&pool, &fixture).await;
    let stale_sidecars: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chunk_keyword_indexes_v2 WHERE chunk_id=ANY($1)")
            .bind(&fixture.chunk_ids)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stale_sidecars, 0);
    outcome.unwrap();
}
