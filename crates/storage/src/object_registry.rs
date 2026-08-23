use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionClaim {
    pub object_ref: String,
    pub digest: String,
    pub byte_length: i64,
    pub attempt: i32,
    pub claim_token: Uuid,
}

pub async fn register_knowledge_document_object(
    pool: &PgPool,
    document_id: Uuid,
    media_type: &str,
    actor_identity: &str,
    idempotency_key: &str,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_register_knowledge_document_object($1,$2,$3::kb_actor_identity,$4,$5)",
    )
    .bind(document_id)
    .bind(media_type)
    .bind(actor_identity)
    .bind(idempotency_key)
    .bind(Uuid::new_v4())
    .fetch_one(pool)
    .await
}

pub async fn release_knowledge_document_object(
    pool: &PgPool,
    document_id: Uuid,
    actor_identity: &str,
    idempotency_key: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_release_knowledge_document_object($1,$2::kb_actor_identity,$3,$4)",
    )
    .bind(document_id)
    .bind(actor_identity)
    .bind(idempotency_key)
    .bind(Uuid::new_v4())
    .fetch_one(pool)
    .await
}

async fn claim_retention(
    pool: &PgPool,
    worker_name: &str,
    lease_ms: i32,
) -> Result<Option<RetentionClaim>, sqlx::Error> {
    let claim_token = Uuid::new_v4();
    let row = sqlx::query("SELECT * FROM kb_retention_claim($1,$2,$3)")
        .bind(claim_token)
        .bind(worker_name)
        .bind(lease_ms)
        .fetch_optional(pool)
        .await?;
    row.map(|row| {
        Ok(RetentionClaim {
            object_ref: row.try_get("object_ref")?,
            digest: row.try_get("digest")?,
            byte_length: row.try_get("byte_length")?,
            attempt: row.try_get("attempt")?,
            claim_token,
        })
    })
    .transpose()
}

fn bounded_error_code(error: &str) -> &'static str {
    if error.contains("timeout") {
        "OBJECT_DELETE_TIMEOUT"
    } else if error.contains("s3") {
        "OBJECT_STORE_DELETE_FAILED"
    } else {
        "OBJECT_DELETE_FAILED"
    }
}

/// Claim and process at most one retention item. Physical deletion is hidden
/// inside this module and is impossible without a current database claim.
pub async fn process_one_retention_item(
    pool: &PgPool,
    worker_name: &str,
) -> Result<bool, sqlx::Error> {
    let Some(claim) = claim_retention(pool, worker_name, 60_000).await? else {
        return Ok(false);
    };
    match crate::delete_claimed_blob(&claim.digest).await {
        Ok(()) => {
            let completed: bool =
                sqlx::query_scalar("SELECT kb_retention_complete($1::kb_object_ref,$2)")
                    .bind(&claim.object_ref)
                    .bind(claim.claim_token)
                    .fetch_one(pool)
                    .await?;
            if !completed {
                return Err(sqlx::Error::Protocol(
                    "retention completion did not acknowledge the claim".into(),
                ));
            }
            Ok(true)
        }
        Err(error) => {
            let failed: bool =
                sqlx::query_scalar("SELECT kb_retention_fail($1::kb_object_ref,$2,$3)")
                    .bind(&claim.object_ref)
                    .bind(claim.claim_token)
                    .bind(bounded_error_code(&error))
                    .fetch_one(pool)
                    .await?;
            if !failed {
                return Err(sqlx::Error::Protocol(
                    "retention retry did not acknowledge the claim".into(),
                ));
            }
            Ok(true)
        }
    }
}
