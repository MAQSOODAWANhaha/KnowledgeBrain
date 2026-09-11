//! Concurrent ObjectRegistry ownership via kb_object_reference_add/remove.

use serde_json::Value;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

const REQUIRE_ENV: &str = "KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS";
const URL_ENV: &str = "DATABASE_URL";

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
    actor: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT kb_object_reference_add(
            $1::kb_object_ref,$2::kb_sha256,'application/octet-stream',$3,
            $4,$5,$6,$7::kb_actor_identity
        )",
    )
    .bind(object.reference)
    .bind(object.digest)
    .bind(object.byte_length)
    .bind(owner_kind)
    .bind(owner_id)
    .bind(occurrence)
    .bind(actor)
    .execute(pool)
    .await?;
    Ok(())
}

async fn remove_owner(
    pool: &PgPool,
    object_ref: &str,
    owner_kind: &str,
    owner_id: Uuid,
    occurrence: &str,
    deletion_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_object_reference_remove($1::kb_object_ref,$2,$3,$4,$5)")
        .bind(object_ref)
        .bind(owner_kind)
        .bind(owner_id)
        .bind(occurrence)
        .bind(deletion_id)
        .fetch_one(pool)
        .await
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
    let actor = "system:knowledge-document-ingest";

    add_owner(&pool, &object, owner_kind, owner_seed, "payload", actor)
        .await
        .expect("seed owner so FOR UPDATE hits an existing registry row");

    let (add_a, add_b) = tokio::join!(
        add_owner(&pool, &object, owner_kind, owner_a, "payload", actor,),
        add_owner(&pool, &object, owner_kind, owner_b, "payload", actor,),
    );
    add_a.expect("concurrent add owner a");
    add_b.expect("concurrent add owner b");

    let owners_after_add: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references
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
        sqlx::query_scalar("SELECT state FROM object_registry WHERE object_ref=$1::kb_object_ref")
            .bind(&object_ref)
            .fetch_one(&pool)
            .await
            .expect("registry state after partial remove");
    assert_eq!(state, "available");

    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references
         WHERE object_ref=$1::kb_object_ref AND owner_id=$2",
    )
    .bind(&object_ref)
    .bind(owner_b)
    .fetch_one(&pool)
    .await
    .expect("remaining owner");
    assert_eq!(remaining, 1);

    let tombstones: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_retention_tombstones
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
        sqlx::query_scalar("SELECT state FROM object_registry WHERE object_ref=$1::kb_object_ref")
            .bind(&object_ref)
            .fetch_one(&pool)
            .await
            .expect("registry state after last remove");
    assert_eq!(state_after, "deleting");

    let tombstones_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_retention_tombstones
         WHERE object_ref=$1::kb_object_ref",
    )
    .bind(&object_ref)
    .fetch_one(&pool)
    .await
    .expect("tombstone count after last remove");
    assert_eq!(tombstones_after, 0);
}
