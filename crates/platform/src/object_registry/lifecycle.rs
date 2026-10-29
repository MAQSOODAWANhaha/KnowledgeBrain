use super::{ObjectDeletionIdentity, ObjectState};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

fn invalid(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.into())
}

async fn lock_object(
    conn: &mut PgConnection,
    object_ref: &str,
) -> Result<(String, String, i64, ObjectState), sqlx::Error> {
    sqlx::query_as(
        "SELECT content_sha256::text, media_type, byte_length, state FROM platform_objects
         WHERE object_ref=$1 FOR UPDATE",
    )
    .bind(object_ref)
    .fetch_one(conn)
    .await
}

fn check_object(
    row: &(String, String, i64, ObjectState),
    digest: &str,
    media_type: &str,
    byte_length: i64,
) -> Result<(), sqlx::Error> {
    if row.0 != digest
        || row.1 != media_type
        || row.2 != byte_length
        || row.3 != ObjectState::Available
    {
        return Err(invalid(
            "object registry identity mismatch or object unavailable",
        ));
    }
    Ok(())
}

/// Register bytes owned by an internal producer. The caller must use a
/// transaction and must already have authorized the business owner.
#[allow(clippy::too_many_arguments)]
pub async fn add_object_reference(
    conn: &mut PgConnection,
    object_ref: &str,
    digest: &str,
    media_type: &str,
    byte_length: i64,
    owner_kind: &str,
    owner_id: Uuid,
    occurrence: &str,
) -> Result<(), sqlx::Error> {
    // The insert serializes the first registration too, when there is no row
    // for SELECT FOR UPDATE yet. Deleted registry rows are never removed.
    sqlx::query(
        "INSERT INTO platform_objects(object_ref,content_sha256,media_type,byte_length,state)
         VALUES($1,$2,$3,$4,$5) ON CONFLICT (object_ref) DO NOTHING",
    )
    .bind(object_ref)
    .bind(digest)
    .bind(media_type)
    .bind(byte_length)
    .bind(ObjectState::Available)
    .execute(&mut *conn)
    .await?;
    let registry = lock_object(conn, object_ref).await?;
    let deleted: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM platform_object_deletion_receipts WHERE object_ref=$1)",
    )
    .bind(object_ref)
    .fetch_one(&mut *conn)
    .await?;
    if deleted {
        return Err(invalid("deleted object digest cannot be revived"));
    }
    check_object(&registry, digest, media_type, byte_length)?;
    sqlx::query(
        "INSERT INTO platform_object_references(object_ref,owner_kind,owner_id,occurrence)
         VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(object_ref)
    .bind(owner_kind)
    .bind(owner_id)
    .bind(occurrence)
    .execute(&mut *conn)
    .await?;
    let matches: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM platform_object_references
         WHERE object_ref=$1 AND owner_kind=$2 AND owner_id=$3 AND occurrence=$4)",
    )
    .bind(object_ref)
    .bind(owner_kind)
    .bind(owner_id)
    .bind(occurrence)
    .fetch_one(conn)
    .await?;
    if !matches {
        return Err(invalid(
            "object owner occurrence already references another object",
        ));
    }
    Ok(())
}

/// Remove a reference in the caller's business transaction. The final owner
/// creates a durable deletion identity while holding the registry row lock.
pub async fn release_object_reference(
    conn: &mut PgConnection,
    object_ref: &str,
    owner_kind: &str,
    owner_id: Uuid,
    occurrence: &str,
    deletion_id: Uuid,
) -> Result<Option<ObjectDeletionIdentity>, sqlx::Error> {
    let registry = lock_object(conn, object_ref).await?;
    sqlx::query(
        "DELETE FROM platform_object_references
         WHERE object_ref=$1 AND owner_kind=$2 AND owner_id=$3 AND occurrence=$4",
    )
    .bind(object_ref)
    .bind(owner_kind)
    .bind(owner_id)
    .bind(occurrence)
    .execute(&mut *conn)
    .await?;
    let transitioned = sqlx::query(
        "UPDATE platform_objects SET state=$2,deleting_at=clock_timestamp()
         WHERE object_ref=$1 AND state='available' AND NOT EXISTS(
             SELECT 1 FROM platform_object_references WHERE object_ref=$1)",
    )
    .bind(object_ref)
    .bind(ObjectState::Deleting)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if transitioned != 0 {
        sqlx::query(
            "INSERT INTO platform_object_deletion_requests(id,object_ref,content_sha256,byte_length)
             VALUES($1,$2,$3,$4)",
        )
        .bind(deletion_id)
        .bind(object_ref)
        .bind(&registry.0)
        .bind(registry.2)
        .execute(&mut *conn)
        .await?;
    }
    let deletion = load_deletion(conn, deletion_id).await?;
    if let Some(value) = &deletion
        && (value.object_ref != object_ref
            || value.digest != registry.0
            || value.byte_length != registry.2)
    {
        return Err(invalid("object deletion identity mismatch"));
    }
    Ok(deletion)
}

async fn load_deletion(
    conn: &mut PgConnection,
    deletion_id: Uuid,
) -> Result<Option<ObjectDeletionIdentity>, sqlx::Error> {
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT object_ref::text,content_sha256::text,byte_length FROM platform_object_deletion_requests WHERE id=$1",
    )
    .bind(deletion_id)
    .fetch_optional(conn)
    .await?;
    Ok(
        row.map(|(object_ref, digest, byte_length)| ObjectDeletionIdentity {
            deletion_id,
            object_ref,
            digest,
            byte_length,
        }),
    )
}

pub async fn stage_object_upload(
    pool: &PgPool,
    staging_id: Uuid,
    object_ref: &str,
    digest: &str,
    media_type: &str,
    byte_length: i64,
    owner_user_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    add_object_reference(
        &mut tx,
        object_ref,
        digest,
        media_type,
        byte_length,
        "object_upload_staging",
        staging_id,
        "payload",
    )
    .await?;
    sqlx::query(
        "INSERT INTO platform_object_uploads(id,object_ref,owner_user_id)
         VALUES($1,$2,$3) ON CONFLICT (id) DO NOTHING",
    )
    .bind(staging_id)
    .bind(object_ref)
    .bind(owner_user_id)
    .execute(&mut *tx)
    .await?;
    let matches: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM platform_object_uploads
         WHERE id=$1 AND object_ref=$2 AND owner_user_id IS NOT DISTINCT FROM $3::uuid)",
    )
    .bind(staging_id)
    .bind(object_ref)
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await?;
    if !matches {
        return Err(invalid("object upload staging identity mismatch"));
    }
    tx.commit().await
}

/// Publish only staged bytes or an already published object. Keep this call in
/// the same transaction as the business record that owns the reference.
#[allow(clippy::too_many_arguments)]
pub async fn publish_object_reference(
    conn: &mut PgConnection,
    staging_id: Option<Uuid>,
    object_ref: &str,
    digest: &str,
    media_type: &str,
    byte_length: i64,
    owner_kind: &str,
    owner_id: Uuid,
    occurrence: &str,
    owner_user_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    if let Some(staging_id) = staging_id {
        let (reference, owner, expired): (String, Option<Uuid>, bool) = sqlx::query_as(
            "SELECT object_ref::text,owner_user_id,expires_at<=clock_timestamp()
             FROM platform_object_uploads WHERE id=$1 FOR UPDATE",
        )
        .bind(staging_id)
        .fetch_one(&mut *conn)
        .await?;
        if expired {
            return Err(invalid("object upload staging lease expired"));
        }
        if reference != object_ref || owner != owner_user_id {
            return Err(invalid("object upload staging owner mismatch"));
        }
    }
    let registry = lock_object(conn, object_ref).await?;
    check_object(&registry, digest, media_type, byte_length)?;
    if staging_id.is_none() {
        let published: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM platform_object_references
             WHERE object_ref=$1 AND owner_kind<>'object_upload_staging')",
        )
        .bind(object_ref)
        .fetch_one(&mut *conn)
        .await?;
        if !published {
            return Err(invalid("object requires a valid upload staging identity"));
        }
    }
    add_object_reference(
        conn,
        object_ref,
        digest,
        media_type,
        byte_length,
        owner_kind,
        owner_id,
        occurrence,
    )
    .await?;
    if let Some(staging_id) = staging_id {
        sqlx::query("DELETE FROM platform_object_uploads WHERE id=$1")
            .bind(staging_id)
            .execute(&mut *conn)
            .await?;
        release_object_reference(
            conn,
            object_ref,
            "object_upload_staging",
            staging_id,
            "payload",
            staging_id,
        )
        .await?;
    }
    Ok(())
}

pub async fn abandon_object_upload(
    pool: &PgPool,
    staging_id: Uuid,
    owner_user_id: Option<Uuid>,
) -> Result<Option<ObjectDeletionIdentity>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT object_ref::text,owner_user_id FROM platform_object_uploads WHERE id=$1 FOR UPDATE",
    )
    .bind(staging_id)
    .fetch_optional(&mut *tx)
    .await?;
    let deletion = if let Some((reference, owner)) = row {
        if owner != owner_user_id {
            return Err(invalid("object upload staging owner mismatch"));
        }
        sqlx::query("DELETE FROM platform_object_uploads WHERE id=$1")
            .bind(staging_id)
            .execute(&mut *tx)
            .await?;
        release_object_reference(
            &mut tx,
            &reference,
            "object_upload_staging",
            staging_id,
            "payload",
            staging_id,
        )
        .await?
    } else {
        load_deletion(&mut tx, staging_id).await?
    };
    tx.commit().await?;
    Ok(deletion)
}

/// Retain the staging row as durable work until the cleanup worker consumes it.
pub async fn request_object_upload_expiry(
    pool: &PgPool,
    staging_id: Uuid,
    owner_user_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let owner: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT owner_user_id FROM platform_object_uploads WHERE id=$1 FOR UPDATE",
    )
    .bind(staging_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(owner) = owner {
        if owner != owner_user_id {
            return Err(invalid("object upload staging owner mismatch"));
        }
        sqlx::query(
            "UPDATE platform_object_uploads
             SET expires_at=GREATEST(clock_timestamp(),created_at+interval '1 microsecond')
             WHERE id=$1",
        )
        .bind(staging_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

pub async fn expired_object_upload_candidates(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM platform_object_uploads WHERE expires_at<=clock_timestamp()
         ORDER BY expires_at,id LIMIT 100",
    )
    .fetch_all(pool)
    .await
}

pub async fn expire_one_object_upload(
    pool: &PgPool,
    staging_id: Uuid,
) -> Result<Option<ObjectDeletionIdentity>, String> {
    async fn expire(
        pool: &PgPool,
        staging_id: Uuid,
    ) -> Result<Option<ObjectDeletionIdentity>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let staging: Option<(String, bool)> = sqlx::query_as(
            "SELECT object_ref::text,expires_at<=clock_timestamp()
             FROM platform_object_uploads WHERE id=$1 FOR UPDATE",
        )
        .bind(staging_id)
        .fetch_optional(&mut *tx)
        .await?;
        let deletion = match staging {
            Some((reference, true)) => {
                sqlx::query("DELETE FROM platform_object_uploads WHERE id=$1")
                    .bind(staging_id)
                    .execute(&mut *tx)
                    .await?;
                release_object_reference(
                    &mut tx,
                    &reference,
                    "object_upload_staging",
                    staging_id,
                    "payload",
                    staging_id,
                )
                .await?
            }
            Some((_, false)) => None,
            None => load_deletion(&mut tx, staging_id).await?,
        };
        tx.commit().await?;
        Ok(deletion)
    }
    expire(pool, staging_id)
        .await
        .map_err(|error| error.to_string())
}

/// Check immutable deletion identity and current ownership before touching
/// physical storage. `true` means a matching completion already exists.
pub async fn object_deletion_preflight(
    conn: &mut PgConnection,
    deletion: &ObjectDeletionIdentity,
) -> Result<bool, sqlx::Error> {
    let registry = lock_object(conn, &deletion.object_ref).await?;
    let durable = load_deletion(conn, deletion.deletion_id).await?;
    if durable.as_ref() != Some(deletion)
        || registry.0 != deletion.digest
        || registry.2 != deletion.byte_length
    {
        return Err(invalid("object deletion business fence is not current"));
    }
    let completed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM platform_object_deletion_receipts
         WHERE deletion_id=$1 AND object_ref=$2 AND content_sha256=$3 AND byte_length=$4)",
    )
    .bind(deletion.deletion_id)
    .bind(&deletion.object_ref)
    .bind(&deletion.digest)
    .bind(deletion.byte_length)
    .fetch_one(&mut *conn)
    .await?;
    if completed {
        return Ok(true);
    }
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM platform_object_references WHERE object_ref=$1)
         OR EXISTS(SELECT 1 FROM platform_object_uploads WHERE object_ref=$1 AND expires_at>clock_timestamp())",
    ).bind(&deletion.object_ref).fetch_one(conn).await?;
    if registry.3 != ObjectState::Deleting || owned {
        return Err(invalid("object deletion business fence is not current"));
    }
    Ok(false)
}

/// Record completion only after physical deletion. The caller holds a transaction.
pub async fn complete_object_deletion(
    conn: &mut PgConnection,
    deletion: &ObjectDeletionIdentity,
) -> Result<(), sqlx::Error> {
    if object_deletion_preflight(conn, deletion).await? {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO platform_object_deletion_receipts(object_ref,content_sha256,byte_length,deletion_id,deleted_at)
         VALUES($1,$2,$3,$4,clock_timestamp())",
    ).bind(&deletion.object_ref).bind(&deletion.digest).bind(deletion.byte_length)
        .bind(deletion.deletion_id).execute(&mut *conn).await?;
    sqlx::query(
        "UPDATE platform_objects SET state=$2,deleted_at=clock_timestamp() WHERE object_ref=$1",
    )
    .bind(&deletion.object_ref)
    .bind(ObjectState::Deleted)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn process_object_deletion(
    pool: &PgPool,
    deletion: &ObjectDeletionIdentity,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let completed = object_deletion_preflight(&mut tx, deletion)
        .await
        .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())?;
    if completed {
        return Ok(());
    }
    crate::delete_retained_blob(&deletion.digest).await?;
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    complete_object_deletion(&mut tx, deletion)
        .await
        .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())
}
