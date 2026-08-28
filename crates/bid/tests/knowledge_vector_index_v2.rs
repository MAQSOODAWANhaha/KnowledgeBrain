use async_trait::async_trait;
use domain::knowledge_retrieval::{
    EMBEDDING_DIMENSION_V2, EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2,
    EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2, EMBEDDING_REVISION_SCHEMA_V2, EmbeddingRevisionV2,
};
use sqlx::PgPool;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use storage::knowledge_index_v2::{
    VectorEmbeddingInputV2, VectorEmbeddingProviderV2, VectorIndexErrorV2,
    rebuild_vector_indexes_v2,
};
use tokio::sync::Notify;
use uuid::Uuid;

mod support;

#[derive(Clone)]
struct Fixture {
    workspace_id: Uuid,
    product_id: Uuid,
    version_id: Uuid,
    document_id: Uuid,
    object_ref: String,
    chunk_ids: [Uuid; 2],
    revision_sha256: String,
}

#[derive(Clone)]
enum ProviderMode {
    Success,
    Unavailable,
    Blocking {
        started: Arc<Notify>,
        proceed: Arc<Notify>,
    },
}

#[derive(Clone)]
struct FakeProvider {
    calls: Arc<AtomicUsize>,
    mode: ProviderMode,
}

#[async_trait]
impl VectorEmbeddingProviderV2 for FakeProvider {
    async fn embed_batch(
        &self,
        _revision: &EmbeddingRevisionV2,
        _credential_ref: &str,
        inputs: &[VectorEmbeddingInputV2],
    ) -> Result<Vec<Vec<f32>>, VectorIndexErrorV2> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.mode {
            ProviderMode::Success => {}
            ProviderMode::Unavailable => {
                return Err(VectorIndexErrorV2::Unavailable(
                    "deterministic fake timeout".into(),
                ));
            }
            ProviderMode::Blocking { started, proceed } => {
                started.notify_one();
                proceed.notified().await;
            }
        }
        Ok(inputs
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let mut vector = vec![0.0; 1024];
                vector[index % 1024] = 1.0;
                vector
            })
            .collect())
    }
}

fn revision() -> EmbeddingRevisionV2 {
    EmbeddingRevisionV2 {
        schema_version: EMBEDDING_REVISION_SCHEMA_V2,
        provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
        provider_model_identifier: format!("vector-index-v2-{}@2025-01-15", Uuid::new_v4()),
        provider_model_revision_sha256: domain::sha256_hex(b"vector-index-v2 model revision"),
        endpoint_config_sha256: domain::sha256_hex(b"vector-index-v2 endpoint config"),
        endpoint_identity: "https://embeddings.example.test/v1/embeddings".into(),
        dimension: EMBEDDING_DIMENSION_V2,
        request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
        output_normalization_version: EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2.into(),
    }
}

async fn seed(pool: &PgPool) -> Fixture {
    let workspace_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let chunk_ids = [Uuid::new_v4(), Uuid::new_v4()];
    let file_hash = domain::sha256_hex(document_id.as_bytes());
    let object_ref = format!("objects/{file_hash}");
    let revision = revision();
    let revision_sha256 = revision.sha256().unwrap();
    sqlx::query(
        "INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'vector v2',$2,'product_line')",
    )
    .bind(workspace_id)
    .bind(format!("vector-v2-{workspace_id}"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','vector v2',$3)")
        .bind(product_id)
        .bind(workspace_id)
        .bind(format!("vector-v2-{product_id}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v2','active')",
    )
    .bind(version_id)
    .bind(product_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state) VALUES($1,$2,'text/plain',0,'available')")
        .bind(&object_ref)
        .bind(&file_hash)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO object_owner_references(object_ref,owner_kind,owner_id,occurrence,created_by) VALUES($1,'knowledge_document',$2,'original','system:knowledge-document-ingest')")
        .bind(&object_ref)
        .bind(document_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO documents(id,product_version_id,title,parse_status,file_name,file_size,file_hash,object_ref,enable_status,index_ready) VALUES($1,$2,'vector v2','completed',$3,0,$4,$5,'enabled',true)")
        .bind(document_id)
        .bind(version_id)
        .bind(format!("{document_id}.txt"))
        .bind(&file_hash)
        .bind(&object_ref)
        .execute(pool)
        .await
        .unwrap();
    for (chunk_id, kind, header, content) in [
        (chunk_ids[0], "text", "# nonempty", "alpha source"),
        (chunk_ids[1], "question", "# derived", "alpha question"),
    ] {
        sqlx::query("INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,context_header) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(chunk_id)
            .bind(version_id)
            .bind(document_id)
            .bind(kind)
            .bind(content)
            .bind(header)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO embedding_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,dimension,request_config_sha256,output_normalization_version,credential_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'test:vector-v2')")
        .bind(&revision_sha256)
        .bind(revision.canonical_bytes().unwrap())
        .bind(i16::try_from(revision.schema_version).unwrap())
        .bind(&revision.provider_protocol_version)
        .bind(&revision.provider_model_identifier)
        .bind(&revision.provider_model_revision_sha256)
        .bind(&revision.endpoint_config_sha256)
        .bind(&revision.endpoint_identity)
        .bind(i32::try_from(revision.dimension).unwrap())
        .bind(&revision.request_config_sha256)
        .bind(&revision.output_normalization_version)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO product_version_embedding_bindings_v2(product_version_id,embedding_revision_sha256) VALUES($1,$2)")
        .bind(version_id)
        .bind(&revision_sha256)
        .execute(pool)
        .await
        .unwrap();
    Fixture {
        workspace_id,
        product_id,
        version_id,
        document_id,
        object_ref,
        chunk_ids,
        revision_sha256,
    }
}

async fn marker(pool: &PgPool, version_id: Uuid) -> (String, i64) {
    sqlx::query_as("SELECT source_snapshot_sha256,chunk_count FROM product_version_vector_index_generations_v2 WHERE product_version_id=$1")
        .bind(version_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn cleanup_deleted_fixture(pool: &PgPool, fixture: &Fixture) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("DELETE FROM public.object_owner_references WHERE object_ref=$1")
        .bind(&fixture.object_ref)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.object_registry WHERE object_ref=$1")
        .bind(&fixture.object_ref)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.embedding_revisions_v2 DISABLE TRIGGER USER")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.embedding_revisions_v2 WHERE revision_sha256=$1")
        .bind(&fixture.revision_sha256)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.embedding_revisions_v2 ENABLE TRIGGER USER")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.workspaces WHERE id=$1")
        .bind(fixture.workspace_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn cleanup(pool: &PgPool, fixture: &Fixture) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.product_version_embedding_bindings_v2 DISABLE TRIGGER USER")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.embedding_revisions_v2 DISABLE TRIGGER USER")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.chunk_vector_indexes_v2 WHERE product_version_id=$1")
        .bind(fixture.version_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM public.product_version_vector_index_generations_v2 WHERE product_version_id=$1",
    )
    .bind(fixture.version_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM public.product_version_embedding_bindings_v2 WHERE product_version_id=$1",
    )
    .bind(fixture.version_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("DELETE FROM public.embedding_revisions_v2 WHERE revision_sha256=$1")
        .bind(&fixture.revision_sha256)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.product_version_embedding_bindings_v2 ENABLE TRIGGER USER")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.embedding_revisions_v2 ENABLE TRIGGER USER")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.chunks WHERE document_id=$1")
        .bind(fixture.document_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.object_owner_references WHERE object_ref=$1")
        .bind(&fixture.object_ref)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.documents WHERE id=$1")
        .bind(fixture.document_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.object_registry WHERE object_ref=$1")
        .bind(&fixture.object_ref)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.product_versions WHERE id=$1")
        .bind(fixture.version_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.products WHERE id=$1")
        .bind(fixture.product_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.workspaces WHERE id=$1")
        .bind(fixture.workspace_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn bound_version_hard_delete_cascades_only_from_the_parent() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeVectorIndexV2HardDelete").await
    else {
        return;
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb)') IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);
    if !support::require_final_schema("KnowledgeVectorIndexV2HardDelete", ready) {
        return;
    }

    let fixture = seed(&pool).await;
    rebuild_vector_indexes_v2(
        &pool,
        fixture.version_id,
        &FakeProvider {
            calls: Arc::new(AtomicUsize::new(0)),
            mode: ProviderMode::Success,
        },
    )
    .await
    .unwrap();

    let direct_delete = sqlx::query(
        "DELETE FROM product_version_embedding_bindings_v2 WHERE product_version_id=$1",
    )
    .bind(fixture.version_id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_eq!(
        direct_delete
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514"),
        "a live parent's immutable binding cannot be deleted directly"
    );

    let mut unauthorized = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE kb_runtime_retention")
        .execute(&mut *unauthorized)
        .await
        .unwrap();
    let unauthorized_delete = sqlx::query("DELETE FROM product_versions WHERE id=$1")
        .bind(fixture.version_id)
        .execute(&mut *unauthorized)
        .await
        .unwrap_err();
    assert_eq!(
        unauthorized_delete
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("42501"),
        "an unauthorized parent delete must fail before any cascade"
    );
    unauthorized.rollback().await.unwrap();
    let binding_after_unauthorized: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM product_version_embedding_bindings_v2 WHERE product_version_id=$1",
    )
    .bind(fixture.version_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(binding_after_unauthorized, 1);

    let failed_parent_delete = sqlx::query("DELETE FROM product_versions WHERE id=$1")
        .bind(fixture.version_id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert_eq!(
        failed_parent_delete
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503"),
        "a referenced live version cannot be hard-deleted"
    );
    let retained_after_failure: (i64, i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT count(*) FROM product_version_embedding_bindings_v2 WHERE product_version_id=$1),
           (SELECT count(*) FROM product_version_vector_index_generations_v2 WHERE product_version_id=$1),
           (SELECT count(*) FROM chunk_vector_indexes_v2 WHERE product_version_id=$1)",
    )
    .bind(fixture.version_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        retained_after_failure,
        (1, 1, 2),
        "a failed parent deletion must roll back every V2 cascade"
    );

    sqlx::query("DELETE FROM object_owner_references WHERE object_ref=$1")
        .bind(&fixture.object_ref)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE product_versions SET status='archived',deleted_at=clock_timestamp() WHERE id=$1",
    )
    .bind(fixture.version_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        storage::delete_empty_product(&pool, fixture.product_id)
            .await
            .unwrap(),
        "the established empty-product hard-delete path must complete"
    );

    let deleted_counts: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT count(*) FROM products WHERE id=$1),
           (SELECT count(*) FROM product_versions WHERE id=$2),
           (SELECT count(*) FROM documents WHERE id=$3),
           (SELECT count(*) FROM product_version_embedding_bindings_v2 WHERE product_version_id=$2),
           (SELECT count(*) FROM product_version_vector_index_generations_v2 WHERE product_version_id=$2),
           (SELECT count(*) FROM chunk_vector_indexes_v2 WHERE product_version_id=$2)",
    )
    .bind(fixture.product_id)
    .bind(fixture.version_id)
    .bind(fixture.document_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(deleted_counts, (0, 0, 0, 0, 0, 0));
    let registry_retained: i64 =
        sqlx::query_scalar("SELECT count(*) FROM embedding_revisions_v2 WHERE revision_sha256=$1")
            .bind(&fixture.revision_sha256)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        registry_retained, 1,
        "hard deletion removes ownership rows, not immutable registry provenance"
    );

    cleanup_deleted_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn complete_vector_generation_is_atomic_fail_closed_and_acl_guarded() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeVectorIndexV2").await else {
        return;
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb)') IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);
    if !support::require_final_schema("KnowledgeVectorIndexV2", ready) {
        return;
    }
    let ownership_contract: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM information_schema.columns
              WHERE table_schema='public' AND table_name='chunk_vector_indexes_v2'
                AND column_name='product_version_id' AND is_nullable='NO')
           AND to_regclass('public.chunk_vector_indexes_v2_owner_revision_idx') IS NOT NULL
           AND EXISTS(
             SELECT 1 FROM pg_catalog.pg_constraint constraint_value
              JOIN pg_catalog.pg_class relation_value ON relation_value.oid=constraint_value.conrelid
              JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=relation_value.relnamespace
             WHERE namespace_value.nspname='public'
               AND relation_value.relname='chunk_vector_indexes_v2'
               AND constraint_value.contype='f'
               AND pg_catalog.pg_get_constraintdef(constraint_value.oid)
                   LIKE 'FOREIGN KEY (chunk_id, product_version_id) REFERENCES chunks(id, product_version_id)%')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        ownership_contract,
        "vector sidecar owner constraint/index drifted"
    );
    let fixture = seed(&pool).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let success = FakeProvider {
        calls: calls.clone(),
        mode: ProviderMode::Success,
    };
    let first = rebuild_vector_indexes_v2(&pool, fixture.version_id, &success)
        .await
        .unwrap();
    assert_eq!(first.chunk_count, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        marker(&pool, fixture.version_id).await,
        (first.source_snapshot_sha256.clone(), 2)
    );
    let complete: bool = sqlx::query_scalar(
        "SELECT count(*)=2 AND bool_and(source_snapshot_sha256=$2) FROM chunk_vector_indexes_v2 WHERE product_version_id=$1",
    )
    .bind(fixture.version_id)
    .bind(&first.source_snapshot_sha256)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        complete,
        "successful reconcile must publish one complete generation"
    );

    let different_fixture = seed(&pool).await;
    let first_started = Arc::new(Notify::new());
    let first_proceed = Arc::new(Notify::new());
    let first_calls = Arc::new(AtomicUsize::new(0));
    let first_provider = FakeProvider {
        calls: first_calls.clone(),
        mode: ProviderMode::Blocking {
            started: first_started.clone(),
            proceed: first_proceed.clone(),
        },
    };
    let first_pool = pool.clone();
    let first_version = fixture.version_id;
    let first_rebuild = tokio::spawn(async move {
        rebuild_vector_indexes_v2(&first_pool, first_version, &first_provider).await
    });
    first_started.notified().await;

    let different_calls = Arc::new(AtomicUsize::new(0));
    let different_provider = FakeProvider {
        calls: different_calls.clone(),
        mode: ProviderMode::Success,
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        rebuild_vector_indexes_v2(&pool, different_fixture.version_id, &different_provider),
    )
    .await
    .expect("a different version must not wait for this version advisory lock")
    .unwrap();
    assert_eq!(different_calls.load(Ordering::SeqCst), 1);

    let second_calls = Arc::new(AtomicUsize::new(0));
    let second_provider = FakeProvider {
        calls: second_calls.clone(),
        mode: ProviderMode::Success,
    };
    let second_pool = pool.clone();
    let second_version = fixture.version_id;
    let second_rebuild = tokio::spawn(async move {
        rebuild_vector_indexes_v2(&second_pool, second_version, &second_provider).await
    });
    let advisory_wait = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                   SELECT 1 FROM pg_catalog.pg_locks waiter
                   JOIN pg_catalog.pg_locks holder
                     ON holder.locktype=waiter.locktype
                    AND holder.database IS NOT DISTINCT FROM waiter.database
                    AND holder.classid IS NOT DISTINCT FROM waiter.classid
                    AND holder.objid IS NOT DISTINCT FROM waiter.objid
                    AND holder.objsubid IS NOT DISTINCT FROM waiter.objsubid
                    AND holder.pid<>waiter.pid AND holder.granted
                  WHERE waiter.locktype='advisory' AND NOT waiter.granted)",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting {
                break true;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(advisory_wait);
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert_eq!(second_calls.load(Ordering::SeqCst), 0);
    first_proceed.notify_one();
    first_rebuild.await.unwrap().unwrap();
    second_rebuild.await.unwrap().unwrap();
    assert_eq!(second_calls.load(Ordering::SeqCst), 1);

    let moved_version_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'moved-target','active')",
    )
    .bind(moved_version_id)
    .bind(fixture.product_id)
    .execute(&pool)
    .await
    .unwrap();
    let moved = sqlx::query("UPDATE chunks SET product_version_id=$2 WHERE id=$1")
        .bind(fixture.chunk_ids[0])
        .bind(moved_version_id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert_eq!(
        moved
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503"),
        "stored sidecar ownership must prevent chunk reassignment"
    );
    sqlx::query("DELETE FROM product_versions WHERE id=$1")
        .bind(moved_version_id)
        .execute(&pool)
        .await
        .unwrap();

    let mut serialization_lock = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM product_versions WHERE id=$1 FOR UPDATE")
        .bind(fixture.version_id)
        .execute(&mut *serialization_lock)
        .await
        .unwrap();
    let lock_holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *serialization_lock)
        .await
        .unwrap();
    let concurrent_pool = pool.clone();
    let concurrent_provider = success.clone();
    let concurrent_version = fixture.version_id;
    let concurrent = tokio::spawn(async move {
        rebuild_vector_indexes_v2(&concurrent_pool, concurrent_version, &concurrent_provider).await
    });
    let blocked = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM pg_stat_activity
                    WHERE $1=ANY(pg_blocking_pids(pid))
                )",
            )
            .bind(lock_holder_pid)
            .fetch_one(&pool)
            .await
            .unwrap();
            if blocked {
                break true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or(false);
    let blocked_activity: Vec<(i32, String, Vec<i32>)> = sqlx::query_as(
        "SELECT pid,query,pg_blocking_pids(pid) FROM pg_stat_activity WHERE cardinality(pg_blocking_pids(pid))>0",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        blocked,
        "concurrent complete rebuild must serialize on version lock; holder={lock_holder_pid}, activity={blocked_activity:?}"
    );
    serialization_lock.rollback().await.unwrap();
    concurrent.await.unwrap().unwrap();

    let previous_marker = marker(&pool, fixture.version_id).await;
    let unavailable = FakeProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        mode: ProviderMode::Unavailable,
    };
    assert!(matches!(
        rebuild_vector_indexes_v2(&pool, fixture.version_id, &unavailable).await,
        Err(VectorIndexErrorV2::Unavailable(_))
    ));
    assert_eq!(marker(&pool, fixture.version_id).await, previous_marker);

    let started = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());
    let blocking = FakeProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        mode: ProviderMode::Blocking {
            started: started.clone(),
            proceed: proceed.clone(),
        },
    };
    let rebuild_pool = pool.clone();
    let version_id = fixture.version_id;
    let rebuild = tokio::spawn(async move {
        rebuild_vector_indexes_v2(&rebuild_pool, version_id, &blocking).await
    });
    started.notified().await;
    sqlx::query("UPDATE chunks SET content='alpha source changed' WHERE id=$1")
        .bind(fixture.chunk_ids[0])
        .execute(&pool)
        .await
        .unwrap();
    proceed.notify_one();
    assert!(matches!(
        rebuild.await.unwrap(),
        Err(VectorIndexErrorV2::SnapshotChanged(_))
    ));
    assert_eq!(marker(&pool, fixture.version_id).await, previous_marker);

    let refreshed = rebuild_vector_indexes_v2(&pool, fixture.version_id, &success)
        .await
        .unwrap();
    assert_ne!(refreshed.source_snapshot_sha256, previous_marker.0);

    sqlx::query("UPDATE documents SET index_ready=false WHERE id=$1")
        .bind(fixture.document_id)
        .execute(&pool)
        .await
        .unwrap();
    let empty_calls = Arc::new(AtomicUsize::new(0));
    let empty_provider = FakeProvider {
        calls: empty_calls.clone(),
        mode: ProviderMode::Unavailable,
    };
    let empty = rebuild_vector_indexes_v2(&pool, fixture.version_id, &empty_provider)
        .await
        .unwrap();
    assert_eq!(empty.chunk_count, 0);
    assert_eq!(empty_calls.load(Ordering::SeqCst), 0);
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chunk_vector_indexes_v2 WHERE product_version_id=$1",
    )
    .bind(fixture.version_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0, "complete empty reconcile removes stale rows");

    sqlx::query("UPDATE documents SET index_ready=true WHERE id=$1")
        .bind(fixture.document_id)
        .execute(&pool)
        .await
        .unwrap();
    rebuild_vector_indexes_v2(&pool, fixture.version_id, &success)
        .await
        .unwrap();
    let before_revoke = marker(&pool, fixture.version_id).await;
    sqlx::query(
        "UPDATE embedding_revisions_v2 SET support_state='revoked' WHERE revision_sha256=$1",
    )
    .bind(&fixture.revision_sha256)
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        rebuild_vector_indexes_v2(&pool, fixture.version_id, &success).await,
        Err(VectorIndexErrorV2::InvalidConfiguration(_))
    ));
    assert_eq!(marker(&pool, fixture.version_id).await, before_revoke);

    for role in ["kb_runtime_api", "kb_runtime_worker"] {
        for table in [
            "chunk_vector_indexes_v2",
            "product_version_vector_index_generations_v2",
        ] {
            let acl: (bool, bool, bool, bool) = sqlx::query_as(
                "SELECT has_table_privilege($1,$2,'SELECT'),has_table_privilege($1,$2,'INSERT'),has_table_privilege($1,$2,'UPDATE'),has_table_privilege($1,$2,'DELETE')",
            )
            .bind(role)
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(acl, (true, false, false, false), "ACL for {role} {table}");
        }
    }
    let function_acl: (bool, bool, bool) = sqlx::query_as(
        "SELECT has_function_privilege('kb_runtime_api','kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb)','EXECUTE'),has_function_privilege('kb_runtime_worker','kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb)','EXECUTE'),has_function_privilege('public','kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb)','EXECUTE')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(function_acl, (false, true, false));

    cleanup(&pool, &different_fixture).await;
    cleanup(&pool, &fixture).await;
}
