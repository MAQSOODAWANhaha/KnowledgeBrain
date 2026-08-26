use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Acquire, PgPool, Postgres, Row, Transaction};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const ALLOWLIST: &str = include_str!("../../../deploy/first-launch/catalog-row-allowlist.toml");
const MIGRATION_MANIFEST: &str =
    include_str!("../../../deploy/first-launch/migration-manifest.toml");
const BOOTSTRAP_AUTHORITY: &str =
    include_str!("../../../deploy/postgres-init/010-runtime-identities.sh");
const BASELINE_SLICES: &[(&str, &str, &str)] = &[
    (
        "knowledge_base_baseline",
        "knowledge_base_baseline.sql",
        include_str!("../../../migrations/knowledge_base_baseline.sql"),
    ),
    (
        "shared_platform_baseline",
        "shared_platform_baseline.sql",
        include_str!("../../../migrations/shared_platform_baseline.sql"),
    ),
    (
        "bidding_v1_baseline",
        "bidding_v1_baseline.sql",
        include_str!("../../../migrations/bidding_v1_baseline.sql"),
    ),
];
const MIGRATION_LOCK_ID: i64 = 0x4b_42_4d_49_47_52_41_54;
const GOVERNED_ROLES: &[&str] = &[
    "kb_app_owner",
    "kb_first_launch_verifier",
    "kb_launch_attestor",
    "kb_launch_ingress",
    "kb_launch_operator",
    "kb_launch_owner",
    "kb_launch_reset_dispatcher",
    "kb_launch_router",
    "kb_launch_signature_verifier",
    "kb_migrator",
    "kb_runtime_api",
    "kb_runtime_retention",
    "kb_runtime_worker",
];

#[derive(Debug, Clone)]
pub struct FreshPretrafficOwnerBindings {
    pub app_owner: String,
    pub bootstrap_owner: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCatalogRows {
    pub allowlist_sha256: String,
    pub catalog_sha256: String,
    pub rows_sha256: String,
}

#[derive(Debug)]
pub struct FirstLaunchVerificationError(String);

impl fmt::Display for FirstLaunchVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FirstLaunchVerificationError {}

impl From<sqlx::Error> for FirstLaunchVerificationError {
    fn from(error: sqlx::Error) -> Self {
        Self(format!(
            "first-launch database verification failed: {error}"
        ))
    }
}

fn failure(message: impl Into<String>) -> FirstLaunchVerificationError {
    FirstLaunchVerificationError(message.into())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    format_version: u32,
    postgres_major: u32,
    baseline_slice_count: i32,
    owner_contract: OwnerContract,
    database: DatabaseEntry,
    database_acls: Vec<DatabaseAclEntry>,
    extensions: Vec<ExtensionEntry>,
    schemas: Vec<SchemaEntry>,
    schema_acls: Vec<SchemaAclEntry>,
    relations: Vec<RelationEntry>,
    columns: Vec<ColumnEntry>,
    constraints: Vec<ConstraintEntry>,
    indexes: Vec<IndexEntry>,
    triggers: Vec<TriggerEntry>,
    routines: Vec<RoutineEntry>,
    roles: Vec<RoleEntry>,
    role_memberships: Vec<RoleMembershipEntry>,
    finalized_role_memberships: Vec<RoleMembershipEntry>,
    relation_acls: Vec<RelationAclEntry>,
    routine_acls: Vec<RoutineAclEntry>,
    default_acls: Vec<DefaultAclEntry>,
    tables: Vec<TableEntry>,
    seed_rows: Vec<SeedRowEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OwnerContract {
    app_owner_symbol: String,
    bootstrap_owner_symbol: String,
    launch_owner_role: String,
}

macro_rules! catalog_entry {
    ($name:ident { $($field:ident : $type:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
        #[serde(deny_unknown_fields)]
        struct $name { $( $field: $type, )* }
    };
}

catalog_entry!(DatabaseEntry { owner: String });
catalog_entry!(DatabaseAclEntry {
    grantor: String,
    grantee: String,
    privilege: String,
    grant_option: bool
});
catalog_entry!(ExtensionEntry {
    name: String,
    version: String,
    schema: String,
    owner: String
});
catalog_entry!(SchemaEntry {
    name: String,
    owner: String
});
catalog_entry!(SchemaAclEntry {
    name: String,
    grantor: String,
    grantee: String,
    privilege: String,
    grant_option: bool
});
catalog_entry!(RelationEntry {
    schema: String,
    name: String,
    kind: String,
    persistence: String,
    owner: String
});
catalog_entry!(ColumnEntry {
    schema: String,
    relation: String,
    ordinal: i16,
    name: String,
    r#type: String,
    nullable: bool,
    default: String,
    identity: String,
    generated: String
});
catalog_entry!(ConstraintEntry {
    schema: String,
    relation: String,
    name: String,
    kind: String,
    definition: String,
    deferrable: bool,
    initially_deferred: bool,
    validated: bool
});
catalog_entry!(IndexEntry {
    schema: String,
    table: String,
    name: String,
    owner: String,
    definition: String,
    primary: bool,
    unique: bool,
    valid: bool,
    ready: bool,
    live: bool
});
catalog_entry!(TriggerEntry {
    schema: String,
    table: String,
    name: String,
    enabled: String,
    definition: String
});
catalog_entry!(RoutineEntry {
    schema: String, name: String, identity_arguments: String, kind: String, owner: String,
    language: String, return_type: String, volatility: String, strict: bool,
    security_definer: bool, leakproof: bool, parallel: String, config: Vec<String>,
    definition_sha256: String
});
catalog_entry!(RoleEntry {
    name: String, login: bool, inherit: bool, superuser: bool, create_db: bool,
    create_role: bool, replication: bool, bypass_rls: bool, connection_limit: i32,
    valid_until: String, password: String, config: Vec<String>
});
catalog_entry!(RoleMembershipEntry {
    role: String,
    member: String,
    admin_option: bool,
    inherit_option: bool,
    set_option: bool
});
catalog_entry!(RelationAclEntry {
    schema: String,
    relation: String,
    kind: String,
    grantor: String,
    grantee: String,
    privilege: String,
    grant_option: bool
});
catalog_entry!(RoutineAclEntry {
    schema: String,
    name: String,
    identity_arguments: String,
    grantor: String,
    grantee: String,
    privilege: String,
    grant_option: bool
});
catalog_entry!(DefaultAclEntry {
    owner: String,
    schema: String,
    object_kind: String,
    grantor: String,
    grantee: String,
    privilege: String,
    grant_option: bool
});
catalog_entry!(TableEntry { schema: String, name: String, row_count: i64, key_columns: Vec<String> });
catalog_entry!(SeedRowEntry {
    schema: String,
    table: String,
    key: String,
    value_sha256: String
});

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationManifest {
    format_version: u32,
    bootstrap: ManifestBootstrap,
    migrations: Vec<ManifestMigration>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestBootstrap {
    filename: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestMigration {
    version: i32,
    name: String,
    filename: String,
    sha256: String,
}

fn hash_hex(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes.as_ref()))
}

fn owner_name(
    symbol: &str,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<String, FirstLaunchVerificationError> {
    match symbol {
        "app_owner" => Ok(owners.app_owner.clone()),
        "bootstrap_owner" => Ok(owners.bootstrap_owner.clone()),
        "kb_launch_owner" => Ok("kb_launch_owner".into()),
        other => Err(failure(format!("unknown owner symbol {other:?}"))),
    }
}

fn owner_symbol(
    actual: String,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<String, FirstLaunchVerificationError> {
    if actual == owners.app_owner {
        Ok("app_owner".into())
    } else if actual == owners.bootstrap_owner {
        Ok("bootstrap_owner".into())
    } else if actual == "kb_launch_owner" {
        Ok(actual)
    } else {
        Err(failure(format!(
            "catalog object has unbound owner {actual:?}"
        )))
    }
}

fn ensure_sorted_unique<T: Ord + fmt::Debug>(
    label: &str,
    values: &[T],
) -> Result<(), FirstLaunchVerificationError> {
    for pair in values.windows(2) {
        if pair[0] >= pair[1] {
            return Err(failure(format!(
                "allow-list {label} entries are duplicate or unsorted near {:?}",
                pair[1]
            )));
        }
    }
    Ok(())
}

fn parse_key_names(raw: &str) -> Result<Vec<String>, FirstLaunchVerificationError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| failure(format!("invalid canonical seed key: {error}")))?;
    let items = value
        .as_array()
        .ok_or_else(|| failure("canonical seed key is not an array"))?;
    let mut names = Vec::with_capacity(items.len());
    for item in items {
        let pair = item
            .as_array()
            .ok_or_else(|| failure("canonical seed key item is not an array"))?;
        if pair.len() != 2 {
            return Err(failure(
                "canonical seed key item must contain a name and value",
            ));
        }
        names.push(
            pair[0]
                .as_str()
                .ok_or_else(|| failure("canonical seed key name is not text"))?
                .to_owned(),
        );
    }
    Ok(names)
}

fn simple_constraint_columns(definition: &str) -> Option<Vec<String>> {
    let body = definition
        .strip_prefix("PRIMARY KEY (")
        .or_else(|| definition.strip_prefix("UNIQUE ("))?
        .strip_suffix(')')?;
    Some(
        body.split(',')
            .map(|value| value.trim().trim_matches('"').to_owned())
            .collect(),
    )
}

fn validated_allowlist(raw: &str) -> Result<Allowlist, FirstLaunchVerificationError> {
    let allowlist: Allowlist = toml::from_str(raw)
        .map_err(|error| failure(format!("invalid catalog/row allow-list: {error}")))?;
    if allowlist.format_version != 1
        || allowlist.postgres_major != 16
        || allowlist.baseline_slice_count != 3
    {
        return Err(failure(
            "allow-list format, PostgreSQL major, or baseline slice count mismatch",
        ));
    }
    if allowlist.owner_contract.app_owner_symbol != "app_owner"
        || allowlist.owner_contract.bootstrap_owner_symbol != "bootstrap_owner"
        || allowlist.owner_contract.launch_owner_role != "kb_launch_owner"
    {
        return Err(failure("allow-list owner contract mismatch"));
    }
    owner_name(
        &allowlist.database.owner,
        &FreshPretrafficOwnerBindings {
            app_owner: "app_owner".into(),
            bootstrap_owner: "bootstrap_owner".into(),
        },
    )?;
    ensure_sorted_unique("database ACLs", &allowlist.database_acls)?;
    ensure_sorted_unique("extensions", &allowlist.extensions)?;
    ensure_sorted_unique("schemas", &allowlist.schemas)?;
    ensure_sorted_unique("schema ACLs", &allowlist.schema_acls)?;
    ensure_sorted_unique("relations", &allowlist.relations)?;
    ensure_sorted_unique("columns", &allowlist.columns)?;
    ensure_sorted_unique("constraints", &allowlist.constraints)?;
    ensure_sorted_unique("indexes", &allowlist.indexes)?;
    ensure_sorted_unique("triggers", &allowlist.triggers)?;
    ensure_sorted_unique("routines", &allowlist.routines)?;
    ensure_sorted_unique("roles", &allowlist.roles)?;
    ensure_sorted_unique("role memberships", &allowlist.role_memberships)?;
    ensure_sorted_unique(
        "finalized role memberships",
        &allowlist.finalized_role_memberships,
    )?;
    ensure_sorted_unique("relation ACLs", &allowlist.relation_acls)?;
    ensure_sorted_unique("routine ACLs", &allowlist.routine_acls)?;
    ensure_sorted_unique("default ACLs", &allowlist.default_acls)?;
    ensure_sorted_unique("tables", &allowlist.tables)?;
    ensure_sorted_unique("seed rows", &allowlist.seed_rows)?;

    macro_rules! exact_identities {
        ($label:literal, $values:expr, $key:expr) => {{
            let mut identities = BTreeSet::new();
            for value in $values {
                if !identities.insert($key(value)) {
                    return Err(failure(concat!("duplicate semantic ", $label, " identity")));
                }
            }
        }};
    }
    exact_identities!(
        "database ACL",
        &allowlist.database_acls,
        |v: &DatabaseAclEntry| (v.grantor.clone(), v.grantee.clone(), v.privilege.clone())
    );
    exact_identities!("extension", &allowlist.extensions, |v: &ExtensionEntry| v
        .name
        .clone());
    exact_identities!("schema", &allowlist.schemas, |v: &SchemaEntry| v
        .name
        .clone());
    exact_identities!(
        "schema ACL",
        &allowlist.schema_acls,
        |v: &SchemaAclEntry| (
            v.name.clone(),
            v.grantor.clone(),
            v.grantee.clone(),
            v.privilege.clone()
        )
    );
    exact_identities!("relation", &allowlist.relations, |v: &RelationEntry| (
        v.schema.clone(),
        v.name.clone()
    ));
    exact_identities!("column", &allowlist.columns, |v: &ColumnEntry| (
        v.schema.clone(),
        v.relation.clone(),
        v.ordinal
    ));
    exact_identities!(
        "constraint",
        &allowlist.constraints,
        |v: &ConstraintEntry| (v.schema.clone(), v.relation.clone(), v.name.clone())
    );
    exact_identities!("index", &allowlist.indexes, |v: &IndexEntry| (
        v.schema.clone(),
        v.name.clone()
    ));
    exact_identities!("trigger", &allowlist.triggers, |v: &TriggerEntry| (
        v.schema.clone(),
        v.table.clone(),
        v.name.clone()
    ));
    exact_identities!("routine", &allowlist.routines, |v: &RoutineEntry| (
        v.schema.clone(),
        v.name.clone(),
        v.identity_arguments.clone()
    ));
    exact_identities!("role", &allowlist.roles, |v: &RoleEntry| v.name.clone());
    exact_identities!(
        "role membership",
        &allowlist.role_memberships,
        |v: &RoleMembershipEntry| (v.role.clone(), v.member.clone())
    );
    exact_identities!(
        "finalized role membership",
        &allowlist.finalized_role_memberships,
        |v: &RoleMembershipEntry| (v.role.clone(), v.member.clone())
    );
    if !allowlist.finalized_role_memberships.is_empty() {
        return Err(failure(
            "post-verification allow-list requires zero launch-role membership edges",
        ));
    }
    exact_identities!(
        "relation ACL",
        &allowlist.relation_acls,
        |v: &RelationAclEntry| (
            v.schema.clone(),
            v.relation.clone(),
            v.grantor.clone(),
            v.grantee.clone(),
            v.privilege.clone()
        )
    );
    exact_identities!(
        "routine ACL",
        &allowlist.routine_acls,
        |v: &RoutineAclEntry| (
            v.schema.clone(),
            v.name.clone(),
            v.identity_arguments.clone(),
            v.grantor.clone(),
            v.grantee.clone(),
            v.privilege.clone()
        )
    );
    exact_identities!(
        "default ACL",
        &allowlist.default_acls,
        |v: &DefaultAclEntry| (
            v.owner.clone(),
            v.schema.clone(),
            v.object_kind.clone(),
            v.grantor.clone(),
            v.grantee.clone(),
            v.privilege.clone()
        )
    );
    exact_identities!("table", &allowlist.tables, |v: &TableEntry| (
        v.schema.clone(),
        v.name.clone()
    ));

    let relation_tables: BTreeSet<_> = allowlist
        .relations
        .iter()
        .filter(|entry| entry.kind == "table")
        .map(|entry| (entry.schema.as_str(), entry.name.as_str()))
        .collect();
    let counted_tables: BTreeSet<_> = allowlist
        .tables
        .iter()
        .map(|entry| (entry.schema.as_str(), entry.name.as_str()))
        .collect();
    if relation_tables != counted_tables {
        return Err(failure(
            "every table relation must have exactly one table-count entry and no non-table may have one",
        ));
    }
    for hash in allowlist
        .routines
        .iter()
        .map(|entry| &entry.definition_sha256)
        .chain(allowlist.seed_rows.iter().map(|entry| &entry.value_sha256))
    {
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(failure(format!(
                "allow-list contains malformed SHA-256 {hash:?}"
            )));
        }
    }
    let tables: BTreeMap<_, _> = allowlist
        .tables
        .iter()
        .map(|entry| ((entry.schema.as_str(), entry.name.as_str()), entry))
        .collect();
    let constraints: Vec<_> = allowlist
        .constraints
        .iter()
        .filter_map(|entry| {
            simple_constraint_columns(&entry.definition)
                .map(|columns| ((entry.schema.as_str(), entry.relation.as_str()), columns))
        })
        .collect();
    let mut seed_counts = BTreeMap::<(&str, &str), i64>::new();
    let mut seed_keys = BTreeSet::new();
    for seed in &allowlist.seed_rows {
        let table = tables
            .get(&(seed.schema.as_str(), seed.table.as_str()))
            .ok_or_else(|| {
                failure(format!(
                    "seed row references absent table {}.{}",
                    seed.schema, seed.table
                ))
            })?;
        let key_names = parse_key_names(&seed.key)?;
        if key_names != table.key_columns {
            return Err(failure(format!(
                "seed key columns mismatch for {}.{}",
                seed.schema, seed.table
            )));
        }
        if !constraints.iter().any(|(identity, columns)| {
            *identity == (seed.schema.as_str(), seed.table.as_str()) && *columns == key_names
        }) {
            return Err(failure(format!(
                "seed key is not a declared primary/unique key for {}.{}",
                seed.schema, seed.table
            )));
        }
        if !seed_keys.insert((seed.schema.as_str(), seed.table.as_str(), seed.key.as_str())) {
            return Err(failure("duplicate canonical seed key"));
        }
        *seed_counts
            .entry((seed.schema.as_str(), seed.table.as_str()))
            .or_default() += 1;
    }
    for (identity, table) in &tables {
        let represented = seed_counts.get(identity).copied().unwrap_or(0);
        if table.name == "schema_migrations" {
            if table.row_count != 3 || represented != 0 {
                return Err(failure(
                    "migration ledger must be represented only as three ledger evidence rows",
                ));
            }
        } else if represented != table.row_count {
            return Err(failure(format!(
                "seed-row count differs from declared row count for {}.{}",
                identity.0, identity.1
            )));
        }
    }
    Ok(allowlist)
}

fn compare_exact<T: Ord + fmt::Debug>(
    label: &str,
    expected: Vec<T>,
    actual: Vec<T>,
) -> Result<(), FirstLaunchVerificationError> {
    if expected == actual {
        return Ok(());
    }
    let expected: BTreeSet<_> = expected.into_iter().collect();
    let actual: BTreeSet<_> = actual.into_iter().collect();
    let missing = expected.difference(&actual).next();
    let extra = actual.difference(&expected).next();
    Err(failure(format!(
        "{label} allow-list mismatch; missing={missing:?}; extra={extra:?}"
    )))
}

fn text_array(row: &sqlx::postgres::PgRow, column: &str) -> Result<Vec<String>, sqlx::Error> {
    Ok(row
        .try_get::<Option<Vec<String>>, _>(column)?
        .unwrap_or_default())
}

async fn verify_owner_contract(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<(), FirstLaunchVerificationError> {
    if owners.app_owner.is_empty() || owners.bootstrap_owner.is_empty() {
        return Err(failure("owner bindings must not be empty"));
    }
    let row = sqlx::query(
        "SELECT session_user AS session_owner,
                pg_catalog.pg_get_userbyid(database_value.datdba) AS database_owner,
                pg_catalog.pg_get_userbyid(namespace_value.nspowner) AS schema_owner,
                EXISTS(SELECT 1 FROM pg_catalog.pg_roles WHERE rolname=$1
                  AND NOT rolcanlogin AND NOT rolinherit AND NOT rolsuper
                  AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication
                  AND NOT rolbypassrls) AS app_owner_exists,
                EXISTS(SELECT 1 FROM pg_catalog.pg_roles WHERE rolname=$2) AS bootstrap_exists,
                EXISTS(SELECT 1 FROM pg_catalog.pg_roles
                  WHERE rolname='kb_first_launch_verifier' AND rolcanlogin AND rolinherit
                    AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
                    AND NOT rolreplication AND NOT rolbypassrls) AS verifier_exists
         FROM pg_catalog.pg_database database_value, pg_catalog.pg_namespace namespace_value
         WHERE database_value.datname=current_database() AND namespace_value.nspname='public'",
    )
    .bind(&owners.app_owner)
    .bind(&owners.bootstrap_owner)
    .fetch_one(&mut **tx)
    .await?;
    let session: String = row.try_get("session_owner")?;
    let database_owner: String = row.try_get("database_owner")?;
    let schema_owner: String = row.try_get("schema_owner")?;
    if !row.try_get::<bool, _>("app_owner_exists")?
        || !row.try_get::<bool, _>("bootstrap_exists")?
        || !row.try_get::<bool, _>("verifier_exists")?
        || session != "kb_first_launch_verifier"
        || database_owner != owners.app_owner
        || schema_owner != owners.app_owner
    {
        return Err(failure(format!(
            "first-launch owner binding mismatch: session={session}, database={database_owner}, public={schema_owner}"
        )));
    }
    Ok(())
}

fn validated_migration_manifest(
    raw: &str,
) -> Result<MigrationManifest, FirstLaunchVerificationError> {
    let manifest: MigrationManifest = toml::from_str(raw)
        .map_err(|error| failure(format!("invalid migration manifest: {error}")))?;
    if manifest.format_version != 1
        || manifest.bootstrap.filename != "deploy/postgres-init/010-runtime-identities.sh"
        || manifest.bootstrap.sha256 != hash_hex(BOOTSTRAP_AUTHORITY)
        || manifest.migrations.len() != BASELINE_SLICES.len()
    {
        return Err(failure(
            "migration manifest bootstrap or slice identity mismatch",
        ));
    }
    let mut versions = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut filenames = BTreeSet::new();
    for (ordinal, (entry, (name, filename, sql))) in
        manifest.migrations.iter().zip(BASELINE_SLICES).enumerate()
    {
        let expected_version =
            i32::try_from(ordinal + 1).map_err(|_| failure("baseline slice ordinal overflow"))?;
        if entry.version != expected_version
            || entry.name != *name
            || entry.filename != *filename
            || !versions.insert(entry.version)
            || !names.insert(entry.name.as_str())
            || !filenames.insert(entry.filename.as_str())
            || entry.sha256 != hash_hex(sql)
        {
            return Err(failure(
                "migration manifest contains an unexpected slice or checksum",
            ));
        }
    }
    Ok(manifest)
}

async fn verify_ledger_and_lifecycle(
    tx: &mut Transaction<'_, Postgres>,
    manifest: &MigrationManifest,
    _owners: &FreshPretrafficOwnerBindings,
) -> Result<(), FirstLaunchVerificationError> {
    sqlx::query("SET LOCAL ROLE kb_launch_owner")
        .execute(&mut **tx)
        .await?;
    let ledger: Vec<(i32, String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT version,name,checksum,applied_at FROM public.schema_migrations ORDER BY version",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| failure(format!("ledger read: {error}")))?;
    if ledger.len() != manifest.migrations.len()
        || ledger.iter().zip(&manifest.migrations).any(
            |((version, name, checksum, _), expected)| {
                *version != expected.version
                    || name != &expected.name
                    || checksum != &expected.sha256
            },
        )
        || ledger.windows(2).any(|pair| pair[0].3 > pair[1].3)
    {
        return Err(failure(
            "migration ledger does not exactly match migration-manifest.toml",
        ));
    }
    sqlx::query("SET LOCAL ROLE kb_app_owner")
        .execute(&mut **tx)
        .await?;
    let marker_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM public.production_first_launch_catalog_verifications",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| failure(format!("empty marker read: {error}")))?;
    if marker_count != 0 {
        return Err(failure(
            "zero-row first-launch verifier cannot be rerun after durable verification",
        ));
    }
    let gate_valid: bool = sqlx::query_scalar(
        "SELECT count(*)=1 AND bool_and(mode='maintenance') FROM public.application_maintenance_gate",
    ).fetch_one(&mut **tx).await
        .map_err(|error| failure(format!("maintenance gate read: {error}")))?;
    let launch_valid: bool = sqlx::query_scalar(
        "SELECT count(*)=1 AND bool_and(state='preflight' AND cutover_id IS NULL
           AND cutover_epoch=0 AND evidence_epoch=0
           AND traffic_exposure_started_at IS NULL AND reset_authority_revoked_at IS NULL
           AND first_production_request_at IS NULL)
         FROM public.production_launch_state",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| failure(format!("launch state read: {error}")))?;
    sqlx::query("RESET ROLE").execute(&mut **tx).await?;
    if !gate_valid || !launch_valid {
        return Err(failure(
            "database is not in the initial maintenance/pretraffic launch state",
        ));
    }
    Ok(())
}

const INCLUDED_CLASS_CTE: &str = "WITH included_class AS (
 SELECT relation.oid,namespace.nspname AS schema_name,relation.relname,relation.relkind,
        relation.relpersistence,relation.relowner
 FROM pg_catalog.pg_class relation
 JOIN pg_catalog.pg_namespace namespace ON namespace.oid=relation.relnamespace
 WHERE namespace.nspname NOT IN ('pg_catalog','information_schema')
   AND namespace.nspname NOT LIKE 'pg_toast%' AND namespace.nspname NOT LIKE 'pg_temp_%'
   AND relation.relkind IN ('r','p','v','m','S','f','c','i','I')
   AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_depend dependency
     WHERE dependency.classid='pg_catalog.pg_class'::regclass AND dependency.objid=relation.oid
       AND dependency.objsubid=0 AND dependency.refclassid='pg_catalog.pg_extension'::regclass
       AND dependency.deptype='e')
) ";

fn persistence(value: &str) -> Result<String, FirstLaunchVerificationError> {
    match value {
        "p" => Ok("permanent".into()),
        "u" => Ok("unlogged".into()),
        "t" => Ok("temporary".into()),
        _ => Err(failure("unsupported relation persistence")),
    }
}
fn relation_kind(value: &str) -> Result<String, FirstLaunchVerificationError> {
    match value {
        "r" => Ok("table".into()),
        "p" => Ok("partitioned".into()),
        "v" => Ok("view".into()),
        "m" => Ok("matview".into()),
        "S" => Ok("sequence".into()),
        "f" => Ok("foreign".into()),
        "c" => Ok("composite".into()),
        "i" | "I" => Ok("index".into()),
        _ => Err(failure("unsupported relation kind")),
    }
}
fn constraint_kind(value: &str) -> Result<String, FirstLaunchVerificationError> {
    match value {
        "p" => Ok("primary_key".into()),
        "u" => Ok("unique".into()),
        "f" => Ok("foreign_key".into()),
        "c" => Ok("check".into()),
        "x" => Ok("exclusion".into()),
        "t" => Ok("constraint_trigger".into()),
        _ => Err(failure(format!("unsupported constraint kind {value:?}"))),
    }
}
fn enabled_kind(value: &str) -> Result<String, FirstLaunchVerificationError> {
    match value {
        "O" => Ok("origin".into()),
        "D" => Ok("disabled".into()),
        "R" => Ok("replica".into()),
        "A" => Ok("always".into()),
        _ => Err(failure("unsupported trigger enabled state")),
    }
}
fn routine_kind(value: &str) -> Result<String, FirstLaunchVerificationError> {
    match value {
        "f" => Ok("function".into()),
        "p" => Ok("procedure".into()),
        "a" => Ok("aggregate".into()),
        "w" => Ok("window".into()),
        _ => Err(failure("unsupported routine kind")),
    }
}
fn volatility(value: &str) -> Result<String, FirstLaunchVerificationError> {
    match value {
        "v" => Ok("volatile".into()),
        "s" => Ok("stable".into()),
        "i" => Ok("immutable".into()),
        _ => Err(failure("unsupported routine volatility")),
    }
}
fn parallel(value: &str) -> Result<String, FirstLaunchVerificationError> {
    match value {
        "u" => Ok("unsafe".into()),
        "r" => Ok("restricted".into()),
        "s" => Ok("safe".into()),
        _ => Err(failure("unsupported routine parallel state")),
    }
}

async fn actual_extensions(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<ExtensionEntry>, FirstLaunchVerificationError> {
    let rows = sqlx::query("SELECT extension_value.extname,extension_value.extversion,namespace.nspname,pg_catalog.pg_get_userbyid(extension_value.extowner) AS owner_name FROM pg_catalog.pg_extension extension_value JOIN pg_catalog.pg_namespace namespace ON namespace.oid=extension_value.extnamespace WHERE extension_value.extname<>'plpgsql' ORDER BY 1")
        .fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            let actual_owner: String = row.try_get("owner_name")?;
            if actual_owner != owners.bootstrap_owner {
                return Err(failure(format!(
                    "extension has unexpected bootstrap owner {actual_owner:?}"
                )));
            }
            Ok(ExtensionEntry {
                name: row.try_get("extname")?,
                version: row.try_get("extversion")?,
                schema: row.try_get("nspname")?,
                owner: "bootstrap_owner".into(),
            })
        })
        .collect()
}

async fn actual_schemas(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<SchemaEntry>, FirstLaunchVerificationError> {
    let rows = sqlx::query("SELECT namespace.nspname,pg_catalog.pg_get_userbyid(namespace.nspowner) AS owner_name FROM pg_catalog.pg_namespace namespace WHERE namespace.nspname NOT IN ('pg_catalog','information_schema') AND namespace.nspname NOT LIKE 'pg_toast%' AND namespace.nspname NOT LIKE 'pg_temp_%' AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_depend dependency WHERE dependency.classid='pg_catalog.pg_namespace'::regclass AND dependency.objid=namespace.oid AND dependency.objsubid=0 AND dependency.refclassid='pg_catalog.pg_extension'::regclass AND dependency.deptype='e') ORDER BY 1")
        .fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            Ok(SchemaEntry {
                name: row.try_get("nspname")?,
                owner: owner_symbol(row.try_get("owner_name")?, owners)?,
            })
        })
        .collect()
}

async fn actual_relations(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<RelationEntry>, FirstLaunchVerificationError> {
    let sql = format!(
        "{INCLUDED_CLASS_CTE} SELECT schema_name,relname,relkind::text AS relkind,relpersistence::text AS relpersistence,pg_catalog.pg_get_userbyid(relowner) AS owner_name FROM included_class ORDER BY 1,2"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(RelationEntry {
                schema: row.try_get("schema_name")?,
                name: row.try_get("relname")?,
                kind: relation_kind(&row.try_get::<String, _>("relkind")?)?,
                persistence: persistence(&row.try_get::<String, _>("relpersistence")?)?,
                owner: owner_symbol(row.try_get("owner_name")?, owners)?,
            })
        })
        .collect()
}

async fn actual_columns(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<ColumnEntry>, FirstLaunchVerificationError> {
    let sql = format!(
        "{INCLUDED_CLASS_CTE} SELECT relation.schema_name,relation.relname,attribute.attnum,attribute.attname,pg_catalog.format_type(attribute.atttypid,attribute.atttypmod) AS type_name,NOT attribute.attnotnull AS nullable,COALESCE(pg_catalog.pg_get_expr(default_value.adbin,default_value.adrelid),'') AS default_value,attribute.attidentity::text AS identity_value,attribute.attgenerated::text AS generated_value FROM included_class relation JOIN pg_catalog.pg_attribute attribute ON attribute.attrelid=relation.oid LEFT JOIN pg_catalog.pg_attrdef default_value ON default_value.adrelid=attribute.attrelid AND default_value.adnum=attribute.attnum WHERE attribute.attnum>0 AND NOT attribute.attisdropped ORDER BY 1,2,3"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ColumnEntry {
                schema: row.try_get("schema_name")?,
                relation: row.try_get("relname")?,
                ordinal: row.try_get("attnum")?,
                name: row.try_get("attname")?,
                r#type: row.try_get("type_name")?,
                nullable: row.try_get("nullable")?,
                default: row.try_get("default_value")?,
                identity: row.try_get("identity_value")?,
                generated: row.try_get("generated_value")?,
            })
        })
        .collect()
}

async fn actual_constraints(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<ConstraintEntry>, FirstLaunchVerificationError> {
    let sql = format!(
        "{INCLUDED_CLASS_CTE} SELECT relation.schema_name,relation.relname,constraint_value.conname,constraint_value.contype::text AS contype,pg_catalog.pg_get_constraintdef(constraint_value.oid,true) AS definition,constraint_value.condeferrable,constraint_value.condeferred,constraint_value.convalidated FROM pg_catalog.pg_constraint constraint_value JOIN included_class relation ON relation.oid=constraint_value.conrelid ORDER BY 1,2,3"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ConstraintEntry {
                schema: row.try_get("schema_name")?,
                relation: row.try_get("relname")?,
                name: row.try_get("conname")?,
                kind: constraint_kind(&row.try_get::<String, _>("contype")?)?,
                definition: row.try_get("definition")?,
                deferrable: row.try_get("condeferrable")?,
                initially_deferred: row.try_get("condeferred")?,
                validated: row.try_get("convalidated")?,
            })
        })
        .collect()
}

async fn actual_indexes(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<IndexEntry>, FirstLaunchVerificationError> {
    let sql = format!(
        "{INCLUDED_CLASS_CTE} SELECT table_relation.schema_name,table_relation.relname AS table_name,index_relation.relname AS index_name,pg_catalog.pg_get_userbyid(index_relation.relowner) AS owner_name,pg_catalog.pg_get_indexdef(index_value.indexrelid) AS definition,index_value.indisprimary,index_value.indisunique,index_value.indisvalid,index_value.indisready,index_value.indislive FROM pg_catalog.pg_index index_value JOIN included_class table_relation ON table_relation.oid=index_value.indrelid JOIN included_class index_relation ON index_relation.oid=index_value.indexrelid ORDER BY 1,2,3"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(IndexEntry {
                schema: row.try_get("schema_name")?,
                table: row.try_get("table_name")?,
                name: row.try_get("index_name")?,
                owner: owner_symbol(row.try_get("owner_name")?, owners)?,
                definition: row.try_get("definition")?,
                primary: row.try_get("indisprimary")?,
                unique: row.try_get("indisunique")?,
                valid: row.try_get("indisvalid")?,
                ready: row.try_get("indisready")?,
                live: row.try_get("indislive")?,
            })
        })
        .collect()
}

async fn actual_triggers(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<TriggerEntry>, FirstLaunchVerificationError> {
    let sql = format!(
        "{INCLUDED_CLASS_CTE} SELECT relation.schema_name,relation.relname,trigger_value.tgname,trigger_value.tgenabled::text AS enabled_value,pg_catalog.pg_get_triggerdef(trigger_value.oid,true) AS definition FROM pg_catalog.pg_trigger trigger_value JOIN included_class relation ON relation.oid=trigger_value.tgrelid WHERE NOT trigger_value.tgisinternal ORDER BY 1,2,3"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(TriggerEntry {
                schema: row.try_get("schema_name")?,
                table: row.try_get("relname")?,
                name: row.try_get("tgname")?,
                enabled: enabled_kind(&row.try_get::<String, _>("enabled_value")?)?,
                definition: row.try_get("definition")?,
            })
        })
        .collect()
}

async fn actual_routines(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<RoutineEntry>, FirstLaunchVerificationError> {
    let rows = sqlx::query("SELECT namespace.nspname,procedure_value.proname,pg_catalog.pg_get_function_identity_arguments(procedure_value.oid) AS identity_arguments,procedure_value.prokind::text AS prokind,pg_catalog.pg_get_userbyid(procedure_value.proowner) AS owner_name,language_value.lanname,pg_catalog.format_type(procedure_value.prorettype,NULL) AS return_type,procedure_value.provolatile::text AS provolatile,procedure_value.proisstrict,procedure_value.prosecdef,procedure_value.proleakproof,procedure_value.proparallel::text AS proparallel,procedure_value.proconfig,pg_catalog.pg_get_functiondef(procedure_value.oid) AS definition FROM pg_catalog.pg_proc procedure_value JOIN pg_catalog.pg_namespace namespace ON namespace.oid=procedure_value.pronamespace JOIN pg_catalog.pg_language language_value ON language_value.oid=procedure_value.prolang WHERE namespace.nspname NOT IN ('pg_catalog','information_schema') AND namespace.nspname NOT LIKE 'pg_toast%' AND namespace.nspname NOT LIKE 'pg_temp_%' AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_depend dependency WHERE dependency.classid='pg_catalog.pg_proc'::regclass AND dependency.objid=procedure_value.oid AND dependency.objsubid=0 AND dependency.refclassid='pg_catalog.pg_extension'::regclass AND dependency.deptype='e') ORDER BY 1,2,3")
        .fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            let definition: String = row.try_get("definition")?;
            Ok(RoutineEntry {
                schema: row.try_get("nspname")?,
                name: row.try_get("proname")?,
                identity_arguments: row.try_get("identity_arguments")?,
                kind: routine_kind(&row.try_get::<String, _>("prokind")?)?,
                owner: owner_symbol(row.try_get("owner_name")?, owners)?,
                language: row.try_get("lanname")?,
                return_type: row.try_get("return_type")?,
                volatility: volatility(&row.try_get::<String, _>("provolatile")?)?,
                strict: row.try_get("proisstrict")?,
                security_definer: row.try_get("prosecdef")?,
                leakproof: row.try_get("proleakproof")?,
                parallel: parallel(&row.try_get::<String, _>("proparallel")?)?,
                config: text_array(&row, "proconfig")?,
                definition_sha256: hash_hex(definition),
            })
        })
        .collect()
}

async fn actual_roles(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<RoleEntry>, FirstLaunchVerificationError> {
    let rows = sqlx::query("SELECT role_value.rolname,role_value.rolcanlogin,role_value.rolinherit,role_value.rolsuper,role_value.rolcreatedb,role_value.rolcreaterole,role_value.rolreplication,role_value.rolbypassrls,role_value.rolconnlimit,COALESCE(role_value.rolvaliduntil::text,'infinity') AS valid_until,COALESCE((SELECT array_agg(config_value ORDER BY config_value) FROM pg_catalog.pg_db_role_setting setting_value CROSS JOIN LATERAL unnest(setting_value.setconfig) config_value WHERE setting_value.setrole=role_value.oid AND setting_value.setdatabase=0),ARRAY[]::text[]) AS config FROM pg_catalog.pg_roles role_value WHERE role_value.rolname=ANY($1) ORDER BY 1")
        .bind(GOVERNED_ROLES).fetch_all(&mut **tx).await?;
    let (is_superuser, has_helper): (bool, bool) = sqlx::query_as(
        "SELECT rolsuper,to_regprocedure('pg_catalog.kb_launch_role_password_absent(name)') IS NOT NULL
         FROM pg_catalog.pg_roles WHERE rolname=current_user",
    ).fetch_one(&mut **tx).await?;
    if !is_superuser && !has_helper {
        return Err(failure(
            "restricted bootstrap password-posture helper is absent",
        ));
    }
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.try_get("rolname")?;
        let password_absent: bool = if is_superuser {
            sqlx::query_scalar(
                "SELECT rolpassword IS NULL FROM pg_catalog.pg_authid WHERE rolname=$1",
            )
            .bind(&name)
            .fetch_one(&mut **tx)
            .await?
        } else {
            sqlx::query_scalar("SELECT pg_catalog.kb_launch_role_password_absent($1::name)")
                .bind(&name)
                .fetch_one(&mut **tx)
                .await?
        };
        result.push(RoleEntry {
            name,
            login: row.try_get("rolcanlogin")?,
            inherit: row.try_get("rolinherit")?,
            superuser: row.try_get("rolsuper")?,
            create_db: row.try_get("rolcreatedb")?,
            create_role: row.try_get("rolcreaterole")?,
            replication: row.try_get("rolreplication")?,
            bypass_rls: row.try_get("rolbypassrls")?,
            connection_limit: row.try_get("rolconnlimit")?,
            valid_until: row.try_get("valid_until")?,
            password: if password_absent {
                "absent".into()
            } else {
                "present".into()
            },
            config: text_array(&row, "config")?,
        });
    }
    Ok(result)
}

async fn verify_runtime_role_reachability(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), FirstLaunchVerificationError> {
    let unsafe_path: bool = sqlx::query_scalar(
        "WITH privileged(role_name) AS (
           SELECT unnest($1::text[])
           UNION
           SELECT pg_catalog.pg_get_userbyid(database_value.datdba)
           FROM pg_catalog.pg_database database_value
           WHERE database_value.datname=current_database()
           UNION
           SELECT pg_catalog.pg_get_userbyid(namespace.nspowner)
           FROM pg_catalog.pg_namespace namespace WHERE namespace.nspname='public'
         )
         SELECT EXISTS(
           SELECT 1
           FROM unnest(ARRAY['kb_runtime_api','kb_runtime_retention','kb_runtime_worker']) runtime(role_name)
           CROSS JOIN privileged
           WHERE privileged.role_name<>runtime.role_name
             AND (pg_catalog.pg_has_role(runtime.role_name,privileged.role_name,'MEMBER')
               OR pg_catalog.pg_has_role(runtime.role_name,privileged.role_name,'SET')))",
    )
    .bind(GOVERNED_ROLES)
    .fetch_one(&mut **tx)
    .await?;
    if unsafe_path {
        return Err(failure(
            "runtime role has recursive MEMBER/SET authority to a governed or owner identity",
        ));
    }
    Ok(())
}

async fn actual_memberships(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<RoleMembershipEntry>, FirstLaunchVerificationError> {
    fn membership_name(
        value: String,
        owners: &FreshPretrafficOwnerBindings,
    ) -> Result<String, FirstLaunchVerificationError> {
        if GOVERNED_ROLES.contains(&value.as_str()) {
            Ok(value)
        } else {
            owner_symbol(value, owners)
        }
    }
    // PostgreSQL 16 can retain equivalent grants from multiple grantors. The
    // security edge is role/member plus its effective options, not grantor OID.
    let rows = sqlx::query("SELECT granted.rolname AS role_name,member.rolname AS member_name,bool_or(membership.admin_option) AS admin_option,bool_or(membership.inherit_option) AS inherit_option,bool_or(membership.set_option) AS set_option FROM pg_catalog.pg_auth_members membership JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid JOIN pg_catalog.pg_roles member ON member.oid=membership.member WHERE granted.rolname=ANY($1) OR member.rolname=ANY($1) GROUP BY granted.rolname,member.rolname ORDER BY 1,2")
        .bind(GOVERNED_ROLES).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            Ok(RoleMembershipEntry {
                role: membership_name(row.try_get("role_name")?, owners)?,
                member: membership_name(row.try_get("member_name")?, owners)?,
                admin_option: row.try_get("admin_option")?,
                inherit_option: row.try_get("inherit_option")?,
                set_option: row.try_get("set_option")?,
            })
        })
        .collect()
}

fn acl_principal(
    actual: String,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<String, FirstLaunchVerificationError> {
    if actual == "PUBLIC" || GOVERNED_ROLES.contains(&actual.as_str()) {
        Ok(actual)
    } else {
        owner_symbol(actual, owners)
    }
}

async fn actual_database_and_acls(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<(DatabaseEntry, Vec<DatabaseAclEntry>), FirstLaunchVerificationError> {
    let row = sqlx::query(
        "SELECT pg_catalog.pg_get_userbyid(database_value.datdba) AS owner_name
         FROM pg_catalog.pg_database database_value
         WHERE database_value.datname=current_database()",
    )
    .fetch_one(&mut **tx)
    .await?;
    let owner = owner_symbol(row.try_get("owner_name")?, owners)?;
    let rows = sqlx::query(
        "SELECT pg_catalog.pg_get_userbyid(acl.grantor) AS grantor_name,
                CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(acl.grantee) END AS grantee_name,
                acl.privilege_type,acl.is_grantable
         FROM pg_catalog.pg_database database_value
         CROSS JOIN LATERAL pg_catalog.aclexplode(
           COALESCE(database_value.datacl,pg_catalog.acldefault('d',database_value.datdba))) acl
         WHERE database_value.datname=current_database()
         ORDER BY 1,2,3,4",
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut acls = rows
        .into_iter()
        .map(|row| {
            Ok(DatabaseAclEntry {
                grantor: acl_principal(row.try_get("grantor_name")?, owners)?,
                grantee: acl_principal(row.try_get("grantee_name")?, owners)?,
                privilege: row.try_get("privilege_type")?,
                grant_option: row.try_get("is_grantable")?,
            })
        })
        .collect::<Result<Vec<_>, FirstLaunchVerificationError>>()?;
    acls.sort();
    Ok((DatabaseEntry { owner }, acls))
}

async fn actual_schema_acls(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<SchemaAclEntry>, FirstLaunchVerificationError> {
    let rows = sqlx::query(
        "SELECT namespace.nspname,
                pg_catalog.pg_get_userbyid(acl.grantor) AS grantor_name,
                CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(acl.grantee) END AS grantee_name,
                acl.privilege_type,acl.is_grantable
         FROM pg_catalog.pg_namespace namespace
         CROSS JOIN LATERAL pg_catalog.aclexplode(
           COALESCE(namespace.nspacl,pg_catalog.acldefault('n',namespace.nspowner))) acl
         WHERE namespace.nspname NOT IN ('pg_catalog','information_schema')
           AND namespace.nspname NOT LIKE 'pg_toast%'
           AND namespace.nspname NOT LIKE 'pg_temp_%'
           AND NOT EXISTS(
             SELECT 1 FROM pg_catalog.pg_depend dependency
             WHERE dependency.classid='pg_catalog.pg_namespace'::regclass
               AND dependency.objid=namespace.oid AND dependency.objsubid=0
               AND dependency.refclassid='pg_catalog.pg_extension'::regclass
               AND dependency.deptype='e')
         ORDER BY 1,2,3,4,5",
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut result = rows
        .into_iter()
        .map(|row| {
            Ok(SchemaAclEntry {
                name: row.try_get("nspname")?,
                grantor: acl_principal(row.try_get("grantor_name")?, owners)?,
                grantee: acl_principal(row.try_get("grantee_name")?, owners)?,
                privilege: row.try_get("privilege_type")?,
                grant_option: row.try_get("is_grantable")?,
            })
        })
        .collect::<Result<Vec<_>, FirstLaunchVerificationError>>()?;
    result.sort();
    Ok(result)
}

async fn actual_relation_acls(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<RelationAclEntry>, FirstLaunchVerificationError> {
    let sql = format!(
        "{INCLUDED_CLASS_CTE} SELECT relation.schema_name,relation.relname,relation.relkind::text AS relkind,pg_catalog.pg_get_userbyid(acl.grantor) AS grantor_name,CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(acl.grantee) END AS grantee_name,acl.privilege_type,acl.is_grantable FROM included_class relation CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE((SELECT relacl FROM pg_catalog.pg_class WHERE oid=relation.oid),pg_catalog.acldefault(CASE WHEN relation.relkind='S' THEN 'S'::\"char\" ELSE 'r'::\"char\" END,relation.relowner))) acl WHERE relation.relkind IN ('r','p','v','m','S','f') ORDER BY 1,2,4,5,6,7"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut **tx)
        .await?;
    let mut result: Vec<_> = rows
        .into_iter()
        .map(|row| {
            Ok(RelationAclEntry {
                schema: row.try_get("schema_name")?,
                relation: row.try_get("relname")?,
                kind: relation_kind(&row.try_get::<String, _>("relkind")?)?,
                grantor: acl_principal(row.try_get("grantor_name")?, owners)?,
                grantee: acl_principal(row.try_get("grantee_name")?, owners)?,
                privilege: row.try_get("privilege_type")?,
                grant_option: row.try_get("is_grantable")?,
            })
        })
        .collect::<Result<_, FirstLaunchVerificationError>>()?;
    result.sort();
    Ok(result)
}

async fn actual_routine_acls(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<RoutineAclEntry>, FirstLaunchVerificationError> {
    let rows = sqlx::query("SELECT namespace.nspname,procedure_value.proname,pg_catalog.pg_get_function_identity_arguments(procedure_value.oid) AS identity_arguments,pg_catalog.pg_get_userbyid(acl.grantor) AS grantor_name,CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(acl.grantee) END AS grantee_name,acl.privilege_type,acl.is_grantable FROM pg_catalog.pg_proc procedure_value JOIN pg_catalog.pg_namespace namespace ON namespace.oid=procedure_value.pronamespace CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(procedure_value.proacl,pg_catalog.acldefault('f',procedure_value.proowner))) acl WHERE namespace.nspname NOT IN ('pg_catalog','information_schema') AND namespace.nspname NOT LIKE 'pg_toast%' AND namespace.nspname NOT LIKE 'pg_temp_%' AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_depend dependency WHERE dependency.classid='pg_catalog.pg_proc'::regclass AND dependency.objid=procedure_value.oid AND dependency.objsubid=0 AND dependency.refclassid='pg_catalog.pg_extension'::regclass AND dependency.deptype='e') ORDER BY 1,2,3,4,5,6,7")
        .fetch_all(&mut **tx).await?;
    let mut result: Vec<_> = rows
        .into_iter()
        .map(|row| {
            Ok(RoutineAclEntry {
                schema: row.try_get("nspname")?,
                name: row.try_get("proname")?,
                identity_arguments: row.try_get("identity_arguments")?,
                grantor: acl_principal(row.try_get("grantor_name")?, owners)?,
                grantee: acl_principal(row.try_get("grantee_name")?, owners)?,
                privilege: row.try_get("privilege_type")?,
                grant_option: row.try_get("is_grantable")?,
            })
        })
        .collect::<Result<_, FirstLaunchVerificationError>>()?;
    result.sort();
    Ok(result)
}

async fn actual_default_acls(
    tx: &mut Transaction<'_, Postgres>,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<DefaultAclEntry>, FirstLaunchVerificationError> {
    let rows = sqlx::query("SELECT pg_catalog.pg_get_userbyid(default_value.defaclrole) AS owner_name,COALESCE(namespace.nspname,'') AS schema_name,default_value.defaclobjtype::text AS object_kind,pg_catalog.pg_get_userbyid(acl.grantor) AS grantor_name,CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(acl.grantee) END AS grantee_name,acl.privilege_type,acl.is_grantable FROM pg_catalog.pg_default_acl default_value LEFT JOIN pg_catalog.pg_namespace namespace ON namespace.oid=default_value.defaclnamespace CROSS JOIN LATERAL pg_catalog.aclexplode(default_value.defaclacl) acl WHERE namespace.nspname IS NULL OR (namespace.nspname NOT IN ('pg_catalog','information_schema') AND namespace.nspname NOT LIKE 'pg_toast%' AND namespace.nspname NOT LIKE 'pg_temp_%') ORDER BY 1,2,3,4,5,6,7")
        .fetch_all(&mut **tx).await?;
    let mut result: Vec<_> = rows
        .into_iter()
        .map(|row| {
            Ok(DefaultAclEntry {
                owner: acl_principal(row.try_get("owner_name")?, owners)?,
                schema: row.try_get("schema_name")?,
                object_kind: row.try_get("object_kind")?,
                grantor: acl_principal(row.try_get("grantor_name")?, owners)?,
                grantee: acl_principal(row.try_get("grantee_name")?, owners)?,
                privilege: row.try_get("privilege_type")?,
                grant_option: row.try_get("is_grantable")?,
            })
        })
        .collect::<Result<_, FirstLaunchVerificationError>>()?;
    result.sort();
    Ok(result)
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

async fn verify_table_counts_and_lock(
    tx: &mut Transaction<'_, Postgres>,
    tables: &[TableEntry],
    relations: &[RelationEntry],
    owners: &FreshPretrafficOwnerBindings,
) -> Result<(), FirstLaunchVerificationError> {
    let mut identities: Vec<_> = tables
        .iter()
        .map(|table| (&table.schema, &table.name))
        .collect();
    identities.sort();
    for (schema, table) in identities {
        let expected_owner = relations
            .iter()
            .find(|entry| &entry.schema == schema && &entry.name == table)
            .ok_or_else(|| failure(format!("table {schema}.{table} has no relation entry")))?;
        if expected_owner.owner == "kb_launch_owner" {
            sqlx::query("SET LOCAL ROLE kb_launch_owner")
                .execute(&mut **tx)
                .await?;
        } else {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "SET LOCAL ROLE {}",
                quote_identifier(&owners.app_owner)
            )))
            .execute(&mut **tx)
            .await?;
        }
        let qualified = format!("{}.{}", quote_identifier(schema), quote_identifier(table));
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "LOCK TABLE {qualified} IN ACCESS EXCLUSIVE MODE"
        )))
        .execute(&mut **tx)
        .await?;
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {qualified}"
        )))
        .fetch_one(&mut **tx)
        .await?;
        let expected = tables
            .iter()
            .find(|entry| &entry.schema == schema && &entry.name == table)
            .unwrap()
            .row_count;
        if count != expected {
            return Err(failure(format!(
                "exact row count mismatch for {schema}.{table}: expected {expected}, found {count}"
            )));
        }
    }
    sqlx::query("RESET ROLE").execute(&mut **tx).await?;
    Ok(())
}

fn column_value_expression(column: &ColumnEntry) -> String {
    let reference = format!("value.{}", quote_identifier(&column.name));
    let json_value = if column.r#type == "bytea" {
        format!(
            "to_jsonb(CASE WHEN {reference} IS NULL THEN NULL ELSE encode({reference},'hex') END)"
        )
    } else if column.r#type == "timestamp with time zone" {
        format!(
            "to_jsonb(CASE WHEN {reference} IS NULL THEN NULL ELSE CASE WHEN {reference}='infinity'::timestamptz THEN 'infinity' WHEN {reference}='-infinity'::timestamptz THEN '-infinity' ELSE to_char({reference} AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') END END)"
        )
    } else {
        format!("to_jsonb({reference})")
    };
    format!(
        "jsonb_build_array({}, {}, {json_value})",
        sql_literal(&column.name),
        sql_literal(&column.r#type)
    )
}
fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

async fn actual_seed_rows(
    tx: &mut Transaction<'_, Postgres>,
    allowlist: &Allowlist,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<Vec<SeedRowEntry>, FirstLaunchVerificationError> {
    let mut result = Vec::new();
    for table in allowlist
        .tables
        .iter()
        .filter(|entry| entry.row_count > 0 && entry.name != "schema_migrations")
    {
        let relation = allowlist
            .relations
            .iter()
            .find(|entry| entry.schema == table.schema && entry.name == table.name)
            .ok_or_else(|| {
                failure(format!(
                    "seed table {}.{} has no relation entry",
                    table.schema, table.name
                ))
            })?;
        if relation.owner == "kb_launch_owner" {
            sqlx::query("SET LOCAL ROLE kb_launch_owner")
                .execute(&mut **tx)
                .await?;
        } else {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "SET LOCAL ROLE {}",
                quote_identifier(&owners.app_owner)
            )))
            .execute(&mut **tx)
            .await?;
        }
        let columns: Vec<_> = allowlist
            .columns
            .iter()
            .filter(|column| column.schema == table.schema && column.relation == table.name)
            .collect();
        let key_columns: Vec<_> = columns
            .iter()
            .filter(|column| table.key_columns.contains(&column.name))
            .copied()
            .collect();
        if key_columns.len() != table.key_columns.len() {
            return Err(failure(format!(
                "missing catalog columns for seed key {}.{}",
                table.schema, table.name
            )));
        }
        let value_columns: Vec<_> = columns
            .iter()
            .filter(|column| !table.key_columns.contains(&column.name))
            .copied()
            .collect();
        let key_parts = key_columns
            .iter()
            .map(|column| {
                format!(
                    "jsonb_build_array({},to_jsonb(value.{}))",
                    sql_literal(&column.name),
                    quote_identifier(&column.name)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let value_parts = value_columns
            .iter()
            .map(|column| column_value_expression(column))
            .collect::<Vec<_>>()
            .join(",");
        let qualified = format!(
            "{}.{}",
            quote_identifier(&table.schema),
            quote_identifier(&table.name)
        );
        let query = format!(
            "SELECT jsonb_build_array({key_parts})::text AS canonical_key,jsonb_build_array({value_parts})::text AS canonical_value FROM {qualified} value ORDER BY 1"
        );
        for row in sqlx::query(sqlx::AssertSqlSafe(query))
            .fetch_all(&mut **tx)
            .await?
        {
            let canonical_value: String = row.try_get("canonical_value")?;
            result.push(SeedRowEntry {
                schema: table.schema.clone(),
                table: table.name.clone(),
                key: row.try_get("canonical_key")?,
                value_sha256: hash_hex(canonical_value),
            });
        }
    }
    sqlx::query("RESET ROLE").execute(&mut **tx).await?;
    result.sort();
    Ok(result)
}

#[derive(Serialize)]
struct CatalogDigest<'a> {
    database: &'a DatabaseEntry,
    database_acls: &'a [DatabaseAclEntry],
    extensions: &'a [ExtensionEntry],
    schemas: &'a [SchemaEntry],
    schema_acls: &'a [SchemaAclEntry],
    relations: &'a [RelationEntry],
    columns: &'a [ColumnEntry],
    constraints: &'a [ConstraintEntry],
    indexes: &'a [IndexEntry],
    triggers: &'a [TriggerEntry],
    routines: &'a [RoutineEntry],
    roles: &'a [RoleEntry],
    role_memberships: &'a [RoleMembershipEntry],
    relation_acls: &'a [RelationAclEntry],
    routine_acls: &'a [RoutineAclEntry],
    default_acls: &'a [DefaultAclEntry],
}
#[derive(Serialize)]
struct RowDigest<'a> {
    tables: &'a [TableEntry],
    seed_rows: &'a [SeedRowEntry],
}

fn expected_authority_digests(
    allowlist: &Allowlist,
) -> Result<(String, String), FirstLaunchVerificationError> {
    let catalog = CatalogDigest {
        database: &allowlist.database,
        database_acls: &allowlist.database_acls,
        extensions: &allowlist.extensions,
        schemas: &allowlist.schemas,
        schema_acls: &allowlist.schema_acls,
        relations: &allowlist.relations,
        columns: &allowlist.columns,
        constraints: &allowlist.constraints,
        indexes: &allowlist.indexes,
        triggers: &allowlist.triggers,
        routines: &allowlist.routines,
        roles: &allowlist.roles,
        role_memberships: &allowlist.role_memberships,
        relation_acls: &allowlist.relation_acls,
        routine_acls: &allowlist.routine_acls,
        default_acls: &allowlist.default_acls,
    };
    let rows = RowDigest {
        tables: &allowlist.tables,
        seed_rows: &allowlist.seed_rows,
    };
    Ok((
        hash_hex(serde_json::to_vec(&catalog).map_err(|error| failure(error.to_string()))?),
        hash_hex(serde_json::to_vec(&rows).map_err(|error| failure(error.to_string()))?),
    ))
}

async fn verify_locked(
    tx: &mut Transaction<'_, Postgres>,
    allowlist: &Allowlist,
    manifest: &MigrationManifest,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<(String, String), FirstLaunchVerificationError> {
    for symbol in allowlist
        .extensions
        .iter()
        .map(|entry| &entry.owner)
        .chain(allowlist.schemas.iter().map(|entry| &entry.owner))
        .chain(allowlist.relations.iter().map(|entry| &entry.owner))
        .chain(allowlist.indexes.iter().map(|entry| &entry.owner))
        .chain(allowlist.routines.iter().map(|entry| &entry.owner))
    {
        owner_name(symbol, owners)?;
    }
    let version: i32 =
        sqlx::query_scalar("SELECT current_setting('server_version_num')::integer / 10000")
            .fetch_one(&mut **tx)
            .await?;
    if version != allowlist.postgres_major as i32 {
        return Err(failure(format!(
            "PostgreSQL major mismatch: expected {}, found {version}",
            allowlist.postgres_major
        )));
    }
    verify_owner_contract(tx, owners)
        .await
        .map_err(|error| failure(format!("owner contract: {error}")))?;
    verify_runtime_role_reachability(tx)
        .await
        .map_err(|error| failure(format!("runtime reachability: {error}")))?;
    verify_ledger_and_lifecycle(tx, manifest, owners)
        .await
        .map_err(|error| failure(format!("ledger/lifecycle: {error}")))?;
    verify_table_counts_and_lock(tx, &allowlist.tables, &allowlist.relations, owners)
        .await
        .map_err(|error| failure(format!("table lock/count: {error}")))?;

    let (database, database_acls) = actual_database_and_acls(tx, owners).await?;
    let extensions = actual_extensions(tx, owners).await?;
    let schemas = actual_schemas(tx, owners).await?;
    let schema_acls = actual_schema_acls(tx, owners).await?;
    let relations = actual_relations(tx, owners).await?;
    let columns = actual_columns(tx).await?;
    let constraints = actual_constraints(tx).await?;
    let indexes = actual_indexes(tx, owners).await?;
    let triggers = actual_triggers(tx).await?;
    let routines = actual_routines(tx, owners).await?;
    let roles = actual_roles(tx).await?;
    let memberships = actual_memberships(tx, owners).await?;
    let relation_acls = actual_relation_acls(tx, owners).await?;
    let routine_acls = actual_routine_acls(tx, owners).await?;
    let default_acls = actual_default_acls(tx, owners).await?;
    let seed_rows = actual_seed_rows(tx, allowlist, owners).await?;

    compare_exact(
        "database",
        vec![allowlist.database.clone()],
        vec![database.clone()],
    )?;
    compare_exact(
        "database ACLs",
        allowlist.database_acls.clone(),
        database_acls.clone(),
    )?;
    compare_exact(
        "extensions",
        allowlist.extensions.clone(),
        extensions.clone(),
    )?;
    compare_exact("schemas", allowlist.schemas.clone(), schemas.clone())?;
    compare_exact(
        "schema ACLs",
        allowlist.schema_acls.clone(),
        schema_acls.clone(),
    )?;
    compare_exact("relations", allowlist.relations.clone(), relations.clone())?;
    compare_exact("columns", allowlist.columns.clone(), columns.clone())?;
    compare_exact(
        "constraints",
        allowlist.constraints.clone(),
        constraints.clone(),
    )?;
    compare_exact("indexes", allowlist.indexes.clone(), indexes.clone())?;
    compare_exact("triggers", allowlist.triggers.clone(), triggers.clone())?;
    compare_exact("routines", allowlist.routines.clone(), routines.clone())?;
    compare_exact("roles", allowlist.roles.clone(), roles.clone())?;
    compare_exact(
        "role memberships",
        allowlist.role_memberships.clone(),
        memberships.clone(),
    )?;
    compare_exact(
        "relation ACLs",
        allowlist.relation_acls.clone(),
        relation_acls.clone(),
    )?;
    compare_exact(
        "routine ACLs",
        allowlist.routine_acls.clone(),
        routine_acls.clone(),
    )?;
    compare_exact(
        "default ACLs",
        allowlist.default_acls.clone(),
        default_acls.clone(),
    )?;
    compare_exact("seed rows", allowlist.seed_rows.clone(), seed_rows.clone())?;

    let catalog = CatalogDigest {
        database: &database,
        database_acls: &database_acls,
        extensions: &extensions,
        schemas: &schemas,
        schema_acls: &schema_acls,
        relations: &relations,
        columns: &columns,
        constraints: &constraints,
        indexes: &indexes,
        triggers: &triggers,
        routines: &routines,
        roles: &roles,
        role_memberships: &memberships,
        relation_acls: &relation_acls,
        routine_acls: &routine_acls,
        default_acls: &default_acls,
    };
    let rows = RowDigest {
        tables: &allowlist.tables,
        seed_rows: &seed_rows,
    };
    Ok((
        hash_hex(serde_json::to_vec(&catalog).map_err(|error| failure(error.to_string()))?),
        hash_hex(serde_json::to_vec(&rows).map_err(|error| failure(error.to_string()))?),
    ))
}

async fn require_committed_handoff_and_zero_migrator_backends(
    connection: &mut sqlx::pool::PoolConnection<Postgres>,
) -> Result<(), FirstLaunchVerificationError> {
    let ready: bool = sqlx::query_scalar(
        "SELECT
           NOT migrator.rolcanlogin AND NOT migrator.rolinherit
           AND NOT migrator.rolsuper AND NOT migrator.rolcreatedb
           AND NOT migrator.rolcreaterole AND NOT migrator.rolreplication
           AND NOT migrator.rolbypassrls
           AND pg_catalog.kb_launch_role_password_absent('kb_migrator')
           AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_stat_activity activity
                          WHERE activity.usesysid=migrator.oid)
           AND NOT pg_catalog.pg_has_role('kb_migrator','kb_app_owner','SET')
           AND NOT pg_catalog.pg_has_role('kb_migrator','kb_launch_owner','SET')
           AND NOT pg_catalog.has_function_privilege(
             'kb_migrator','pg_catalog.kb_terminate_residual_migrator_backends()','EXECUTE')
           AND pg_catalog.pg_get_userbyid((SELECT datdba FROM pg_catalog.pg_database
                                           WHERE datname=current_database()))='kb_app_owner'
           AND pg_catalog.pg_get_userbyid((SELECT nspowner FROM pg_catalog.pg_namespace
                                           WHERE nspname='public'))='kb_app_owner'
           AND (SELECT count(*) FROM pg_catalog.pg_auth_members membership
                JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
                JOIN pg_catalog.pg_roles member ON member.oid=membership.member
                WHERE granted.rolname=ANY($1) OR member.rolname=ANY($1))=2
           AND pg_catalog.pg_has_role('kb_first_launch_verifier','kb_app_owner','SET')
           AND pg_catalog.pg_has_role('kb_first_launch_verifier','kb_launch_owner','SET')
         FROM pg_catalog.pg_roles migrator WHERE migrator.rolname='kb_migrator'",
    )
    .bind(GOVERNED_ROLES)
    .fetch_one(&mut **connection)
    .await?;
    if !ready {
        return Err(failure(
            "verifier requires committed handoff topology and exactly zero migrator backends",
        ));
    }
    Ok(())
}

pub async fn verify_fresh_pretraffic_catalog_rows(
    pool: &PgPool,
    owners: &FreshPretrafficOwnerBindings,
) -> Result<VerifiedCatalogRows, FirstLaunchVerificationError> {
    // Parse and close both checked-in authorities before acquiring a connection or lock.
    let allowlist = validated_allowlist(ALLOWLIST)?;
    let manifest = validated_migration_manifest(MIGRATION_MANIFEST)?;
    let allowlist_sha256 = hash_hex(ALLOWLIST);
    let manifest_sha256 = hash_hex(MIGRATION_MANIFEST);
    let expected_digests = expected_authority_digests(&allowlist)?;
    let mut connection = pool.acquire().await?;
    // Phase-2 completion is checked before taking the catalog advisory lock or
    // beginning the transaction which can insert the durable marker. NOLOGIN
    // makes the observed exact-zero backend state stable.
    require_committed_handoff_and_zero_migrator_backends(&mut connection).await?;
    sqlx::query("SELECT pg_catalog.pg_advisory_lock($1)")
        .bind(MIGRATION_LOCK_ID)
        .execute(&mut *connection)
        .await?;
    let result = async {
        let mut tx = connection.begin().await?;
        sqlx::raw_sql("SET LOCAL search_path=pg_catalog; SET LOCAL TimeZone='UTC'; SET LOCAL DateStyle='ISO, YMD'; SET LOCAL IntervalStyle='iso_8601'; SET LOCAL bytea_output='hex'; SET LOCAL extra_float_digits=3;").execute(&mut *tx).await?;
        let (catalog_sha256, rows_sha256) = verify_locked(&mut tx, &allowlist, &manifest, owners)
            .await
            .map_err(|error| failure(format!("pre-marker exact verification: {error}")))?;
        if (catalog_sha256.as_str(), rows_sha256.as_str())
            != (expected_digests.0.as_str(), expected_digests.1.as_str())
        {
            return Err(failure("verified catalog/row digests differ from checked-in authority digests"));
        }
        sqlx::query(
            "INSERT INTO public.production_first_launch_catalog_verifications
             (singleton_key,allowlist_sha256,catalog_sha256,rows_sha256,manifest_sha256)
             VALUES(true,$1,$2,$3,$4)",
        )
        .bind(&allowlist_sha256)
        .bind(&catalog_sha256)
        .bind(&rows_sha256)
        .bind(&manifest_sha256)
        .execute(&mut *tx)
        .await
        .map_err(|error| failure(format!("marker insert: {error}")))?;
        sqlx::query("SELECT pg_catalog.kb_finalize_first_launch_privileges()")
            .execute(&mut *tx)
            .await
            .map_err(|error| failure(format!("verifier finalizer: {error}")))?;
        let finalized: bool = sqlx::query_scalar(
            "SELECT
                NOT (SELECT rolcanlogin OR rolinherit OR rolcreaterole FROM pg_catalog.pg_roles WHERE rolname='kb_migrator')
                AND NOT (SELECT rolcanlogin OR rolinherit FROM pg_catalog.pg_roles WHERE rolname='kb_first_launch_verifier')
                AND NOT pg_catalog.pg_has_role('kb_migrator','kb_app_owner','SET')
                AND NOT pg_catalog.pg_has_role('kb_migrator','kb_launch_owner','SET')
                AND NOT pg_catalog.has_table_privilege('kb_migrator',(
                  SELECT relation.oid FROM pg_catalog.pg_class relation
                  JOIN pg_catalog.pg_namespace namespace ON namespace.oid=relation.relnamespace
                  WHERE namespace.nspname='public'
                    AND relation.relname='production_first_launch_catalog_verifications'),'INSERT')
                AND NOT pg_catalog.has_function_privilege('kb_first_launch_verifier','pg_catalog.kb_launch_role_password_absent(name)','EXECUTE')
                AND NOT pg_catalog.has_function_privilege('kb_first_launch_verifier','pg_catalog.kb_finalize_first_launch_privileges()','EXECUTE')
                AND NOT pg_catalog.has_table_privilege('kb_first_launch_verifier',(
                  SELECT relation.oid FROM pg_catalog.pg_class relation
                  JOIN pg_catalog.pg_namespace namespace ON namespace.oid=relation.relnamespace
                  WHERE namespace.nspname='public'
                    AND relation.relname='production_first_launch_catalog_verifications'),'INSERT')
                AND pg_catalog.pg_get_userbyid((SELECT datdba FROM pg_catalog.pg_database WHERE datname=current_database()))='kb_app_owner'
                AND pg_catalog.pg_get_userbyid((SELECT nspowner FROM pg_catalog.pg_namespace WHERE nspname='public'))='kb_app_owner'
                AND NOT EXISTS(
                  SELECT 1 FROM pg_catalog.pg_auth_members membership
                  JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
                  JOIN pg_catalog.pg_roles member ON member.oid=membership.member
                  WHERE granted.rolname=ANY($1) OR member.rolname=ANY($1))",
        )
        .bind(GOVERNED_ROLES)
        .fetch_one(&mut *tx)
        .await?;
        if !finalized {
            return Err(failure("post-verification migrator privilege finalization mismatch"));
        }
        compare_exact(
            "finalized role memberships",
            allowlist.finalized_role_memberships.clone(),
            actual_memberships(&mut tx, owners).await?,
        )?;
        verify_runtime_role_reachability(&mut tx).await?;
        tx.commit().await?;
        Ok::<_, FirstLaunchVerificationError>((catalog_sha256, rows_sha256))
    }.await;
    let unlock = sqlx::query("SELECT pg_catalog.pg_advisory_unlock($1)")
        .bind(MIGRATION_LOCK_ID)
        .execute(&mut *connection)
        .await;
    let (catalog_sha256, rows_sha256) = result?;
    unlock?;
    Ok(VerifiedCatalogRows {
        allowlist_sha256,
        catalog_sha256,
        rows_sha256,
    })
}

async fn verify_finalized_runtime_topology(
    tx: &mut Transaction<'_, Postgres>,
    allowlist: &Allowlist,
) -> Result<(), FirstLaunchVerificationError> {
    let topology_valid: bool = sqlx::query_scalar(
        "WITH governed(role_name,login,inherit_value,password_absent) AS (VALUES
           ('kb_app_owner',false,false,true),('kb_migrator',false,false,true),
           ('kb_first_launch_verifier',false,false,true),
           ('kb_runtime_api',true,true,false),('kb_runtime_worker',true,true,false),
           ('kb_runtime_retention',true,true,false),
           ('kb_launch_owner',false,false,true),('kb_launch_operator',false,true,true),
           ('kb_launch_router',false,true,true),('kb_launch_ingress',false,true,true),
           ('kb_launch_attestor',false,true,true),('kb_launch_signature_verifier',false,true,true),
           ('kb_launch_reset_dispatcher',false,true,true)),
         actual_database_acl AS (
           SELECT pg_catalog.pg_get_userbyid(acl.grantor) grantor_name,
             CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(acl.grantee) END grantee_name,
             acl.privilege_type,acl.is_grantable
           FROM pg_catalog.pg_database database_value
           CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(database_value.datacl,
             pg_catalog.acldefault('d',database_value.datdba))) acl
           WHERE database_value.datname=current_database()),
         expected_database_acl(grantor_name,grantee_name,privilege_type,is_grantable) AS (VALUES
           ('kb_app_owner','kb_runtime_api','CONNECT',false),
           ('kb_app_owner','kb_runtime_retention','CONNECT',false),
           ('kb_app_owner','kb_runtime_worker','CONNECT',false)),
         actual_schema_acl AS (
           SELECT pg_catalog.pg_get_userbyid(acl.grantor) grantor_name,
             CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(acl.grantee) END grantee_name,
             acl.privilege_type,acl.is_grantable
           FROM pg_catalog.pg_namespace namespace_value
           CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(namespace_value.nspacl,
             pg_catalog.acldefault('n',namespace_value.nspowner))) acl
           WHERE namespace_value.nspname='public'),
         expected_schema_grantee(grantee_name) AS (VALUES
           ('kb_app_owner'),('kb_runtime_api'),('kb_runtime_retention'),('kb_runtime_worker'),('kb_launch_owner'),
           ('kb_launch_operator'),('kb_launch_router'),('kb_launch_ingress'),
           ('kb_launch_attestor'),('kb_launch_signature_verifier'),
           ('kb_launch_reset_dispatcher')),
         expected_schema_acl(grantor_name,grantee_name,privilege_type,is_grantable) AS (
           SELECT 'kb_app_owner',grantee_name,'USAGE',false FROM expected_schema_grantee)
         SELECT
           (SELECT count(*)=13 AND bool_and(role_value.rolcanlogin=governed.login
              AND role_value.rolinherit=governed.inherit_value
              AND (NOT governed.password_absent
                OR pg_catalog.kb_launch_role_password_absent(role_value.rolname))
              AND NOT role_value.rolsuper
              AND NOT role_value.rolcreatedb AND NOT role_value.rolcreaterole
              AND NOT role_value.rolreplication AND NOT role_value.rolbypassrls
              AND role_value.rolconnlimit=-1 AND role_value.rolvaliduntil IS NULL)
            FROM governed JOIN pg_catalog.pg_roles role_value
              ON role_value.rolname=governed.role_name)
           AND NOT EXISTS(
             SELECT 1 FROM pg_catalog.pg_auth_members membership
             JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
             JOIN pg_catalog.pg_roles member ON member.oid=membership.member
             WHERE granted.rolname IN (SELECT role_name FROM governed)
                OR member.rolname IN (SELECT role_name FROM governed))
           AND pg_catalog.pg_get_userbyid((SELECT datdba FROM pg_catalog.pg_database
                 WHERE datname=current_database()))='kb_app_owner'
           AND pg_catalog.pg_get_userbyid((SELECT nspowner FROM pg_catalog.pg_namespace
                 WHERE nspname='public'))='kb_app_owner'
           AND NOT EXISTS((SELECT * FROM actual_database_acl EXCEPT SELECT * FROM expected_database_acl)
                          UNION ALL
                          (SELECT * FROM expected_database_acl EXCEPT SELECT * FROM actual_database_acl))
           AND NOT EXISTS((SELECT * FROM actual_schema_acl EXCEPT SELECT * FROM expected_schema_acl)
                          UNION ALL
                          (SELECT * FROM expected_schema_acl EXCEPT SELECT * FROM actual_schema_acl))",
    )
    .fetch_one(&mut **tx)
    .await?;
    if !topology_valid {
        return Err(failure(
            "finalized first-launch role, membership, database, or schema topology mismatch",
        ));
    }

    let bootstrap_owner: String = sqlx::query_scalar(
        "SELECT pg_catalog.pg_get_userbyid(extowner) FROM pg_catalog.pg_extension WHERE extname='vector'",
    )
    .fetch_one(&mut **tx)
    .await?;
    let owners = FreshPretrafficOwnerBindings {
        app_owner: "kb_app_owner".into(),
        bootstrap_owner,
    };
    let expected_relations: Vec<_> = allowlist
        .relations
        .iter()
        .map(|entry| (&entry.schema, &entry.name, &entry.owner))
        .collect();
    let actual_relation_values = actual_relations(tx, &owners).await?;
    let actual_relation_owners: Vec<_> = actual_relation_values
        .iter()
        .map(|entry| (&entry.schema, &entry.name, &entry.owner))
        .collect();
    if expected_relations != actual_relation_owners {
        return Err(failure("finalized relation ownership topology mismatch"));
    }
    let expected_routines: BTreeSet<_> = allowlist
        .routines
        .iter()
        .map(|entry| {
            (
                entry.schema.clone(),
                entry.name.clone(),
                entry.identity_arguments.replace("public.", ""),
                entry.owner.clone(),
            )
        })
        .collect();
    let actual_routine_values = actual_routines(tx, &owners).await?;
    let actual_routine_owners: BTreeSet<_> = actual_routine_values
        .iter()
        .map(|entry| {
            (
                entry.schema.clone(),
                entry.name.clone(),
                entry.identity_arguments.replace("public.", ""),
                entry.owner.clone(),
            )
        })
        .collect();
    if expected_routines != actual_routine_owners {
        return Err(failure(format!(
            "finalized routine ownership topology mismatch: expected_only={:?}, actual_only={:?}",
            expected_routines.difference(&actual_routine_owners).next(),
            actual_routine_owners.difference(&expected_routines).next()
        )));
    }
    Ok(())
}

/// Runtime services perform an idempotent marker and finalized-topology read.
/// They never invoke migration, handoff, or the zero-row verifier on restart.
pub async fn require_production_first_launch_verified(
    pool: &PgPool,
) -> Result<(), FirstLaunchVerificationError> {
    let allowlist = validated_allowlist(ALLOWLIST)?;
    validated_migration_manifest(MIGRATION_MANIFEST)?;
    let (catalog_sha256, rows_sha256) = expected_authority_digests(&allowlist)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let valid: bool = sqlx::query_scalar(
        "SELECT count(*)=1
           AND bool_and(singleton_key
             AND allowlist_sha256=$1
             AND catalog_sha256=$2
             AND rows_sha256=$3
             AND manifest_sha256=$4)
         FROM public.production_first_launch_catalog_verifications",
    )
    .bind(hash_hex(ALLOWLIST))
    .bind(catalog_sha256)
    .bind(rows_sha256)
    .bind(hash_hex(MIGRATION_MANIFEST))
    .fetch_one(&mut *tx)
    .await?;
    if !valid {
        return Err(failure(
            "mandatory production first-launch verification marker is absent or invalid",
        ));
    }
    verify_finalized_runtime_topology(&mut tx, &allowlist).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_allowlist_is_strict_and_closed() {
        validated_allowlist(ALLOWLIST).unwrap();
        let unknown = ALLOWLIST.replacen(
            "format_version = 1",
            "format_version = 1\nunknown = true",
            1,
        );
        assert!(validated_allowlist(&unknown).is_err());
        let bad_hash = ALLOWLIST.replacen("definition_sha256 = \"", "definition_sha256 = \"A", 1);
        assert!(validated_allowlist(&bad_hash).is_err());
    }

    #[test]
    fn parser_rejects_manifest_name_and_lowercase_valid_checksum_tampering() {
        validated_migration_manifest(MIGRATION_MANIFEST).unwrap();
        let name = MIGRATION_MANIFEST.replacen(
            "name = \"knowledge_base_baseline\"",
            "name = \"knowledge_base_baseline_tampered\"",
            1,
        );
        assert!(validated_migration_manifest(&name).is_err());
        let duplicate = MIGRATION_MANIFEST.replacen("version = 2", "version = 1", 1);
        assert!(validated_migration_manifest(&duplicate).is_err());
        let malformed = MIGRATION_MANIFEST.replacen("sha256 = \"", "sha256 = \"g", 1);
        assert!(validated_migration_manifest(&malformed).is_err());
    }

    #[test]
    fn parser_rejects_omitted_table_count_and_semantic_duplicate_identity() {
        let parsed = validated_allowlist(ALLOWLIST).unwrap();
        let first = parsed
            .tables
            .iter()
            .find(|entry| entry.key_columns.len() == 1)
            .expect("allowlist has a single-column table key");
        let block = format!(
            "[[tables]]\nschema = {:?}\nname = {:?}\nrow_count = {}\nkey_columns = {:?}\n\n",
            first.schema, first.name, first.row_count, first.key_columns
        );
        let omitted = ALLOWLIST.replacen(&block, "", 1);
        assert!(validated_allowlist(&omitted).is_err());

        let relation = &parsed.relations[0];
        let needle = format!(
            "[[relations]]\nschema = {:?}\nname = {:?}\nkind = {:?}\npersistence = {:?}\nowner = {:?}\n",
            relation.schema, relation.name, relation.kind, relation.persistence, relation.owner
        );
        let duplicate = format!("{needle}\n{needle}");
        assert!(validated_allowlist(&ALLOWLIST.replacen(&needle, &duplicate, 1)).is_err());
    }

    #[tokio::test]
    async fn regenerate_catalog_allowlist_when_explicitly_requested() {
        let Ok(path) = std::env::var("KNOWLEDGEBRAIN_REGENERATE_ALLOWLIST") else {
            return;
        };
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let pool = PgPool::connect(&url).await.unwrap();
        let row = sqlx::query("SELECT current_user,pg_catalog.pg_get_userbyid(extowner) AS bootstrap_owner FROM pg_catalog.pg_extension WHERE extname='vector'")
            .fetch_one(&pool).await.unwrap();
        let owners = FreshPretrafficOwnerBindings {
            app_owner: std::env::var("KNOWLEDGEBRAIN_APP_OWNER")
                .unwrap_or_else(|_| row.try_get("current_user").unwrap()),
            bootstrap_owner: row.try_get("bootstrap_owner").unwrap(),
        };
        let mut tx = pool.begin().await.unwrap();
        sqlx::raw_sql("SET LOCAL search_path=pg_catalog; SET LOCAL TimeZone='UTC'; SET LOCAL DateStyle='ISO, YMD'; SET LOCAL IntervalStyle='iso_8601'; SET LOCAL bytea_output='hex'; SET LOCAL extra_float_digits=3;").execute(&mut *tx).await.unwrap();
        let (database, database_acls) = actual_database_and_acls(&mut tx, &owners).await.unwrap();
        let extensions = actual_extensions(&mut tx, &owners).await.unwrap();
        let schemas = actual_schemas(&mut tx, &owners).await.unwrap();
        let schema_acls = actual_schema_acls(&mut tx, &owners).await.unwrap();
        let relations = actual_relations(&mut tx, &owners).await.unwrap();
        let columns = actual_columns(&mut tx).await.unwrap();
        let constraints = actual_constraints(&mut tx).await.unwrap();
        let indexes = actual_indexes(&mut tx, &owners).await.unwrap();
        let triggers = actual_triggers(&mut tx).await.unwrap();
        let routines = actual_routines(&mut tx, &owners).await.unwrap();
        let roles = actual_roles(&mut tx).await.unwrap();
        let role_memberships = actual_memberships(&mut tx, &owners).await.unwrap();
        let relation_acls = actual_relation_acls(&mut tx, &owners).await.unwrap();
        let routine_acls = actual_routine_acls(&mut tx, &owners).await.unwrap();
        let default_acls = actual_default_acls(&mut tx, &owners).await.unwrap();
        let mut tables = Vec::new();
        for relation in relations.iter().filter(|entry| entry.kind == "table") {
            if relation.owner == "kb_launch_owner" {
                sqlx::query("SET LOCAL ROLE kb_launch_owner")
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            } else {
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "SET LOCAL ROLE {}",
                    quote_identifier(&owners.app_owner)
                )))
                .execute(&mut *tx)
                .await
                .unwrap();
            }
            let qualified = format!(
                "{}.{}",
                quote_identifier(&relation.schema),
                quote_identifier(&relation.name)
            );
            let row_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM {qualified}"
            )))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            tables.push(TableEntry {
                schema: relation.schema.clone(),
                name: relation.name.clone(),
                row_count,
                key_columns: constraints
                    .iter()
                    .find(|constraint| {
                        constraint.schema == relation.schema
                            && constraint.relation == relation.name
                            && constraint.kind == "primary_key"
                    })
                    .and_then(|constraint| simple_constraint_columns(&constraint.definition))
                    .unwrap_or_else(|| panic!("missing primary-key columns for {qualified}")),
            });
        }
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
        tables.sort();
        let mut generated = Allowlist {
            format_version: 1,
            postgres_major: 16,
            baseline_slice_count: 3,
            owner_contract: OwnerContract {
                app_owner_symbol: "app_owner".into(),
                bootstrap_owner_symbol: "bootstrap_owner".into(),
                launch_owner_role: "kb_launch_owner".into(),
            },
            database,
            database_acls,
            extensions,
            schemas,
            schema_acls,
            relations,
            columns,
            constraints,
            indexes,
            triggers,
            routines,
            roles,
            role_memberships,
            finalized_role_memberships: Vec::new(),
            relation_acls,
            routine_acls,
            default_acls,
            tables,
            seed_rows: Vec::new(),
        };
        generated.seed_rows = actual_seed_rows(&mut tx, &generated, &owners)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        let text = toml::to_string_pretty(&generated).unwrap();
        validated_allowlist(&text).unwrap();
        std::fs::write(path, text).unwrap();
    }
}
