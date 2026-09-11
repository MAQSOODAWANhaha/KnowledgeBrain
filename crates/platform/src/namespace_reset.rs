//! Read-only reset preflight. These observations alone do not
//! authorize deletion: runtime drain, other backends and durable checkpointing
//! must be verified by the reset coordinator before any destructive operation.
use crate::{DeploymentNamespaceV1, SchemaReceiptV1, SchemaRuntimeIdentity};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};
use std::{
    fs::File,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const CONFIRM_DOMAIN: &[u8] = b"KB:NamespaceReset:v1\0";

#[derive(Debug, thiserror::Error)]
pub enum NamespaceResetError {
    #[error("reset namespace label is invalid")]
    Namespace,
    #[error("reset schema revision is invalid")]
    Revision,
    #[error("reset confirmation checksum does not match")]
    Confirmation,
    #[error("reset request differs from the verified release identity")]
    Identity,
    #[error("connected PostgreSQL database does not match the deployment mapping")]
    DatabaseMapping,
    #[error("target PostgreSQL database still has other sessions")]
    DatabaseSessions,
    #[error("reset object root must be an absolute directory below the filesystem root")]
    ObjectRoot,
    #[error("object reset preflight could not safely open the configured directories")]
    ObjectDirectory(#[source] std::io::Error),
    #[error("Redis reset preflight failed")]
    Redis(#[source] redis::RedisError),
    #[error("Redis reset preflight timed out")]
    RedisTimeout,
    #[error("Redis reset address or observed database/server identity is invalid")]
    RedisMapping,
    #[error("MinIO reset preflight failed: {0}")]
    Minio(String),
    #[error("PostgreSQL reset preflight query failed")]
    Sql(#[from] sqlx::Error),
    #[error("{0}")]
    Schema(#[from] crate::SchemaError),
}

/// Confirmation is a deterministic checksum of the reviewed target, not an
/// authentication credential or permission to bypass any later preflight gate.
pub fn namespace_reset_confirmation(
    namespace: DeploymentNamespaceV1,
    revision: &str,
) -> Result<String, NamespaceResetError> {
    crate::release::require_revision(revision).map_err(|_| NamespaceResetError::Revision)?;
    let mut hash = Sha256::new();
    hash.update(CONFIRM_DOMAIN);
    hash.update(namespace.storage_label().as_bytes());
    hash.update(b"\0");
    hash.update(revision.as_bytes());
    hash.update(b"\0");
    hash.update(namespace.postgres_database().as_bytes());
    Ok(hex::encode_upper(hash.finalize()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceResetRequest {
    namespace: DeploymentNamespaceV1,
    revision: String,
}

impl NamespaceResetRequest {
    pub fn new(
        label: &str,
        revision: &str,
        confirmation: &str,
    ) -> Result<Self, NamespaceResetError> {
        let namespace = DeploymentNamespaceV1::from_storage_label(label)
            .map_err(|_| NamespaceResetError::Namespace)?;
        let expected = namespace_reset_confirmation(namespace, revision)?;
        if confirmation != expected {
            return Err(NamespaceResetError::Confirmation);
        }
        Ok(Self {
            namespace,
            revision: revision.into(),
        })
    }

    pub fn namespace(&self) -> DeploymentNamespaceV1 {
        self.namespace
    }

    pub fn schema_revision(&self) -> &str {
        &self.revision
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NamespaceResetDirectoryIdentity {
    pub device: u64,
    pub inode: u64,
}

impl NamespaceResetDirectoryIdentity {
    fn observe(directory: &File) -> std::io::Result<Self> {
        let metadata = directory.metadata()?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

/// A read-only mapping observation. Not a deletion capability: the coordinator
/// must still verify drain/receipt/checkpoint and re-open/recheck directories at
/// use time. In particular these numeric identities do not pin future paths.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NamespaceResetObjectsPreflight {
    pub namespace: DeploymentNamespaceV1,
    pub configured_root: PathBuf,
    pub root: NamespaceResetDirectoryIdentity,
    pub namespace_directory: Option<NamespaceResetDirectoryIdentity>,
    pub objects_directory: Option<NamespaceResetDirectoryIdentity>,
}

/// A successfully authenticated bucket HEAD, bound to one captured connection
/// configuration. S3 does not expose a bucket instance UUID in this operation;
/// this mapping alone cannot detect bucket replacement or authorize deletion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NamespaceResetMinioPreflight {
    pub namespace: DeploymentNamespaceV1,
    pub endpoint: String,
    pub bucket: String,
    pub signing_region: String,
    pub prefix: String,
}

/// Both optional endpoint/bucket settings absent means no MinIO backend.
/// Partial configuration or an inaccessible bucket must fail, never create it.
pub fn verify_namespace_reset_minio(
    request: &NamespaceResetRequest,
    timeout: std::time::Duration,
) -> Result<Option<NamespaceResetMinioPreflight>, NamespaceResetError> {
    crate::s3::reset_preflight(request.namespace, timeout).map_err(NamespaceResetError::Minio)
}

pub fn verify_namespace_reset_objects(
    root: &Path,
    request: &NamespaceResetRequest,
) -> Result<NamespaceResetObjectsPreflight, NamespaceResetError> {
    if !root.is_absolute() || root == Path::new("/") || root.to_str().is_none() {
        return Err(NamespaceResetError::ObjectRoot);
    }
    let store = crate::object_store::LocalObjectStore::new(root.into(), request.namespace)
        .map_err(NamespaceResetError::ObjectDirectory)?;
    let (directory, namespace, objects) = store
        .reset_directories()
        .map_err(NamespaceResetError::ObjectDirectory)?;
    let observe = |directory: &File| {
        NamespaceResetDirectoryIdentity::observe(directory)
            .map_err(NamespaceResetError::ObjectDirectory)
    };
    Ok(NamespaceResetObjectsPreflight {
        namespace: request.namespace,
        configured_root: root.into(),
        root: observe(&directory)?,
        namespace_directory: namespace.as_ref().map(observe).transpose()?,
        objects_directory: objects.as_ref().map(observe).transpose()?,
    })
}

/// Only connection location is persisted, never Redis authentication settings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum NamespaceResetRedisAddress {
    Tcp { host: String, port: u16 },
    Tls { host: String, port: u16 },
    Unix { path: PathBuf },
}

impl NamespaceResetRedisAddress {
    fn from_client(client: &redis::Client) -> Result<Self, NamespaceResetError> {
        match client.get_connection_info().addr() {
            redis::ConnectionAddr::Tcp(host, port) => Ok(Self::Tcp {
                host: host.clone(),
                port: *port,
            }),
            redis::ConnectionAddr::TcpTls {
                host,
                port,
                insecure: false,
                ..
            } => Ok(Self::Tls {
                host: host.clone(),
                port: *port,
            }),
            redis::ConnectionAddr::Unix(path) if path.is_absolute() && path.to_str().is_some() => {
                Ok(Self::Unix { path: path.clone() })
            }
            _ => Err(NamespaceResetError::RedisMapping),
        }
    }
}

/// Observation from one dedicated connection; not evidence of drained workers.
/// A changed Redis run ID must be re-reviewed by the reset coordinator rather
/// than silently treated as the same server after a restart or failover.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NamespaceResetRedisPreflight {
    pub namespace: DeploymentNamespaceV1,
    pub address: NamespaceResetRedisAddress,
    pub database: i64,
    pub prefix: String,
    pub server_run_id: String,
}

pub async fn verify_namespace_reset_redis(
    client: &redis::Client,
    request: &NamespaceResetRequest,
    timeout: std::time::Duration,
) -> Result<NamespaceResetRedisPreflight, NamespaceResetError> {
    let address = NamespaceResetRedisAddress::from_client(client)?;
    let expected_database = client.get_connection_info().redis_settings().db();
    tokio::time::timeout(timeout, async {
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(NamespaceResetError::Redis)?;
        let server: redis::InfoDict = redis::cmd("INFO")
            .arg("server")
            .query_async(&mut connection)
            .await
            .map_err(NamespaceResetError::Redis)?;
        let replication: redis::InfoDict = redis::cmd("INFO")
            .arg("replication")
            .query_async(&mut connection)
            .await
            .map_err(NamespaceResetError::Redis)?;
        let info: String = redis::cmd("CLIENT")
            .arg("INFO")
            .query_async(&mut connection)
            .await
            .map_err(NamespaceResetError::Redis)?;
        // Read the selected database from the actual connection. A URL default
        // or a successful PING cannot establish which database would be reset.
        let databases: Vec<_> = info
            .split_ascii_whitespace()
            .filter_map(|field| field.strip_prefix("db="))
            .collect();
        let run_id = server
            .get::<String>("run_id")
            .ok_or(NamespaceResetError::RedisMapping)?;
        if databases.len() != 1
            || databases[0].parse::<i64>().ok() != Some(expected_database)
            || expected_database < 0
            || run_id.len() != 40
            || !run_id.bytes().all(|b| b.is_ascii_hexdigit())
            || server.get::<String>("redis_mode").as_deref() != Some("standalone")
            || replication.get::<String>("role").as_deref() != Some("master")
        {
            return Err(NamespaceResetError::RedisMapping);
        }
        Ok(NamespaceResetRedisPreflight {
            namespace: request.namespace,
            address,
            database: expected_database,
            prefix: request.namespace.redis_prefix(),
            server_run_id: run_id,
        })
    })
    .await
    .map_err(|_| NamespaceResetError::RedisTimeout)?
}

/// Observed server identity, not a copy of a database name from a URL. This
/// result deliberately has no Deserialize implementation or delete operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NamespaceResetPostgresPreflight {
    pub database: String,
    pub database_oid: i64,
    pub database_owner: String,
    pub cluster_system_identifier: String,
    pub receipt: SchemaReceiptV1,
}

async fn require_no_other_sessions(
    connection: &mut PgConnection,
    database_oid: i64,
) -> Result<(), NamespaceResetError> {
    sqlx::query("SELECT pg_catalog.pg_stat_clear_snapshot()")
        .execute(&mut *connection)
        .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_catalog.pg_stat_activity
         WHERE datid::bigint=$1 AND pid<>pg_catalog.pg_backend_pid()",
    )
    .bind(database_oid)
    .fetch_one(connection)
    .await?;
    if count != 0 {
        return Err(NamespaceResetError::DatabaseSessions);
    }
    Ok(())
}

/// Use a dedicated connection. The caller must stop runtimes and prevent new
/// connections before deleting; observing zero other sessions is insufficient.
pub async fn verify_namespace_reset_postgres(
    connection: &mut PgConnection,
    request: &NamespaceResetRequest,
    identity: &SchemaRuntimeIdentity,
) -> Result<NamespaceResetPostgresPreflight, NamespaceResetError> {
    if identity.deployment_namespace_id != request.namespace.id()
        || identity.schema_revision() != request.revision
        || identity
            .descriptor
            .sha256()
            .map_err(|_| NamespaceResetError::Identity)?
            != identity.release_descriptor_sha256
    {
        return Err(NamespaceResetError::Identity);
    }
    let mut transaction = connection.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await?;
    let (database, database_oid, database_owner, template): (String, i64, String, bool) =
        sqlx::query_as(
            "SELECT database.datname, database.oid::bigint, owner.rolname, database.datistemplate
             FROM pg_catalog.pg_database database
             JOIN pg_catalog.pg_roles owner ON owner.oid=database.datdba
             WHERE database.datname=current_database()",
        )
        .fetch_one(&mut *transaction)
        .await?;
    if template || database != request.namespace.postgres_database() {
        return Err(NamespaceResetError::DatabaseMapping);
    }
    // Reject a busy target before catalog queries might wait on runtime locks.
    require_no_other_sessions(&mut transaction, database_oid).await?;
    // Reuse the full existing receipt, catalog, extension and server verifier.
    let receipt = crate::verify_runtime_schema_on_connection(&mut transaction, identity).await?;
    let cluster_system_identifier: String =
        sqlx::query_scalar("SELECT system_identifier::text FROM pg_catalog.pg_control_system()")
            .fetch_one(&mut *transaction)
            .await?;
    require_no_other_sessions(&mut transaction, database_oid).await?;
    transaction.commit().await?;
    Ok(NamespaceResetPostgresPreflight {
        database,
        database_oid,
        database_owner,
        cluster_system_identifier,
        receipt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    struct ObjectsFixture(PathBuf);
    impl ObjectsFixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("kb-reset-object-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn request(&self) -> NamespaceResetRequest {
            let namespace: DeploymentNamespaceV1 =
                uuid::Uuid::new_v4().to_string().parse().unwrap();
            let token = namespace_reset_confirmation(namespace, "test-v1").unwrap();
            NamespaceResetRequest::new(&namespace.storage_label(), "test-v1", &token).unwrap()
        }
    }
    impl Drop for ObjectsFixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn object_preflight_observes_real_identity_and_detects_replaced_directories() {
        let fixture = ObjectsFixture::new();
        let request = fixture.request();
        let root = fixture.0.join("root");
        let namespace = root.join(request.namespace.storage_label());
        let objects = namespace.join("objects");
        fs::create_dir_all(&objects).unwrap();
        fs::write(objects.join("sentinel"), b"untouched").unwrap();
        let foreign = root.join(fixture.request().namespace.storage_label());
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("sentinel"), b"foreign").unwrap();
        let first = verify_namespace_reset_objects(&root, &request).unwrap();
        assert_eq!(
            first,
            verify_namespace_reset_objects(&root, &request).unwrap()
        );
        assert_eq!(first.namespace, request.namespace);
        assert_eq!(first.root.inode, fs::metadata(&root).unwrap().ino());
        assert_eq!(
            first.objects_directory.as_ref().unwrap().inode,
            fs::metadata(&objects).unwrap().ino()
        );
        assert_eq!(
            first.objects_directory.as_ref().unwrap().device,
            fs::metadata(&objects).unwrap().dev()
        );
        let retained = root.join("retained-test-directory");
        fs::rename(&namespace, &retained).unwrap();
        fs::create_dir_all(&objects).unwrap();
        let replacement = verify_namespace_reset_objects(&root, &request).unwrap();
        assert_eq!(first.root, replacement.root);
        assert_ne!(first.namespace_directory, replacement.namespace_directory);
        assert_ne!(first.objects_directory, replacement.objects_directory);
        assert_eq!(
            fs::read(retained.join("objects/sentinel")).unwrap(),
            b"untouched"
        );
        assert_eq!(fs::read(foreign.join("sentinel")).unwrap(), b"foreign");
    }

    #[test]
    fn object_preflight_distinguishes_missing_children_without_creating_them() {
        let fixture = ObjectsFixture::new();
        let request = fixture.request();
        let root = fixture.0.join("root");
        assert!(verify_namespace_reset_objects(&root, &request).is_err());
        assert!(!root.exists());
        fs::create_dir(&root).unwrap();
        let missing = verify_namespace_reset_objects(&root, &request).unwrap();
        assert!(missing.namespace_directory.is_none() && missing.objects_directory.is_none());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        let namespace = root.join(request.namespace.storage_label());
        fs::create_dir(&namespace).unwrap();
        let missing = verify_namespace_reset_objects(&root, &request).unwrap();
        assert!(missing.namespace_directory.is_some() && missing.objects_directory.is_none());
        assert_eq!(fs::read_dir(&namespace).unwrap().count(), 0);
        fs::write(namespace.join("objects"), b"not a directory").unwrap();
        assert!(verify_namespace_reset_objects(&root, &request).is_err());
        for invalid in [
            Path::new("/"),
            Path::new("relative"),
            Path::new("/tmp/../tmp"),
        ] {
            assert!(verify_namespace_reset_objects(invalid, &request).is_err());
        }
    }

    #[test]
    fn object_preflight_rejects_symlinks_at_each_directory_boundary() {
        for level in 0..3 {
            let fixture = ObjectsFixture::new();
            let request = fixture.request();
            let root = fixture.0.join("root");
            let namespace = root.join(request.namespace.storage_label());
            let objects = namespace.join("objects");
            fs::create_dir_all(&objects).unwrap();
            let outside = fixture.0.join("outside");
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("sentinel"), b"protected").unwrap();
            let path = [&root, &namespace, &objects][level];
            fs::rename(path, fixture.0.join("retained")).unwrap();
            symlink(&outside, path).unwrap();
            assert!(verify_namespace_reset_objects(&root, &request).is_err());
            assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"protected");
            assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        }
    }

    #[test]
    fn reset_confirmation_binds_exact_label_revision_and_database() {
        let namespace: DeploymentNamespaceV1 =
            "a4598c31-6184-4f75-9ee2-4a0a2783d803".parse().unwrap();
        let label = namespace.storage_label();
        let token = namespace_reset_confirmation(namespace, "test-v1").unwrap();
        // Independent SHA-256 test vector for the documented byte sequence.
        assert_eq!(
            token,
            "DC52C72F1B4AD32A0F0D5E895B7A5DF7E846A58B83FF80EF701C927E1AE71E08"
        );
        assert_eq!(
            namespace.postgres_database(),
            "kb_a4598c3161844f759ee24a0a2783d803"
        );
        let request = NamespaceResetRequest::new(&label, "test-v1", &token).unwrap();
        assert_eq!(request.namespace(), namespace);
        assert_eq!(request.schema_revision(), "test-v1");
        for invalid in [
            namespace.to_string(),
            label.to_uppercase(),
            format!("{label}/"),
            format!(" {label}"),
            format!("{label}\n"),
            format!("kb-{}", namespace),
        ] {
            assert!(NamespaceResetRequest::new(&invalid, "test-v1", &token).is_err());
        }
        for invalid in [
            token.to_lowercase(),
            format!(" {token}"),
            format!("{token}\n"),
            String::new(),
        ] {
            assert!(NamespaceResetRequest::new(&label, "test-v1", &invalid).is_err());
        }
        assert!(NamespaceResetRequest::new(&label, "test-v2", &token).is_err());
        let other = uuid::Uuid::new_v4()
            .to_string()
            .parse::<DeploymentNamespaceV1>()
            .unwrap();
        assert!(NamespaceResetRequest::new(&other.storage_label(), "test-v1", &token).is_err());
        for invalid in ["", "bad revision", "a\0b", "../revision"] {
            assert!(namespace_reset_confirmation(namespace, invalid).is_err());
        }
    }
}
