//! Object lifecycle transactions against an explicitly isolated database.

use serde_json::Value;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

const REQUIRE_ENV: &str = "KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS";
const URL_ENV: &str = "KNOWLEDGEBRAIN_OBJECT_TEST_DATABASE_URL";

fn postgres_tests_required() -> bool {
    std::env::var(REQUIRE_ENV)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn database_url() -> Option<String> {
    match std::env::var(URL_ENV) {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ if postgres_tests_required() => panic!("{REQUIRE_ENV}=1 requires {URL_ENV}"),
        _ => {
            eprintln!("skip object registry concurrent: {URL_ENV} is not configured");
            None
        }
    }
}

async fn connect_pool() -> Option<PgPool> {
    let url = database_url()?;
    let options: sqlx::postgres::PgConnectOptions = url.parse().expect("test database URL");
    assert_eq!(options.get_host(), "127.0.0.1");
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("knowledgebrain_test_"))
    );
    match sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
    {
        Ok(pool) => Some(pool),
        Err(error) if postgres_tests_required() => {
            panic!("required runtime test object_registry_concurrent postgres is down: {error}");
        }
        Err(error) => {
            eprintln!("skip object registry concurrent: postgres not up ({error})");
            None
        }
    }
}

async fn fresh_user(pool: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
        .bind(id)
        .bind(format!("object-owner-{id}@example.invalid"))
        .execute(pool)
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn storage_states_decode_text_and_reject_unknown_values() {
    let Some(pool) = connect_pool().await else {
        return;
    };
    for state in [
        platform::ObjectState::Available,
        platform::ObjectState::Deleting,
        platform::ObjectState::Deleted,
    ] {
        let actual: platform::ObjectState = sqlx::query_scalar("SELECT $1::text")
            .bind(state)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(actual, state);
    }
    for mode in [
        platform::MaintenanceMode::Open,
        platform::MaintenanceMode::Maintenance,
        platform::MaintenanceMode::Draining,
        platform::MaintenanceMode::Rollback,
    ] {
        let actual: platform::MaintenanceMode = sqlx::query_scalar("SELECT $1::text")
            .bind(mode)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(actual, mode);
    }
    assert!(
        sqlx::query_scalar::<_, platform::ObjectState>("SELECT 'pending'::text")
            .fetch_one(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query_scalar::<_, platform::MaintenanceMode>("SELECT 'unknown'::text")
            .fetch_one(&pool)
            .await
            .is_err()
    );
}

struct TestObject<'a> {
    reference: &'a str,
    digest: &'a str,
    byte_length: i64,
}

async fn add_owner(
    pool: &PgPool,
    object: &TestObject<'_>,
    owner_kind: &str,
    owner_id: Uuid,
    occurrence: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    platform::add_object_reference(
        &mut tx,
        object.reference,
        object.digest,
        "application/octet-stream",
        object.byte_length,
        owner_kind,
        owner_id,
        occurrence,
    )
    .await?;
    tx.commit().await
}

async fn remove_owner(
    pool: &PgPool,
    object_ref: &str,
    owner_kind: &str,
    owner_id: Uuid,
    occurrence: &str,
    deletion_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = platform::release_object_reference(
        &mut tx,
        object_ref,
        owner_kind,
        owner_id,
        occurrence,
        deletion_id,
    )
    .await?;
    tx.commit().await?;
    result
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| sqlx::Error::Encode(Box::new(error)))
}

#[tokio::test]
async fn concurrent_add_and_partial_remove_keeps_remaining_owner() {
    let Some(pool) = connect_pool().await else {
        return;
    };

    let digest = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let object_ref = format!("objects/{digest}");
    let byte_length = 16_i64;
    let object = TestObject {
        reference: &object_ref,
        digest: &digest,
        byte_length,
    };
    let owner_kind = "concurrent_owner";
    let owner_seed = Uuid::new_v4();
    let owner_a = Uuid::new_v4();
    let owner_b = Uuid::new_v4();
    add_owner(&pool, &object, owner_kind, owner_seed, "payload")
        .await
        .expect("seed owner so FOR UPDATE hits an existing registry row");

    let (add_a, add_b) = tokio::join!(
        add_owner(&pool, &object, owner_kind, owner_a, "payload"),
        add_owner(&pool, &object, owner_kind, owner_b, "payload"),
    );
    add_a.expect("concurrent add owner a");
    add_b.expect("concurrent add owner b");

    let owners_after_add: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_object_references
         WHERE object_ref=$1::kb_object_ref",
    )
    .bind(&object_ref)
    .fetch_one(&pool)
    .await
    .expect("count owners after add");
    assert_eq!(owners_after_add, 3);

    let remove_a_first = Uuid::new_v4();
    let remove_a_second = Uuid::new_v4();
    let (remove_first, remove_second) = tokio::join!(
        remove_owner(
            &pool,
            &object_ref,
            owner_kind,
            owner_a,
            "payload",
            remove_a_first,
        ),
        remove_owner(
            &pool,
            &object_ref,
            owner_kind,
            owner_a,
            "payload",
            remove_a_second,
        ),
    );
    assert_eq!(
        remove_first.expect("concurrent remove owner a first"),
        None,
        "remaining owner must suppress deletion identity"
    );
    assert_eq!(
        remove_second.expect("concurrent remove owner a second"),
        None,
        "remaining owner must suppress deletion identity"
    );

    let state: String =
        sqlx::query_scalar("SELECT state FROM platform_objects WHERE object_ref=$1::kb_object_ref")
            .bind(&object_ref)
            .fetch_one(&pool)
            .await
            .expect("registry state after partial remove");
    assert_eq!(state, "available");

    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_object_references
         WHERE object_ref=$1::kb_object_ref AND owner_id=$2",
    )
    .bind(&object_ref)
    .bind(owner_b)
    .fetch_one(&pool)
    .await
    .expect("remaining owner");
    assert_eq!(remaining, 1);

    let tombstones: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_object_deletion_receipts
         WHERE object_ref=$1::kb_object_ref",
    )
    .bind(&object_ref)
    .fetch_one(&pool)
    .await
    .expect("tombstone count after partial remove");
    assert_eq!(tombstones, 0);

    let extra = remove_owner(
        &pool,
        &object_ref,
        owner_kind,
        owner_b,
        "payload",
        Uuid::new_v4(),
    )
    .await
    .expect("remove extra owner b");
    assert_eq!(extra, None, "seed owner must still suppress deletion");

    let last_deletion_id = Uuid::new_v4();
    let last = remove_owner(
        &pool,
        &object_ref,
        owner_kind,
        owner_seed,
        "payload",
        last_deletion_id,
    )
    .await
    .expect("remove remaining seed owner");
    let deletion = last.expect("last owner remove returns deletion identity");
    assert_eq!(
        deletion.get("deletion_id").and_then(Value::as_str),
        Some(last_deletion_id.to_string()).as_deref()
    );
    assert_eq!(
        deletion.get("object_ref").and_then(Value::as_str),
        Some(object_ref.as_str())
    );
    assert_eq!(
        deletion.get("digest").and_then(Value::as_str),
        Some(digest.as_str())
    );
    assert_eq!(
        deletion.get("byte_length").and_then(Value::as_i64),
        Some(byte_length)
    );

    let state_after: String =
        sqlx::query_scalar("SELECT state FROM platform_objects WHERE object_ref=$1::kb_object_ref")
            .bind(&object_ref)
            .fetch_one(&pool)
            .await
            .expect("registry state after last remove");
    assert_eq!(state_after, "deleting");

    let tombstones_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_object_deletion_receipts
         WHERE object_ref=$1::kb_object_ref",
    )
    .bind(&object_ref)
    .fetch_one(&pool)
    .await
    .expect("tombstone count after last remove");
    assert_eq!(tombstones_after, 0);
}

fn fresh_object() -> (String, String) {
    let digest = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    (format!("objects/{digest}"), digest)
}

#[tokio::test]
async fn concurrent_first_registration_and_conflicting_occurrence_are_atomic() {
    let Some(pool) = connect_pool().await else {
        return;
    };
    let (reference, digest) = fresh_object();
    let object = TestObject {
        reference: &reference,
        digest: &digest,
        byte_length: 8,
    };
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let (a, b) = tokio::join!(
        add_owner(&pool, &object, "test_first", first, "payload"),
        add_owner(&pool, &object, "test_first", second, "payload"),
    );
    a.unwrap();
    b.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_object_references WHERE object_ref=$1")
            .bind(&reference)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
    let (other_ref, other_digest) = fresh_object();
    let other = TestObject {
        reference: &other_ref,
        digest: &other_digest,
        byte_length: 8,
    };
    assert!(
        add_owner(&pool, &other, "test_first", first, "payload")
            .await
            .is_err()
    );
    let orphan: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM platform_objects WHERE object_ref=$1)")
            .bind(other_ref)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !orphan,
        "rejected occurrence must roll back its new registry row"
    );
}

#[tokio::test]
async fn publish_requires_staging_identity_and_rolls_back_with_business_write() {
    let Some(pool) = connect_pool().await else {
        return;
    };
    let (reference, digest) = fresh_object();
    let stage = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let user = fresh_user(&pool).await;
    platform::stage_object_upload(
        &pool,
        stage,
        &reference,
        &digest,
        "application/octet-stream",
        12,
        Some(user),
    )
    .await
    .unwrap();
    for (staging, supplied_user, length) in [
        (None, Some(user), 12),
        (Some(stage), None, 12),
        (Some(stage), Some(fresh_user(&pool).await), 12),
        (Some(stage), Some(user), 13),
    ] {
        let mut tx = pool.begin().await.unwrap();
        assert!(
            platform::publish_object_reference(
                &mut tx,
                staging,
                &reference,
                &digest,
                "application/octet-stream",
                length,
                "test_document",
                owner,
                "original",
                supplied_user
            )
            .await
            .is_err()
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = pool.begin().await.unwrap();
    platform::publish_object_reference(
        &mut tx,
        Some(stage),
        &reference,
        &digest,
        "application/octet-stream",
        12,
        "test_document",
        owner,
        "original",
        Some(user),
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    let staged: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM platform_object_uploads WHERE id=$1)")
            .bind(stage)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        staged,
        "failed business transaction must retain staged ownership"
    );
    let mut tx = pool.begin().await.unwrap();
    platform::publish_object_reference(
        &mut tx,
        Some(stage),
        &reference,
        &digest,
        "application/octet-stream",
        12,
        "test_document",
        owner,
        "original",
        Some(user),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        platform::expire_one_object_upload(&pool, stage)
            .await
            .unwrap(),
        None
    );
    let owners: Vec<String> =
        sqlx::query_scalar("SELECT owner_kind FROM platform_object_references WHERE object_ref=$1")
            .bind(&reference)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(owners, ["test_document"]);
    let mut tx = pool.begin().await.unwrap();
    platform::publish_object_reference(
        &mut tx,
        None,
        &reference,
        &digest,
        "application/octet-stream",
        12,
        "test_document",
        Uuid::new_v4(),
        "original",
        Some(user),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn expiry_retention_replay_and_deleted_digest_fence() {
    let Some(pool) = connect_pool().await else {
        return;
    };
    let (reference, digest) = fresh_object();
    let stage = Uuid::new_v4();
    let user = fresh_user(&pool).await;
    platform::stage_object_upload(
        &pool,
        stage,
        &reference,
        &digest,
        "application/octet-stream",
        9,
        Some(user),
    )
    .await
    .unwrap();
    for supplied_user in [None, Some(fresh_user(&pool).await)] {
        assert!(
            platform::request_object_upload_expiry(&pool, stage, supplied_user)
                .await
                .is_err()
        );
    }
    assert_eq!(
        platform::expire_one_object_upload(&pool, stage)
            .await
            .unwrap(),
        None
    );
    platform::request_object_upload_expiry(&pool, stage, Some(user))
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(
        platform::publish_object_reference(
            &mut tx,
            Some(stage),
            &reference,
            &digest,
            "application/octet-stream",
            9,
            "test_document",
            Uuid::new_v4(),
            "original",
            Some(user)
        )
        .await
        .is_err()
    );
    tx.rollback().await.unwrap();
    let (a, b) = tokio::join!(
        platform::expire_one_object_upload(&pool, stage),
        platform::expire_one_object_upload(&pool, stage)
    );
    let deletion = a.unwrap().unwrap();
    assert_eq!(b.unwrap(), Some(deletion.clone()));
    let mut wrong = deletion.clone();
    wrong.byte_length += 1;
    let mut tx = pool.begin().await.unwrap();
    assert!(
        platform::object_deletion_preflight(&mut tx, &wrong)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    let mismatch = sqlx::query(
        "INSERT INTO platform_object_deletion_receipts
         (object_ref,content_sha256,byte_length,deletion_id,deleted_at)
         VALUES($1,$2,$3,$4,now())",
    )
    .bind(&deletion.object_ref)
    .bind(&deletion.digest)
    .bind(wrong.byte_length)
    .bind(deletion.deletion_id)
    .execute(&pool)
    .await
    .expect_err("a receipt must preserve its request's complete object identity");
    assert_eq!(
        mismatch
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );
    let mut tx = pool.begin().await.unwrap();
    assert!(
        !platform::object_deletion_preflight(&mut tx, &deletion)
            .await
            .unwrap()
    );
    // This fixture has no physical bytes: exercise acknowledgement after the
    // storage adapter has deleted them, without requiring an object-store server.
    platform::complete_object_deletion(&mut tx, &deletion)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(
        platform::object_deletion_preflight(&mut tx, &deletion)
            .await
            .unwrap()
    );
    platform::complete_object_deletion(&mut tx, &deletion)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        platform::stage_object_upload(
            &pool,
            Uuid::new_v4(),
            &reference,
            &digest,
            "application/octet-stream",
            9,
            Some(user)
        )
        .await
        .is_err()
    );
    assert_eq!(
        platform::expire_one_object_upload(&pool, stage)
            .await
            .unwrap(),
        Some(deletion)
    );
}

#[tokio::test]
async fn upload_owner_is_a_real_user_and_cannot_be_replaced_or_abandoned_by_others() {
    let Some(pool) = connect_pool().await else {
        return;
    };
    let (reference, digest) = fresh_object();
    let stage = Uuid::new_v4();
    let error = platform::stage_object_upload(
        &pool,
        stage,
        &reference,
        &digest,
        "application/octet-stream",
        7,
        Some(Uuid::new_v4()),
    )
    .await
    .expect_err("an upload cannot reference an absent user");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM platform_objects WHERE object_ref=$1)")
            .bind(&reference)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !exists,
        "rejected ownership must roll back staged object registration"
    );
    let user = fresh_user(&pool).await;
    for _ in 0..2 {
        platform::stage_object_upload(
            &pool,
            stage,
            &reference,
            &digest,
            "application/octet-stream",
            7,
            Some(user),
        )
        .await
        .unwrap();
    }
    for supplied_user in [None, Some(fresh_user(&pool).await)] {
        assert!(
            platform::stage_object_upload(
                &pool,
                stage,
                &reference,
                &digest,
                "application/octet-stream",
                7,
                supplied_user,
            )
            .await
            .is_err()
        );
        assert!(
            platform::abandon_object_upload(&pool, stage, supplied_user)
                .await
                .is_err()
        );
    }
    let recorded_owner: Option<Uuid> =
        sqlx::query_scalar("SELECT owner_user_id FROM platform_object_uploads WHERE id=$1")
            .bind(stage)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(recorded_owner, Some(user));
    let deletion = platform::abandon_object_upload(&pool, stage, Some(user))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(deletion.object_ref, reference);
}

#[tokio::test]
async fn internal_uploads_use_no_user_without_bypassing_owned_upload_checks() {
    let Some(pool) = connect_pool().await else {
        return;
    };
    let user = fresh_user(&pool).await;
    for abandon in [true, false] {
        let (reference, digest) = fresh_object();
        let stage = Uuid::new_v4();
        platform::stage_object_upload(
            &pool,
            stage,
            &reference,
            &digest,
            "application/octet-stream",
            4,
            None,
        )
        .await
        .unwrap();
        assert!(
            platform::request_object_upload_expiry(&pool, stage, Some(user))
                .await
                .is_err()
        );
        assert!(
            platform::abandon_object_upload(&pool, stage, Some(user))
                .await
                .is_err()
        );
        let deletion = if abandon {
            platform::abandon_object_upload(&pool, stage, None)
                .await
                .unwrap()
        } else {
            platform::request_object_upload_expiry(&pool, stage, None)
                .await
                .unwrap();
            platform::expire_one_object_upload(&pool, stage)
                .await
                .unwrap()
        }
        .unwrap();
        assert_eq!(deletion.object_ref, reference);
    }
}

#[tokio::test]
async fn last_release_racing_new_owner_cannot_delete_owned_object() {
    let Some(pool) = connect_pool().await else {
        return;
    };
    let (reference, digest) = fresh_object();
    let object = TestObject {
        reference: &reference,
        digest: &digest,
        byte_length: 10,
    };
    let old = Uuid::new_v4();
    let new = Uuid::new_v4();
    add_owner(&pool, &object, "test_race", old, "payload")
        .await
        .unwrap();
    let (added, removed) = tokio::join!(
        add_owner(&pool, &object, "test_race", new, "payload"),
        remove_owner(
            &pool,
            &reference,
            "test_race",
            old,
            "payload",
            Uuid::new_v4()
        ),
    );
    let deletion = removed.unwrap();
    let (state, count): (String, i64) = sqlx::query_as(
        "SELECT state,(SELECT count(*) FROM platform_object_references WHERE object_ref=$1)
         FROM platform_objects WHERE object_ref=$1",
    )
    .bind(&reference)
    .fetch_one(&pool)
    .await
    .unwrap();
    if added.is_ok() {
        assert_eq!((state.as_str(), count), ("available", 1));
        assert!(deletion.is_none());
    } else {
        assert_eq!((state.as_str(), count), ("deleting", 0));
        assert!(deletion.is_some());
    }
}
