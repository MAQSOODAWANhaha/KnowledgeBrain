//! Requires a fresh, isolated baseline database. These tests never reset a schema.
use knowledge::{
    Chunk, ChunkEmbedding, DocJob, SummaryStatus,
    workflow::{self, Work},
};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let url =
        std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").expect("isolated test database required");
    assert!(
        !url.contains(":15432/"),
        "refusing the live development database"
    );
    PgPool::connect(&url).await.unwrap()
}
async fn seed(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let wid = Uuid::new_v4();
    let pid = Uuid::new_v4();
    let vid = Uuid::new_v4();
    let did = Uuid::new_v4();
    knowledge::insert_user(pool, wid, &format!("{wid}@test.invalid"), None)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_workspaces(id,name,slug,kind,owner_user_id) VALUES($1,'workflow',$2,'product_line',$1)",
    )
    .bind(wid)
    .bind(wid.to_string())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO knowledge_products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','workflow','workflow')").bind(pid).bind(wid).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO knowledge_product_versions(id,product_id,label,status,indexing_strategy,embedding_model_id) VALUES($1,$2,'v1','active','{\"vector\":false,\"keyword\":true,\"wiki\":false,\"graph\":false}','invalid-no-provider')").bind(vid).bind(pid).execute(pool).await.unwrap();
    sqlx::query("UPDATE knowledge_products SET current_version_id=$2 WHERE id=$1")
        .bind(pid)
        .bind(vid)
        .execute(pool)
        .await
        .unwrap();
    let digest = platform::sha256_hex(did.as_bytes());
    knowledge::insert_document(
        pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: vid,
            title: "needle",
            file_name: "fixture.txt",
            file_size: 16,
            file_hash: &digest,
            object_ref: &format!("objects/{digest}"),
        },
    )
    .await
    .unwrap();
    knowledge::set_parse_status(pool, did, knowledge::ParseStatus::Processing, "")
        .await
        .unwrap();
    (pid, vid, did)
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
async fn explicit_state_mutations_keep_obligations_without_noop_revisions() {
    let pool = pool().await;
    let (_, _, did) = seed(&pool).await;
    let initial: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=1 AND job_key='convert' AND payload->>'kind'='convert')")
        .bind(did).fetch_one(&pool).await.unwrap();
    assert!(
        initial,
        "document creation must commit its conversion obligation"
    );
    let before: i64 = sqlx::query_scalar("SELECT revision FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=1 AND job_key='reconcile'")
        .bind(did).fetch_one(&pool).await.unwrap();
    knowledge::set_parse_status(&pool, did, knowledge::ParseStatus::Processing, "")
        .await
        .unwrap();
    let after: i64 = sqlx::query_scalar("SELECT revision FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=1 AND job_key='reconcile'")
        .bind(did).fetch_one(&pool).await.unwrap();
    assert_eq!(
        before, after,
        "no-op status writes must not restart reconciliation"
    );
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn stale_span_updates_are_ignored_and_single_connection_open_does_not_deadlock() {
    let pool = pool().await;
    let (_, _, did) = seed(&pool).await;
    let single = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        knowledge::open_attempt(&single, did, 1),
    )
    .await
    .expect("span lifecycle must not acquire a second pooled connection")
    .unwrap();
    knowledge::start_span(&single, did, 1, "embedding", None, None)
        .await
        .unwrap();
    workflow::retry_document(&single, did).await.unwrap();
    knowledge::finish_span(
        &single,
        did,
        1,
        "embedding",
        knowledge::SpanStatus::Done,
        None,
    )
    .await
    .unwrap();
    knowledge::upsert_span(
        &single,
        did,
        1,
        "late-worker",
        knowledge::SpanStatus::Done,
        None,
    )
    .await
    .unwrap();
    let old: String=sqlx::query_scalar("SELECT status FROM knowledge_document_processing_spans WHERE document_id=$1 AND attempt=1 AND name='embedding'")
        .bind(did).fetch_one(&pool).await.unwrap();
    assert_eq!(old, "running");
    let late: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_document_processing_spans WHERE document_id=$1 AND name='late-worker')")
        .bind(did).fetch_one(&pool).await.unwrap();
    assert!(!late);
    knowledge::open_attempt(&single, did, 2).await.unwrap();
    knowledge::set_parse_status(&single, did, knowledge::ParseStatus::Cancelled, "")
        .await
        .unwrap();
    knowledge::finish_span(
        &single,
        did,
        2,
        "document_processing",
        knowledge::SpanStatus::Done,
        None,
    )
    .await
    .unwrap();
    let current: String=sqlx::query_scalar("SELECT status FROM knowledge_document_processing_spans WHERE document_id=$1 AND attempt=2 AND name='document_processing'")
        .bind(did).fetch_one(&pool).await.unwrap();
    assert_eq!(current, "running");
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn queue_failure_keeps_obligation_and_duplicate_summary_has_one_commit() {
    let pool = pool().await;
    let (_, vid, did) = seed(&pool).await;
    workflow::put(&pool, did, vid, 1, Work::Postprocess { clone_keep: false })
        .await
        .unwrap();
    workflow::begin_postprocess(&pool, did, vid, 1, &[Work::Summary])
        .await
        .unwrap();
    // Redis is unavailable for this fixture: publication fails after the
    // domain transaction, without losing the summary or decrementing its count.
    workflow::dispatch(&pool).await.unwrap();
    let state: (String, i32, String) = sqlx::query_as(
        "SELECT parse_status,pending_subtasks_count,summary_status FROM knowledge_documents WHERE id=$1",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, ("finalizing".into(), 1, "pending".into()));
    let durable:(String,Option<String>)=sqlx::query_as("SELECT state,last_error FROM knowledge_job_outbox WHERE document_id=$1 AND job_key='summary'").bind(did).fetch_one(&pool).await.unwrap();
    assert_eq!(durable.0, "pending");
    assert!(durable.1.is_some());
    let mut job = DocJob::from_pool(&pool, did).await.unwrap().unwrap();
    job.document.summary_status = SummaryStatus::Completed;
    job.document.description = "durable summary".into();
    let (left, right) = tokio::join!(
        workflow::commit_chunks(&pool, &job, 1, "summary", &["summary"], None, &[], true),
        workflow::commit_chunks(&pool, &job, 1, "summary", &["summary"], None, &[], true)
    );
    assert_eq!(usize::from(left.unwrap()) + usize::from(right.unwrap()), 1);
    let state: (String, i32, String) = sqlx::query_as(
        "SELECT parse_status,pending_subtasks_count,summary_status FROM knowledge_documents WHERE id=$1",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, ("completed".into(), 0, "completed".into()));
    assert!(!workflow::pending(&pool, did, 1, "summary").await.unwrap());
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn stale_attempt_writes_and_old_cleanup_cannot_touch_new_generation() {
    let pool = pool().await;
    let (_, vid, did) = seed(&pool).await;
    let old = chunk(did, vid, "old");
    workflow::stage_index(&pool, did, 1, std::slice::from_ref(&old), &[], true)
        .await
        .unwrap();
    workflow::publish_empty(&pool, did, 1).await.unwrap();
    assert_eq!(workflow::retry_document(&pool, did).await.unwrap(), 2);
    knowledge::set_parse_status(&pool, did, knowledge::ParseStatus::Processing, "")
        .await
        .unwrap();
    let new = chunk(did, vid, "new");
    assert!(
        workflow::stage_index(&pool, did, 2, std::slice::from_ref(&new), &[], true)
            .await
            .unwrap()
    );
    assert!(
        !workflow::stage_index(&pool, did, 1, &[chunk(did, vid, "stale")], &[], true)
            .await
            .unwrap()
    );
    knowledge::ingest::fail_now(&pool, did, 1, "stale failure")
        .await
        .unwrap();
    workflow::delete_generation(&pool, did, 1).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM knowledge_chunks WHERE document_id=$1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    workflow::publish_empty(&pool, did, 2).await.unwrap();
    workflow::delete_generation(&pool, did, 1).await.unwrap();
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM knowledge_chunks WHERE document_id=$1")
        .bind(did)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(ids, vec![new.id]);
    knowledge::set_parse_status(&pool, did, knowledge::ParseStatus::Deleting, "")
        .await
        .unwrap();
    knowledge::ingest::fail_now(&pool, did, 2, "late failure")
        .await
        .unwrap();
    assert_eq!(
        knowledge::document_parse_status(&pool, did).await.unwrap(),
        Some(knowledge::ParseStatus::Deleting)
    );
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn keyword_retrieval_and_source_reconcile_survive_deletion_without_embedding() {
    let pool = pool().await;
    let (pid, vid, deleted) = seed(&pool).await;
    let survivor = Uuid::new_v4();
    let digest = platform::sha256_hex(survivor.as_bytes());
    knowledge::insert_document(
        &pool,
        knowledge::NewDocument {
            id: survivor,
            product_version_id: vid,
            title: "survivor",
            file_name: "survivor.txt",
            file_size: 16,
            file_hash: &digest,
            object_ref: &format!("objects/{digest}"),
        },
    )
    .await
    .unwrap();
    knowledge::set_parse_status(&pool, survivor, knowledge::ParseStatus::Processing, "")
        .await
        .unwrap();
    for did in [deleted, survivor] {
        let c = chunk(did, vid, "needle needle needle");
        let e = ChunkEmbedding {
            chunk_id: c.id,
            document_id: did,
            product_version_id: vid,
            content: c.content.clone(),
            vector: vec![],
            tsv: String::new(),
        };
        workflow::stage_index(&pool, did, 1, &[c], &[e], true)
            .await
            .unwrap();
        workflow::publish_empty(&pool, did, 1).await.unwrap();
    }
    let before = knowledge::knowledge_index_v2::source_snapshot_v2(
        &mut pool.acquire().await.unwrap(),
        vid,
        &"a".repeat(64),
    )
    .await
    .unwrap();
    knowledge::set_parse_status(&pool, deleted, knowledge::ParseStatus::Deleting, "")
        .await
        .unwrap();
    let after = knowledge::knowledge_index_v2::source_snapshot_v2(
        &mut pool.acquire().await.unwrap(),
        vid,
        &"a".repeat(64),
    )
    .await
    .unwrap();
    assert_eq!((before.1, after.1), (2, 1));
    assert_ne!(before.0, after.0);
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_job_outbox WHERE document_id=$1 AND job_key='reconcile' AND state='pending')").bind(deleted).fetch_one(&pool).await.unwrap();
    assert!(pending);
    let req:knowledge::search::SearchRequest=serde_json::from_value(serde_json::json!({"product_id":pid,"query":"needle","expand_wiki":false,"expand_graph":false})).unwrap();
    let owner_id: Uuid = sqlx::query_scalar("SELECT w.owner_user_id FROM knowledge_products p JOIN knowledge_workspaces w ON w.id=p.workspace_id WHERE p.id=$1")
        .bind(pid).fetch_one(&pool).await.unwrap();
    let hits = knowledge::search::assembly_pg(&pool, &req, owner_id)
        .await
        .unwrap()
        .hits;
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.document_id == survivor));
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn exhausted_obligation_is_explicit_and_retry_opens_fresh_attempt() {
    let pool = pool().await;
    let (_, vid, did) = seed(&pool).await;
    workflow::begin_postprocess(&pool, did, vid, 1, &[Work::Summary])
        .await
        .unwrap();
    workflow::fail_job(&pool, did, 1, "summary", "provider retries exhausted")
        .await
        .unwrap();
    let state: (String, String) =
        sqlx::query_as("SELECT parse_status,summary_status FROM knowledge_documents WHERE id=$1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, ("failed".into(), "failed".into()));
    // A delayed conversion delivery must not revive a terminal derived failure.
    sqlx::query("UPDATE knowledge_job_outbox SET state='complete' WHERE document_id=$1 AND job_key='convert'")
        .bind(did).execute(&pool).await.unwrap();
    knowledge::ingest::run_convert(
        &pool,
        did,
        1,
        &[],
        false,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        knowledge::document_parse_status(&pool, did).await.unwrap(),
        Some(knowledge::ParseStatus::Failed)
    );
    // Ordinary API uploads store JSON null rather than SQL NULL passages.
    sqlx::query("UPDATE knowledge_documents SET source_passages='null'::jsonb WHERE id=$1")
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(workflow::retry_document(&pool, did).await.unwrap(), 2);
    let payload:serde_json::Value=sqlx::query_scalar("SELECT payload FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=2 AND job_key='convert'")
        .bind(did).fetch_one(&pool).await.unwrap();
    assert!(
        matches!(serde_json::from_value::<Work>(payload).unwrap(),Work::Convert { passages,manual:false } if passages.is_empty())
    );
    let row:(String,i32)=sqlx::query_as("SELECT state,attempt FROM knowledge_job_outbox WHERE document_id=$1 AND job_key='convert' ORDER BY attempt DESC LIMIT 1").bind(did).fetch_one(&pool).await.unwrap();
    assert_eq!(row, ("pending".into(), 2));
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn duplicate_conversion_cannot_fail_active_owner_and_stalled_owner_gets_new_attempt() {
    let pool = pool().await;
    let (_, vid, did) = seed(&pool).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    knowledge::ingest::run_convert(&pool, did, 1, &[], false, &cancel)
        .await
        .unwrap();
    assert_eq!(
        knowledge::document_parse_status(&pool, did).await.unwrap(),
        Some(knowledge::ParseStatus::Processing)
    );
    let row: (i32, String) = sqlx::query_as(
        "SELECT attempt,COALESCE(error_message,'') FROM knowledge_documents WHERE id=$1",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row, (1, String::new()));
    sqlx::query("UPDATE knowledge_documents SET updated_at=now()-interval '3 hours' WHERE id=$1")
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
    workflow::dispatch(&pool).await.unwrap();
    let row: (i32, String) =
        sqlx::query_as("SELECT attempt,parse_status FROM knowledge_documents WHERE id=$1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, (2, "pending".into()));
    assert!(
        !workflow::stage_index(
            &pool,
            did,
            1,
            &[chunk(did, vid, "late old provider")],
            &[],
            true
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn empty_conversion_publication_failure_rolls_back_its_receipt() {
    let pool = pool().await;
    let (_, vid, did) = seed(&pool).await;
    sqlx::query("UPDATE knowledge_documents SET title='reject-empty-test' WHERE id=$1")
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION knowledge_test_reject_complete() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.title='reject-empty-test' AND NEW.parse_status='completed' THEN RAISE EXCEPTION 'injected publication failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER knowledge_test_reject_complete BEFORE UPDATE ON knowledge_documents FOR EACH ROW EXECUTE FUNCTION knowledge_test_reject_complete();").execute(&pool).await.unwrap();
    assert!(
        knowledge::ingest::after_index_fanout(&pool, did, vid, 1, 0, &[], "")
            .await
            .is_err()
    );
    let receipt:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_job_receipts WHERE document_id=$1 AND job_key='convert')").bind(did).fetch_one(&pool).await.unwrap();
    assert!(!receipt);
    assert_eq!(
        knowledge::document_parse_status(&pool, did).await.unwrap(),
        Some(knowledge::ParseStatus::Processing)
    );
    sqlx::raw_sql("DROP TRIGGER knowledge_test_reject_complete ON knowledge_documents; DROP FUNCTION knowledge_test_reject_complete();").execute(&pool).await.unwrap();
    knowledge::ingest::after_index_fanout(&pool, did, vid, 1, 0, &[], "")
        .await
        .unwrap();
    let done: (String, i32, bool) = sqlx::query_as(
        "SELECT parse_status,active_generation,index_ready FROM knowledge_documents WHERE id=$1",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(done, ("completed".into(), 1, true));
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn source_change_invalidates_an_older_reconcile_acknowledgement() {
    let pool = pool().await;
    let (_, _, did) = seed(&pool).await;
    let old: i64 = sqlx::query_scalar(
        "SELECT revision FROM knowledge_job_outbox WHERE document_id=$1 AND job_key='reconcile'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    knowledge::set_parse_status(&pool, did, knowledge::ParseStatus::Deleting, "")
        .await
        .unwrap();
    let stale_ack=sqlx::query("UPDATE knowledge_job_outbox SET state='complete' WHERE document_id=$1 AND job_key='reconcile' AND revision=$2").bind(did).bind(old).execute(&pool).await.unwrap();
    assert_eq!(stale_ack.rows_affected(), 0);
    let state: String = sqlx::query_scalar(
        "SELECT state FROM knowledge_job_outbox WHERE document_id=$1 AND job_key='reconcile'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, "pending");
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn delayed_reparse_envelope_preserves_completed_generation() {
    let pool = pool().await;
    let (_, vid, did) = seed(&pool).await;
    let c = chunk(did, vid, "new generation survives delayed envelope");
    workflow::stage_index(&pool, did, 1, std::slice::from_ref(&c), &[], true)
        .await
        .unwrap();
    workflow::publish_empty(&pool, did, 1).await.unwrap();
    knowledge::ingest::run_reparse(&pool, did, 1).await.unwrap();
    let id: Uuid = sqlx::query_scalar("SELECT id FROM knowledge_chunks WHERE document_id=$1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(id, c.id);
    assert_eq!(
        knowledge::document_parse_status(&pool, did).await.unwrap(),
        Some(knowledge::ParseStatus::Completed)
    );
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn last_image_receipt_commits_stage_and_next_obligation_atomically() {
    let pool = pool().await;
    let (_, vid, did) = seed(&pool).await;
    sqlx::query("INSERT INTO knowledge_document_processing_spans(document_id,attempt,name,status,started_at) VALUES($1,1,'multimodal','running',now())")
        .bind(did).execute(&pool).await.unwrap();
    for image_key in ["first", "last"] {
        workflow::put(
            &pool,
            did,
            vid,
            1,
            Work::Image {
                image_key: image_key.into(),
                image_source_type: "document".into(),
            },
        )
        .await
        .unwrap();
    }
    workflow::complete(&pool, did, 1, "image:first")
        .await
        .unwrap();
    assert!(
        !workflow::pending(&pool, did, 1, "postprocess")
            .await
            .unwrap()
    );
    // Fail the next obligation insert, after the image receipt and span update.
    // The entire image commit must roll back, rather than leave a lost wakeup.
    sqlx::raw_sql("CREATE FUNCTION knowledge_test_reject_postprocess() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.job_key='postprocess' THEN RAISE EXCEPTION 'injected next obligation failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER knowledge_test_reject_postprocess BEFORE INSERT ON knowledge_job_outbox FOR EACH ROW EXECUTE FUNCTION knowledge_test_reject_postprocess();")
        .execute(&pool).await.unwrap();
    assert!(
        workflow::complete(&pool, did, 1, "image:last")
            .await
            .is_err()
    );
    assert!(
        workflow::pending(&pool, did, 1, "image:last")
            .await
            .unwrap()
    );
    let status: String = sqlx::query_scalar(
        "SELECT status FROM knowledge_document_processing_spans WHERE document_id=$1 AND name='multimodal'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "running");
    sqlx::raw_sql("DROP TRIGGER knowledge_test_reject_postprocess ON knowledge_job_outbox; DROP FUNCTION knowledge_test_reject_postprocess();").execute(&pool).await.unwrap();
    workflow::complete(&pool, did, 1, "image:last")
        .await
        .unwrap();
    assert!(
        workflow::pending(&pool, did, 1, "postprocess")
            .await
            .unwrap()
    );
    let status: String = sqlx::query_scalar(
        "SELECT status FROM knowledge_document_processing_spans WHERE document_id=$1 AND name='multimodal'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "done");
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL baseline"]
async fn delayed_wiki_reducer_cannot_resurrect_tombstoned_or_reparsed_source() {
    let pool = pool().await;
    for tombstone in [true, false] {
        let (_, vid, source) = seed(&pool).await;
        let host = Uuid::new_v4();
        let digest = platform::sha256_hex(host.as_bytes());
        knowledge::insert_document(
            &pool,
            knowledge::NewDocument {
                id: host,
                product_version_id: vid,
                title: "host",
                file_name: "host.txt",
                file_size: 1,
                file_hash: &digest,
                object_ref: &format!("objects/{digest}"),
            },
        )
        .await
        .unwrap();
        knowledge::set_parse_status(&pool, host, knowledge::ParseStatus::Processing, "")
            .await
            .unwrap();
        workflow::put(&pool, host, vid, 1, Work::Wiki)
            .await
            .unwrap();
        let page = knowledge::WikiPage {
            id: Uuid::new_v4(),
            product_version_id: vid,
            slug: "shared".into(),
            title: "shared".into(),
            content: "stale source text".into(),
            page_type: "entity".into(),
            status: knowledge::WikiStatus::Published,
            summary: String::new(),
            aliases: vec![],
            source_refs: vec![source, host],
            chunk_refs: vec![],
            category_path: vec![],
            folder_id: None,
        };
        if tombstone {
            knowledge::set_parse_status(&pool, source, knowledge::ParseStatus::Deleting, "")
                .await
                .unwrap();
        } else {
            workflow::retry_document(&pool, source).await.unwrap();
            knowledge::set_parse_status(&pool, source, knowledge::ParseStatus::Processing, "")
                .await
                .unwrap();
            workflow::publish_empty(&pool, source, 2).await.unwrap();
        }
        let result = knowledge::persist_wiki_changes_atomic(
            &pool,
            vid,
            &[page],
            &[],
            &[],
            &[],
            &["shared".into()],
            &[],
            &[],
            &[(source, 1), (host, 1)],
            Some((host, 1, "wiki")),
        )
        .await;
        assert!(
            result.is_err(),
            "stale source snapshot must fail before publication"
        );
        assert!(workflow::pending(&pool, host, 1, "wiki").await.unwrap());
        let pages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM knowledge_wiki_pages WHERE product_version_id=$1",
        )
        .bind(vid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pages, 0);
    }
}
