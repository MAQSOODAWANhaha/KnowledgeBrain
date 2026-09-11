use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use std::{cmp::Ordering, collections::HashMap};
use thiserror::Error;

use crate::{BIDDING_BASELINE, KNOWLEDGE_BASE_BASELINE, SHARED_PLATFORM_BASELINE};

pub const CATALOG_MANIFEST_CONTRACT_VERSION: i32 = 1;
pub const CATALOG_MANIFEST_SCHEMA: &str =
    include_str!("../../../deploy/catalog-manifest-v1.schema.json");
pub const FROZEN_SEED_TABLES_SPEC: &str =
    include_str!("../../../deploy/platform-frozen-seed-tables-v2.json");
const CATALOG_HASH_DOMAIN: &[u8] = b"KB:PlatformCatalogManifest:v1\0";
const APP_OWNER: &str = "kb_app_owner";
const ALLOWLISTED_ROLES: &[&str] = &[
    "kb_app_owner",
    "kb_migrator",
    "kb_runtime_api",
    "kb_runtime_retention",
    "kb_runtime_worker",
];

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog query failed: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("catalog JSON is outside CatalogManifestV1: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported manifest value: {0}")]
    UnsupportedValue(String),
    #[error("invalid frozen seed specification: {0}")]
    InvalidSeedSpec(String),
    #[error("catalog extraction violated its typed contract: {0}")]
    InvalidCatalog(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AclEntry {
    pub grantee: String,
    pub grantor: String,
    pub privilege: String,
    pub is_grantable: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogKind {
    Database,
    Schema,
    Relation,
    View,
    Sequence,
    Type,
    Column,
    Default,
    Constraint,
    Index,
    Trigger,
    Policy,
    Function,
    Role,
    Membership,
    DefaultAcl,
    SeedRow,
}

impl CatalogKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Schema => "schema",
            Self::Relation => "relation",
            Self::View => "view",
            Self::Sequence => "sequence",
            Self::Type => "type",
            Self::Column => "column",
            Self::Default => "default",
            Self::Constraint => "constraint",
            Self::Index => "index",
            Self::Trigger => "trigger",
            Self::Policy => "policy",
            Self::Function => "function",
            Self::Role => "role",
            Self::Membership => "membership",
            Self::DefaultAcl => "default_acl",
            Self::SeedRow => "seed_row",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyRef {
    pub kind: CatalogKind,
    pub schema: Option<String>,
    pub name: String,
    pub identity: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseDefinition {
    pub encoding: String,
    pub collate: String,
    pub ctype: String,
    pub locale_provider: String,
    pub is_template: bool,
    pub allow_connections: bool,
    pub connection_limit: i32,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaDefinition {}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RelationDefinition {
    pub relkind: String,
    pub persistence: String,
    pub row_security: bool,
    pub force_row_security: bool,
    pub replica_identity: String,
    pub partition_key: Option<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewDefinition {
    pub materialized: bool,
    pub check_option: String,
    pub security_barrier: bool,
    pub definition: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceDefinition {
    pub data_type: String,
    pub start: String,
    pub min: String,
    pub max: String,
    pub increment: String,
    pub cycle: bool,
    pub cache: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypeDefinition {
    pub type_kind: String,
    pub category: String,
    pub preferred: bool,
    pub delimiter: String,
    pub element_type: Option<String>,
    pub base_type: Option<String>,
    pub not_null: bool,
    pub default: Option<String>,
    pub enum_labels: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnDefinition {
    pub ordinal: i32,
    #[serde(rename = "type")]
    pub data_type: String,
    pub not_null: bool,
    pub collation: Option<String>,
    pub identity_kind: String,
    pub generated_kind: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionDefinition {
    pub expression: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NamedDefinition {
    pub definition: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDefinition {
    pub command: String,
    pub permissive: bool,
    pub roles: Vec<String>,
    pub using_expr: Option<String>,
    pub check_expr: Option<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionDefinition {
    pub kind: String,
    pub return_type: String,
    pub language: String,
    pub volatility: String,
    pub security_definer: bool,
    pub strict: bool,
    pub parallel: String,
    pub definition: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleDefinition {
    pub superuser: bool,
    pub inherit: bool,
    pub create_role: bool,
    pub create_db: bool,
    pub can_login: bool,
    pub replication: bool,
    pub bypass_rls: bool,
    pub connection_limit: i32,
    pub valid_until: Option<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MembershipDefinition {
    pub role: String,
    pub member: String,
    pub grantor: String,
    pub admin_option: bool,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultAclDefinition {
    pub entries: Vec<AclEntry>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SeedRowDefinition {
    pub primary_key: Map<String, Value>,
    pub values: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum CatalogDefinition {
    Database(DatabaseDefinition),
    Schema(SchemaDefinition),
    Relation(RelationDefinition),
    View(ViewDefinition),
    Sequence(SequenceDefinition),
    Type(TypeDefinition),
    Column(ColumnDefinition),
    Expression(ExpressionDefinition),
    Named(NamedDefinition),
    Policy(PolicyDefinition),
    Function(FunctionDefinition),
    Role(RoleDefinition),
    Membership(MembershipDefinition),
    DefaultAcl(DefaultAclDefinition),
    SeedRow(SeedRowDefinition),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRecord {
    pub kind: CatalogKind,
    pub schema: Option<String>,
    pub name: String,
    pub identity: String,
    pub owner: Option<String>,
    pub definition: CatalogDefinition,
    pub acl: Vec<AclEntry>,
    pub dependencies: Vec<DependencyRef>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenSeedSpec {
    pub schema_version: u32,
    pub tables: Vec<FrozenSeedTable>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenSeedTable {
    pub schema: String,
    pub table: String,
    pub primary_key: Vec<String>,
    /// Absent freezes all rows. Present freezes only these baseline identities;
    /// runtime contract registrations remain ordinary append-only business data.
    pub primary_key_values: Option<Vec<Vec<String>>>,
}

pub fn load_frozen_seed_spec() -> Result<FrozenSeedSpec, CatalogError> {
    let spec: FrozenSeedSpec = serde_json::from_str(FROZEN_SEED_TABLES_SPEC)?;
    if spec.schema_version != 2 {
        return Err(CatalogError::InvalidSeedSpec(format!(
            "schema_version {} is not 2",
            spec.schema_version
        )));
    }
    let mut prior: Option<(&str, &str)> = None;
    for table in &spec.tables {
        validate_identifier(&table.schema)?;
        validate_identifier(&table.table)?;
        if table.primary_key.is_empty() {
            return Err(CatalogError::InvalidSeedSpec(format!(
                "{}.{} has no primary key",
                table.schema, table.table
            )));
        }
        for column in &table.primary_key {
            validate_identifier(column)?;
        }
        validate_seed_keys(table)?;
        if prior.is_some_and(|value| value >= (table.schema.as_str(), table.table.as_str())) {
            return Err(CatalogError::InvalidSeedSpec(
                "tables are not uniquely sorted by UTF-8 (schema,table)".into(),
            ));
        }
        prior = Some((&table.schema, &table.table));
    }
    Ok(spec)
}

fn validate_seed_keys(table: &FrozenSeedTable) -> Result<(), CatalogError> {
    if let Some(keys) = &table.primary_key_values
        && (keys.is_empty()
            || keys.iter().any(|key| {
                key.len() != table.primary_key.len() || key.iter().any(String::is_empty)
            })
            || keys.windows(2).any(|pair| pair[0] >= pair[1]))
    {
        return Err(CatalogError::InvalidSeedSpec(
            "seed primary-key tuples must be nonempty, complete and uniquely sorted".into(),
        ));
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), CatalogError> {
    if value.is_empty()
        || value.len() > 63
        || !value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_lowercase() || (index > 0 && byte.is_ascii_digit())
        })
    {
        return Err(CatalogError::InvalidSeedSpec(format!(
            "identifier {value:?} is outside the closed grammar"
        )));
    }
    Ok(())
}

/// RFC 8785 JSON Canonicalization Scheme bytes, including ECMAScript number rendering.
pub fn jcs_canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CatalogError> {
    Ok(serde_json_canonicalizer::to_vec(value)?)
}

fn utf8_cmp(left: &str, right: &str) -> Ordering {
    left.as_bytes().cmp(right.as_bytes())
}

pub fn sort_catalog_records(records: &mut [CatalogRecord]) {
    for record in records.iter_mut() {
        record.acl.sort_by(acl_cmp);
        record.dependencies.sort_by(dependency_cmp);
        match &mut record.definition {
            CatalogDefinition::Policy(definition) => {
                definition
                    .roles
                    .sort_by(|left, right| utf8_cmp(left, right));
            }
            CatalogDefinition::DefaultAcl(definition) => definition.entries.sort_by(acl_cmp),
            CatalogDefinition::Type(_)
            | CatalogDefinition::SeedRow(_)
            | CatalogDefinition::Database(_)
            | CatalogDefinition::Schema(_)
            | CatalogDefinition::Relation(_)
            | CatalogDefinition::View(_)
            | CatalogDefinition::Sequence(_)
            | CatalogDefinition::Column(_)
            | CatalogDefinition::Expression(_)
            | CatalogDefinition::Named(_)
            | CatalogDefinition::Function(_)
            | CatalogDefinition::Role(_)
            | CatalogDefinition::Membership(_) => {}
        }
    }
    records.sort_by(|left, right| record_key(left).cmp(&record_key(right)));
}

fn record_key(record: &CatalogRecord) -> (&str, &str, &str, &str) {
    (
        record.kind.as_str(),
        record.schema.as_deref().unwrap_or(""),
        &record.name,
        &record.identity,
    )
}
fn acl_cmp(left: &AclEntry, right: &AclEntry) -> Ordering {
    (
        &left.grantee,
        &left.grantor,
        &left.privilege,
        left.is_grantable,
    )
        .cmp(&(
            &right.grantee,
            &right.grantor,
            &right.privilege,
            right.is_grantable,
        ))
}
fn dependency_cmp(left: &DependencyRef, right: &DependencyRef) -> Ordering {
    (
        left.kind.as_str(),
        left.schema.as_deref().unwrap_or(""),
        &left.name,
        &left.identity,
    )
        .cmp(&(
            right.kind.as_str(),
            right.schema.as_deref().unwrap_or(""),
            &right.name,
            &right.identity,
        ))
}

pub fn validate_catalog_manifest(records: &[CatalogRecord]) -> Result<(), CatalogError> {
    for record in records {
        let definition_matches = matches!(
            (record.kind, &record.definition),
            (CatalogKind::Database, CatalogDefinition::Database(_))
                | (CatalogKind::Schema, CatalogDefinition::Schema(_))
                | (CatalogKind::Relation, CatalogDefinition::Relation(_))
                | (CatalogKind::View, CatalogDefinition::View(_))
                | (CatalogKind::Sequence, CatalogDefinition::Sequence(_))
                | (CatalogKind::Type, CatalogDefinition::Type(_))
                | (CatalogKind::Column, CatalogDefinition::Column(_))
                | (CatalogKind::Default, CatalogDefinition::Expression(_))
                | (
                    CatalogKind::Constraint | CatalogKind::Index | CatalogKind::Trigger,
                    CatalogDefinition::Named(_)
                )
                | (CatalogKind::Policy, CatalogDefinition::Policy(_))
                | (CatalogKind::Function, CatalogDefinition::Function(_))
                | (CatalogKind::Role, CatalogDefinition::Role(_))
                | (CatalogKind::Membership, CatalogDefinition::Membership(_))
                | (CatalogKind::DefaultAcl, CatalogDefinition::DefaultAcl(_))
                | (CatalogKind::SeedRow, CatalogDefinition::SeedRow(_))
        );
        if !definition_matches {
            return Err(CatalogError::InvalidCatalog(format!(
                "{} has a mismatched definition shape",
                record.identity
            )));
        }
        match record.kind {
            CatalogKind::Database => {
                if record.schema.is_some() || record.owner.as_deref().is_none_or(str::is_empty) {
                    return Err(CatalogError::InvalidCatalog(format!(
                        "database {} requires null schema and non-empty owner",
                        record.identity
                    )));
                }
            }
            CatalogKind::Role | CatalogKind::Membership => {
                if record.schema.is_some() || record.owner.is_some() {
                    return Err(CatalogError::InvalidCatalog(format!(
                        "{} requires null schema and owner",
                        record.identity
                    )));
                }
            }
            CatalogKind::DefaultAcl => {
                if record
                    .owner
                    .as_deref()
                    .is_none_or(|owner| !ALLOWLISTED_ROLES.contains(&owner))
                {
                    return Err(CatalogError::InvalidCatalog(format!(
                        "default ACL {} has a non-allowlisted owner",
                        record.identity
                    )));
                }
                if record.schema.as_deref().is_some_and(|schema| {
                    schema.is_empty() || extension_definition_namespace_excluded(schema)
                }) {
                    return Err(CatalogError::InvalidCatalog(format!(
                        "default ACL {} has an empty or excluded schema",
                        record.identity
                    )));
                }
            }
            _ => {
                if record.schema.as_deref().is_none_or(str::is_empty)
                    || record.owner.as_deref() != Some(APP_OWNER)
                {
                    return Err(CatalogError::InvalidCatalog(format!(
                        "application object {} requires schema and kb_app_owner",
                        record.identity
                    )));
                }
            }
        }
    }
    Ok(())
}

fn extension_definition_namespace_excluded(namespace: &str) -> bool {
    namespace == "pg_catalog"
        || namespace == "information_schema"
        || namespace.starts_with("pg_toast")
        || namespace.starts_with("pg_temp_")
}

pub fn catalog_manifest_sha256(records: &[CatalogRecord]) -> Result<String, CatalogError> {
    validate_catalog_manifest(records)?;
    let canonical = jcs_canonical_bytes(&records)?;
    let mut digest = Sha256::new();
    digest.update(CATALOG_HASH_DOMAIN);
    digest.update(canonical);
    Ok(hex::encode(digest.finalize()))
}

pub fn exact_baseline_sha256(source: &[u8]) -> String {
    hex::encode(Sha256::digest(source))
}
pub fn shared_baseline_sha256() -> String {
    exact_baseline_sha256(SHARED_PLATFORM_BASELINE.as_bytes())
}
pub fn knowledge_baseline_sha256() -> String {
    exact_baseline_sha256(KNOWLEDGE_BASE_BASELINE.as_bytes())
}
pub fn bidding_baseline_sha256() -> String {
    exact_baseline_sha256(BIDDING_BASELINE.as_bytes())
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CatalogAddress {
    class_id: i64,
    object_id: i64,
    sub_id: i32,
}

struct AddressedRecord {
    address: Option<CatalogAddress>,
    record: CatalogRecord,
}

const CATALOG_SQL: &str = r#"
WITH app_relations AS (
  SELECT c.*, n.nspname, owner_role.rolname AS owner_name
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
  JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=c.relowner
  WHERE owner_role.rolname=$1
    AND c.relpersistence<>'t'
    AND n.nspname NOT IN ('pg_catalog','information_schema')
    AND n.nspname !~ '^pg_(temp|toast)'
    AND NOT EXISTS (
      SELECT 1 FROM pg_catalog.pg_depend extension_dependency
      WHERE extension_dependency.classid='pg_catalog.pg_class'::regclass
        AND extension_dependency.objid=c.oid AND extension_dependency.deptype='e')
), records AS (
SELECT 'pg_catalog.pg_database'::regclass::oid::int8 class_id, d.oid::int8 object_id, 0::int4 sub_id,
 'database' kind, NULL::text schema_name, d.datname name, d.datname identity,
 owner_role.rolname owner_name,
 jsonb_build_object('encoding',pg_catalog.pg_encoding_to_char(d.encoding),'collate',d.datcollate,
   'ctype',d.datctype,'locale_provider',d.datlocprovider::text,'is_template',d.datistemplate,
   'allow_connections',d.datallowconn,'connection_limit',d.datconnlimit) definition,
 COALESCE((SELECT jsonb_agg(jsonb_build_object('grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,
   'grantor',grantor_role.rolname,'privilege',acl.privilege_type,'is_grantable',acl.is_grantable))
   FROM pg_catalog.aclexplode(d.datacl) acl LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb) acl
FROM pg_catalog.pg_database d JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=d.datdba
WHERE d.datname=pg_catalog.current_database()
UNION ALL
SELECT 'pg_catalog.pg_namespace'::regclass::oid::int8,n.oid::int8,0,'schema',n.nspname,n.nspname,n.nspname,owner_role.rolname,
 '{}'::jsonb,
 COALESCE((SELECT jsonb_agg(jsonb_build_object('grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,
   'grantor',grantor_role.rolname,'privilege',acl.privilege_type,'is_grantable',acl.is_grantable))
   FROM pg_catalog.aclexplode(n.nspacl) acl LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb)
FROM pg_catalog.pg_namespace n JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=n.nspowner
WHERE owner_role.rolname=$1 AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_(temp|toast)'
 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_namespace'::regclass AND e.objid=n.oid AND e.deptype='e')
UNION ALL
SELECT 'pg_catalog.pg_class'::regclass::oid::int8,c.oid::int8,0,'relation',c.nspname,c.relname,c.nspname||'.'||c.relname,c.owner_name,
 jsonb_build_object('relkind',c.relkind::text,'persistence',c.relpersistence::text,'row_security',c.relrowsecurity,
   'force_row_security',c.relforcerowsecurity,'replica_identity',c.relreplident::text,
   'partition_key',CASE WHEN c.relkind='p' THEN pg_catalog.pg_get_partkeydef(c.oid) ELSE NULL END),
 COALESCE((SELECT jsonb_agg(jsonb_build_object('grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,
   'grantor',grantor_role.rolname,'privilege',acl.privilege_type,'is_grantable',acl.is_grantable))
   FROM pg_catalog.aclexplode(c.relacl) acl LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb)
FROM app_relations c WHERE c.relkind IN ('r','p','f')
UNION ALL
SELECT 'pg_catalog.pg_class'::regclass::oid::int8,c.oid::int8,0,'view',c.nspname,c.relname,c.nspname||'.'||c.relname,c.owner_name,
 jsonb_build_object('materialized',c.relkind='m',
   'check_option',COALESCE((SELECT option_value FROM pg_catalog.pg_options_to_table(c.reloptions) WHERE option_name='check_option'),''),
   'security_barrier',COALESCE((SELECT option_value::boolean FROM pg_catalog.pg_options_to_table(c.reloptions) WHERE option_name='security_barrier'),false),
   'definition',pg_catalog.pg_get_viewdef(c.oid,false)),
 COALESCE((SELECT jsonb_agg(jsonb_build_object('grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,
   'grantor',grantor_role.rolname,'privilege',acl.privilege_type,'is_grantable',acl.is_grantable))
   FROM pg_catalog.aclexplode(c.relacl) acl LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb)
FROM app_relations c WHERE c.relkind IN ('v','m')
UNION ALL
SELECT 'pg_catalog.pg_class'::regclass::oid::int8,c.oid::int8,0,'sequence',c.nspname,c.relname,c.nspname||'.'||c.relname,c.owner_name,
 jsonb_build_object('data_type',pg_catalog.format_type(s.seqtypid,NULL),'start',s.seqstart::text,'min',s.seqmin::text,
   'max',s.seqmax::text,'increment',s.seqincrement::text,'cycle',s.seqcycle,'cache',s.seqcache::text),
 COALESCE((SELECT jsonb_agg(jsonb_build_object('grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,
   'grantor',grantor_role.rolname,'privilege',acl.privilege_type,'is_grantable',acl.is_grantable))
   FROM pg_catalog.aclexplode(c.relacl) acl LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb)
FROM app_relations c JOIN pg_catalog.pg_sequence s ON s.seqrelid=c.oid WHERE c.relkind='S'
UNION ALL
SELECT 'pg_catalog.pg_type'::regclass::oid::int8,t.oid::int8,0,'type',n.nspname,t.typname,n.nspname||'.'||t.typname,owner_role.rolname,
 jsonb_build_object('type_kind',t.typtype::text,'category',t.typcategory::text,'preferred',t.typispreferred,
  'delimiter',t.typdelim::text,'element_type',CASE WHEN t.typelem<>0 THEN pg_catalog.format_type(t.typelem,NULL) ELSE NULL END,
  'base_type',CASE WHEN t.typbasetype<>0 THEN pg_catalog.format_type(t.typbasetype,t.typtypmod) ELSE NULL END,
  'not_null',t.typnotnull,'default',t.typdefault,
  'enum_labels',COALESCE((SELECT jsonb_agg(e.enumlabel ORDER BY e.enumsortorder) FROM pg_catalog.pg_enum e WHERE e.enumtypid=t.oid),'[]'::jsonb)),
 COALESCE((SELECT jsonb_agg(jsonb_build_object('grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,
   'grantor',grantor_role.rolname,'privilege',acl.privilege_type,'is_grantable',acl.is_grantable))
   FROM pg_catalog.aclexplode(t.typacl) acl LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb)
FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid=t.typnamespace JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=t.typowner
WHERE owner_role.rolname=$1 AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_(temp|toast)'
 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_type'::regclass AND e.objid=t.oid AND e.deptype='e')
UNION ALL
SELECT 'pg_catalog.pg_class'::regclass::oid::int8,c.oid::int8,a.attnum::int4,'column',c.nspname,a.attname,
 c.nspname||'.'||c.relname||'.'||a.attname,c.owner_name,
 jsonb_build_object('ordinal',a.attnum::int4,'type',pg_catalog.format_type(a.atttypid,a.atttypmod),'not_null',a.attnotnull,
  'collation',CASE WHEN a.attcollation=0 THEN NULL ELSE collation_namespace.nspname||'.'||collation_value.collname END,
  'identity_kind',a.attidentity::text,'generated_kind',a.attgenerated::text),
 COALESCE((SELECT jsonb_agg(jsonb_build_object('grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,
   'grantor',grantor_role.rolname,'privilege',acl.privilege_type,'is_grantable',acl.is_grantable))
   FROM pg_catalog.aclexplode(a.attacl) acl LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb)
FROM app_relations c JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
LEFT JOIN pg_catalog.pg_collation collation_value ON collation_value.oid=a.attcollation
LEFT JOIN pg_catalog.pg_namespace collation_namespace ON collation_namespace.oid=collation_value.collnamespace
WHERE c.relkind IN ('r','p','f','v','m') AND a.attnum>0 AND NOT a.attisdropped
 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_class'::regclass
   AND e.objid=c.oid AND e.objsubid=a.attnum AND e.deptype='e')
UNION ALL
SELECT 'pg_catalog.pg_attrdef'::regclass::oid::int8,d.oid::int8,0,'default',c.nspname,a.attname,
 c.nspname||'.'||c.relname||'.'||a.attname,c.owner_name,
 jsonb_build_object('expression',pg_catalog.pg_get_expr(d.adbin,d.adrelid,false)),'[]'::jsonb
FROM app_relations c JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
JOIN pg_catalog.pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum
WHERE c.relkind IN ('r','p','f')
 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_attrdef'::regclass
   AND e.objid=d.oid AND e.deptype='e')
UNION ALL
SELECT 'pg_catalog.pg_constraint'::regclass::oid::int8,k.oid::int8,0,'constraint',c.nspname,k.conname,
 c.nspname||'.'||c.relname||'.'||k.conname,c.owner_name,
 jsonb_build_object('definition',pg_catalog.pg_get_constraintdef(k.oid,false)),'[]'::jsonb
FROM app_relations c JOIN pg_catalog.pg_constraint k ON k.conrelid=c.oid WHERE c.relkind IN ('r','p','f')
 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_constraint'::regclass
   AND e.objid=k.oid AND e.deptype='e')
UNION ALL
SELECT 'pg_catalog.pg_class'::regclass::oid::int8,index_value.oid::int8,0,'index',c.nspname,index_value.relname,
 c.nspname||'.'||index_value.relname,c.owner_name,
 jsonb_build_object('definition',pg_catalog.pg_get_indexdef(index_value.oid,0,false)),'[]'::jsonb
FROM app_relations c JOIN pg_catalog.pg_index index_link ON index_link.indrelid=c.oid
JOIN pg_catalog.pg_class index_value ON index_value.oid=index_link.indexrelid
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_class'::regclass AND e.objid=index_value.oid AND e.deptype='e')
UNION ALL
SELECT 'pg_catalog.pg_trigger'::regclass::oid::int8,t.oid::int8,0,'trigger',c.nspname,t.tgname,
 c.nspname||'.'||c.relname||'.'||t.tgname,c.owner_name,
 jsonb_build_object('definition',pg_catalog.pg_get_triggerdef(t.oid,false)),'[]'::jsonb
FROM app_relations c JOIN pg_catalog.pg_trigger t ON t.tgrelid=c.oid WHERE NOT t.tgisinternal
 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_trigger'::regclass
   AND e.objid=t.oid AND e.deptype='e')
UNION ALL
SELECT 'pg_catalog.pg_policy'::regclass::oid::int8,p.oid::int8,0,'policy',c.nspname,p.polname,
 c.nspname||'.'||c.relname||'.'||p.polname,c.owner_name,
 jsonb_build_object('command',p.polcmd::text,'permissive',p.polpermissive,
   'roles',COALESCE((SELECT jsonb_agg(CASE WHEN role_oid=0 THEN 'PUBLIC' ELSE role_value.rolname END)
      FROM unnest(p.polroles) role_oid LEFT JOIN pg_catalog.pg_roles role_value ON role_value.oid=role_oid),'[]'::jsonb),
   'using_expr',pg_catalog.pg_get_expr(p.polqual,p.polrelid,false),
   'check_expr',pg_catalog.pg_get_expr(p.polwithcheck,p.polrelid,false)),'[]'::jsonb
FROM app_relations c JOIN pg_catalog.pg_policy p ON p.polrelid=c.oid
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_policy'::regclass
   AND e.objid=p.oid AND e.deptype='e')
UNION ALL
SELECT 'pg_catalog.pg_proc'::regclass::oid::int8,p.oid::int8,0,'function',n.nspname,p.proname,
 n.nspname||'.'||p.proname||'('||pg_catalog.pg_get_function_identity_arguments(p.oid)||')',owner_role.rolname,
 jsonb_build_object('kind',p.prokind::text,'return_type',pg_catalog.pg_get_function_result(p.oid),'language',language_value.lanname,
   'volatility',p.provolatile::text,'security_definer',p.prosecdef,'strict',p.proisstrict,'parallel',p.proparallel::text,
   'definition',pg_catalog.pg_get_functiondef(p.oid)),
 COALESCE((SELECT jsonb_agg(jsonb_build_object('grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,
   'grantor',grantor_role.rolname,'privilege',acl.privilege_type,'is_grantable',acl.is_grantable))
   FROM pg_catalog.aclexplode(p.proacl) acl LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb)
FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=p.proowner JOIN pg_catalog.pg_language language_value ON language_value.oid=p.prolang
WHERE owner_role.rolname=$1 AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_(temp|toast)'
 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid='pg_catalog.pg_proc'::regclass AND e.objid=p.oid AND e.deptype='e')
UNION ALL
SELECT NULL::int8,r.oid::int8,0,'role',NULL,r.rolname,r.rolname,NULL,
 jsonb_build_object('superuser',r.rolsuper,'inherit',r.rolinherit,'create_role',r.rolcreaterole,'create_db',r.rolcreatedb,
  'can_login',r.rolcanlogin,'replication',r.rolreplication,'bypass_rls',r.rolbypassrls,'connection_limit',r.rolconnlimit,
  'valid_until',CASE WHEN r.rolvaliduntil IS NULL THEN NULL ELSE to_char(r.rolvaliduntil AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END), '[]'::jsonb
FROM pg_catalog.pg_roles r WHERE r.rolname = ANY($2::text[])
UNION ALL
SELECT NULL::int8,0,0,'membership',NULL,role_value.rolname,
 role_value.rolname||'/'||member_value.rolname||'/'||grantor_value.rolname,NULL,
 jsonb_build_object('role',role_value.rolname,'member',member_value.rolname,'grantor',grantor_value.rolname,'admin_option',membership.admin_option),'[]'::jsonb
FROM pg_catalog.pg_auth_members membership JOIN pg_catalog.pg_roles role_value ON role_value.oid=membership.roleid
JOIN pg_catalog.pg_roles member_value ON member_value.oid=membership.member
JOIN pg_catalog.pg_roles grantor_value ON grantor_value.oid=membership.grantor
WHERE role_value.rolname = ANY($2::text[]) OR member_value.rolname = ANY($2::text[])
UNION ALL
SELECT 'pg_catalog.pg_default_acl'::regclass::oid::int8,d.oid::int8,0,'default_acl',namespace_value.nspname,owner_role.rolname,
 owner_role.rolname||'/'||COALESCE(namespace_value.nspname,'')||'/'||d.defaclobjtype::text,owner_role.rolname,
 jsonb_build_object('entries',COALESCE((SELECT jsonb_agg(jsonb_build_object(
   'grantee',CASE WHEN acl.grantee=0 THEN 'PUBLIC' ELSE grantee_role.rolname END,'grantor',grantor_role.rolname,
   'privilege',acl.privilege_type,'is_grantable',acl.is_grantable)) FROM pg_catalog.aclexplode(d.defaclacl) acl
   LEFT JOIN pg_catalog.pg_roles grantee_role ON grantee_role.oid=acl.grantee
   JOIN pg_catalog.pg_roles grantor_role ON grantor_role.oid=acl.grantor),'[]'::jsonb)),'[]'::jsonb
FROM pg_catalog.pg_default_acl d JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=d.defaclrole
LEFT JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=d.defaclnamespace
WHERE owner_role.rolname = ANY($2::text[])
)
SELECT class_id,object_id,sub_id,kind,schema_name,name,identity,owner_name,definition,acl FROM records
"#;

const DEPENDENCIES_SQL: &str = r#"
WITH raw_dependencies AS (
  SELECT dependency.classid,dependency.objid,dependency.objsubid,
    dependency.refclassid,dependency.refobjid,dependency.refobjsubid
  FROM pg_catalog.pg_depend dependency
  WHERE dependency.deptype NOT IN ('i','e')
), normalized_dependencies AS (
  SELECT
    CASE WHEN rewrite_rule.rulename='_RETURN' AND owning_view.relkind IN ('v','m')
      THEN 'pg_catalog.pg_class'::regclass::oid ELSE dependency.classid END::int8 AS class_id,
    CASE WHEN rewrite_rule.rulename='_RETURN' AND owning_view.relkind IN ('v','m')
      THEN rewrite_rule.ev_class ELSE dependency.objid END::int8 AS object_id,
    CASE WHEN rewrite_rule.rulename='_RETURN' AND owning_view.relkind IN ('v','m')
      THEN 0 ELSE dependency.objsubid END::int4 AS sub_id,
    dependency.refclassid::int8 AS ref_class_id,
    dependency.refobjid::int8 AS ref_object_id,
    dependency.refobjsubid::int4 AS ref_sub_id
  FROM raw_dependencies dependency
  LEFT JOIN pg_catalog.pg_rewrite rewrite_rule
    ON dependency.classid='pg_catalog.pg_rewrite'::regclass AND rewrite_rule.oid=dependency.objid
  LEFT JOIN pg_catalog.pg_class owning_view ON owning_view.oid=rewrite_rule.ev_class
)
SELECT class_id,object_id,sub_id,ref_class_id,ref_object_id,ref_sub_id
FROM normalized_dependencies
"#;

pub async fn build_catalog_manifest(
    connection: &mut PgConnection,
) -> Result<Vec<CatalogRecord>, CatalogError> {
    let rows = sqlx::query(CATALOG_SQL)
        .bind(APP_OWNER)
        .bind(ALLOWLISTED_ROLES)
        .fetch_all(&mut *connection)
        .await?;
    let mut addressed = Vec::with_capacity(rows.len());
    for row in rows {
        let kind_text: String = row.try_get("kind")?;
        let kind = parse_kind(&kind_text)?;
        let definition_value: Value = row.try_get("definition")?;
        let acl_value: Value = row.try_get("acl")?;
        let mut acl: Vec<AclEntry> = serde_json::from_value(acl_value)?;
        acl.sort_by(acl_cmp);
        let definition = parse_definition(kind, definition_value)?;
        let class_id: Option<i64> = row.try_get("class_id")?;
        let address = class_id.map(|class_id| CatalogAddress {
            class_id,
            object_id: row.try_get("object_id").unwrap_or_default(),
            sub_id: row.try_get("sub_id").unwrap_or_default(),
        });
        addressed.push(AddressedRecord {
            address,
            record: CatalogRecord {
                kind,
                schema: row.try_get("schema_name")?,
                name: row.try_get("name")?,
                identity: row.try_get("identity")?,
                owner: row.try_get("owner_name")?,
                definition,
                acl,
                dependencies: Vec::new(),
            },
        });
    }
    add_manifest_dependencies(connection, &mut addressed).await?;
    add_seed_records(connection, &mut addressed).await?;
    let mut records: Vec<_> = addressed.into_iter().map(|value| value.record).collect();
    sort_catalog_records(&mut records);
    validate_catalog_manifest(&records)?;
    // Serialize now so unsupported values cannot escape the builder boundary.
    let _ = jcs_canonical_bytes(&records)?;
    Ok(records)
}

async fn add_manifest_dependencies(
    connection: &mut PgConnection,
    records: &mut [AddressedRecord],
) -> Result<(), CatalogError> {
    let mut targets = HashMap::new();
    for value in records.iter() {
        if let Some(address) = value.address {
            targets.insert(
                address,
                DependencyRef {
                    kind: value.record.kind,
                    schema: value.record.schema.clone(),
                    name: value.record.name.clone(),
                    identity: value.record.identity.clone(),
                },
            );
        }
    }
    let sources: HashMap<_, _> = records
        .iter()
        .enumerate()
        .filter_map(|(index, value)| value.address.map(|address| (address, index)))
        .collect();
    for row in sqlx::query(DEPENDENCIES_SQL)
        .fetch_all(&mut *connection)
        .await?
    {
        let source = CatalogAddress {
            class_id: row.try_get("class_id")?,
            object_id: row.try_get("object_id")?,
            sub_id: row.try_get("sub_id")?,
        };
        let target = CatalogAddress {
            class_id: row.try_get("ref_class_id")?,
            object_id: row.try_get("ref_object_id")?,
            sub_id: row.try_get("ref_sub_id")?,
        };
        if let Some(source_index) = sources.get(&source).copied() {
            if let Some(target_ref) = targets.get(&target)
                && (records[source_index].record.identity != target_ref.identity
                    || records[source_index].record.kind != target_ref.kind)
            {
                records[source_index]
                    .record
                    .dependencies
                    .push(target_ref.clone());
            }
            // PostgreSQL view rules commonly reference only a relation's columns.
            // Preserve those exact column refs and also emit the owning relation ref.
            if records[source_index].record.kind == CatalogKind::View && target.sub_id > 0 {
                let relation_target = CatalogAddress {
                    sub_id: 0,
                    ..target
                };
                if let Some(target_ref) = targets.get(&relation_target) {
                    records[source_index]
                        .record
                        .dependencies
                        .push(target_ref.clone());
                }
            }
        }
    }
    for value in records {
        value.record.dependencies.sort_by(dependency_cmp);
        value.record.dependencies.dedup();
    }
    Ok(())
}

fn parse_kind(value: &str) -> Result<CatalogKind, CatalogError> {
    serde_json::from_value(Value::String(value.into())).map_err(CatalogError::from)
}
fn parse_definition(kind: CatalogKind, value: Value) -> Result<CatalogDefinition, CatalogError> {
    Ok(match kind {
        CatalogKind::Database => CatalogDefinition::Database(serde_json::from_value(value)?),
        CatalogKind::Schema => CatalogDefinition::Schema(serde_json::from_value(value)?),
        CatalogKind::Relation => CatalogDefinition::Relation(serde_json::from_value(value)?),
        CatalogKind::View => CatalogDefinition::View(serde_json::from_value(value)?),
        CatalogKind::Sequence => CatalogDefinition::Sequence(serde_json::from_value(value)?),
        CatalogKind::Type => CatalogDefinition::Type(serde_json::from_value(value)?),
        CatalogKind::Column => CatalogDefinition::Column(serde_json::from_value(value)?),
        CatalogKind::Default => CatalogDefinition::Expression(serde_json::from_value(value)?),
        CatalogKind::Constraint | CatalogKind::Index | CatalogKind::Trigger => {
            CatalogDefinition::Named(serde_json::from_value(value)?)
        }
        CatalogKind::Policy => CatalogDefinition::Policy(serde_json::from_value(value)?),
        CatalogKind::Function => CatalogDefinition::Function(serde_json::from_value(value)?),
        CatalogKind::Role => CatalogDefinition::Role(serde_json::from_value(value)?),
        CatalogKind::Membership => CatalogDefinition::Membership(serde_json::from_value(value)?),
        CatalogKind::DefaultAcl => CatalogDefinition::DefaultAcl(serde_json::from_value(value)?),
        CatalogKind::SeedRow => CatalogDefinition::SeedRow(serde_json::from_value(value)?),
    })
}

#[derive(Debug)]
struct SeedColumn {
    name: String,
    type_name: String,
    element_type_name: Option<String>,
}

/// On a fresh baseline every row is a seed. Reject an incomplete allowlist
/// before recording a receipt; runtime verification later allows new rows.
pub(crate) async fn verify_fresh_seed_selection(
    connection: &mut PgConnection,
) -> Result<(), CatalogError> {
    for table in load_frozen_seed_spec()?.tables {
        if let Some(keys) = table.primary_key_values {
            let query = format!(
                "SELECT count(*) FROM {}.{}",
                quote_identifier(&table.schema),
                quote_identifier(&table.table)
            );
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(query.as_str()))
                .fetch_one(&mut *connection)
                .await?;
            if usize::try_from(count).ok() != Some(keys.len()) {
                return Err(CatalogError::InvalidSeedSpec(
                    "selected keys do not cover every fresh baseline seed".into(),
                ));
            }
        }
    }
    Ok(())
}

async fn add_seed_records(
    connection: &mut PgConnection,
    records: &mut Vec<AddressedRecord>,
) -> Result<(), CatalogError> {
    let spec = load_frozen_seed_spec()?;
    for table in spec.tables {
        let columns = sqlx::query(
            "SELECT attribute.attname,
                    CASE WHEN type_value.typtype='d' THEN scalar_base_type.typname ELSE type_value.typname END AS type_name,
                    CASE WHEN element_type.typtype='d' THEN element_base_type.typname ELSE element_type.typname END AS element_type_name,
                    owner_role.rolname AS owner_name
             FROM pg_catalog.pg_class relation
             JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=relation.relnamespace
             JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=relation.relowner
             JOIN pg_catalog.pg_attribute attribute ON attribute.attrelid=relation.oid
             JOIN pg_catalog.pg_type type_value ON type_value.oid=attribute.atttypid
             LEFT JOIN pg_catalog.pg_type scalar_base_type ON scalar_base_type.oid=type_value.typbasetype
             LEFT JOIN pg_catalog.pg_type element_type ON element_type.oid=type_value.typelem
             LEFT JOIN pg_catalog.pg_type element_base_type ON element_base_type.oid=element_type.typbasetype
             WHERE namespace_value.nspname=$1 AND relation.relname=$2
               AND attribute.attnum>0 AND NOT attribute.attisdropped
             ORDER BY attribute.attnum",
        )
        .bind(&table.schema)
        .bind(&table.table)
        .fetch_all(&mut *connection)
        .await?;
        if columns.is_empty() {
            return Err(CatalogError::InvalidSeedSpec(format!(
                "allowlisted seed table {}.{} is absent",
                table.schema, table.table
            )));
        }
        let owner: String = columns[0].try_get("owner_name")?;
        let columns: Vec<SeedColumn> = columns
            .into_iter()
            .map(|row| {
                Ok(SeedColumn {
                    name: row.try_get("attname")?,
                    type_name: row.try_get("type_name")?,
                    element_type_name: row.try_get("element_type_name")?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()?;
        verify_primary_key(connection, &table).await?;
        let qualified = format!(
            "{}.{}",
            quote_identifier(&table.schema),
            quote_identifier(&table.table)
        );
        let order = table
            .primary_key
            .iter()
            .map(|value| quote_identifier(value))
            .collect::<Vec<_>>()
            .join(",");
        let projection = columns
            .iter()
            .map(seed_projection_entry)
            .collect::<Vec<_>>()
            .join(",");
        // Identifiers originate only in the embedded, validated allowlist. Type-directed
        // projection converts SQL numeric values to text before PostgreSQL JSON handling.
        let key_projection = table
            .primary_key
            .iter()
            .map(|key| format!("seed_value.{}::text", quote_identifier(key)))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "SELECT jsonb_build_object({projection}) AS value FROM {qualified} seed_value
             WHERE $1::jsonb IS NULL OR to_jsonb(ARRAY[{key_projection}]::text[]) IN
               (SELECT value FROM jsonb_array_elements($1::jsonb)) ORDER BY {order}"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
            .bind(table.primary_key_values.as_ref().map(sqlx::types::Json))
            .fetch_all(&mut *connection)
            .await?;
        if table
            .primary_key_values
            .as_ref()
            .is_some_and(|keys| keys.len() != rows.len())
        {
            return Err(CatalogError::InvalidCatalog(format!(
                "baseline seed identity missing from {}.{}",
                table.schema, table.table,
            )));
        }
        for row in rows {
            let raw: Value = row.try_get("value")?;
            let raw = raw.as_object().ok_or_else(|| {
                CatalogError::InvalidCatalog(format!(
                    "seed row {}.{} is not an object",
                    table.schema, table.table
                ))
            })?;
            let mut values = Map::new();
            for column in &columns {
                let value = raw.get(&column.name).ok_or_else(|| {
                    CatalogError::InvalidCatalog(format!(
                        "seed column {}.{}.{} is absent",
                        table.schema, table.table, column.name
                    ))
                })?;
                values.insert(column.name.clone(), encode_seed_value(value, column)?);
            }
            let mut primary_key = Map::new();
            for key in &table.primary_key {
                primary_key.insert(
                    key.clone(),
                    values.get(key).cloned().ok_or_else(|| {
                        CatalogError::InvalidSeedSpec(format!("primary key column {key} is absent"))
                    })?,
                );
            }
            let primary_key_bytes = jcs_canonical_bytes(&Value::Object(primary_key.clone()))?;
            let primary_key_identity = String::from_utf8(primary_key_bytes)
                .map_err(|error| CatalogError::InvalidCatalog(error.to_string()))?;
            records.push(AddressedRecord {
                address: None,
                record: CatalogRecord {
                    kind: CatalogKind::SeedRow,
                    schema: Some(table.schema.clone()),
                    name: table.table.clone(),
                    identity: format!("{}.{}/{primary_key_identity}", table.schema, table.table),
                    owner: Some(owner.clone()),
                    definition: CatalogDefinition::SeedRow(SeedRowDefinition {
                        primary_key,
                        values,
                    }),
                    acl: Vec::new(),
                    dependencies: Vec::new(),
                },
            });
        }
    }
    Ok(())
}

async fn verify_primary_key(
    connection: &mut PgConnection,
    table: &FrozenSeedTable,
) -> Result<(), CatalogError> {
    let actual: Vec<String> = sqlx::query_scalar(
        "SELECT attribute.attname
         FROM pg_catalog.pg_class relation
         JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=relation.relnamespace
         JOIN pg_catalog.pg_index index_value ON index_value.indrelid=relation.oid AND index_value.indisprimary
         JOIN LATERAL unnest(index_value.indkey) WITH ORDINALITY key_value(attnum,ordinal) ON true
         JOIN pg_catalog.pg_attribute attribute ON attribute.attrelid=relation.oid AND attribute.attnum=key_value.attnum
         WHERE namespace_value.nspname=$1 AND relation.relname=$2 ORDER BY key_value.ordinal",
    )
    .bind(&table.schema)
    .bind(&table.table)
    .fetch_all(&mut *connection)
    .await?;
    if actual != table.primary_key {
        return Err(CatalogError::InvalidSeedSpec(format!(
            "{}.{} primary key mismatch: artifact={:?}, catalog={actual:?}",
            table.schema, table.table, table.primary_key
        )));
    }
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn seed_projection_entry(column: &SeedColumn) -> String {
    let key = column.name.replace('\'', "''");
    let identifier = format!("seed_value.{}", quote_identifier(&column.name));
    let scalar_type = column
        .element_type_name
        .as_deref()
        .unwrap_or(&column.type_name);
    let expression = if column.element_type_name.is_some()
        && matches!(
            scalar_type,
            "int2"
                | "int4"
                | "int8"
                | "oid"
                | "numeric"
                | "uuid"
                | "timestamp"
                | "timestamptz"
                | "bytea"
        ) {
        format!("({identifier})::text[]")
    } else if column.element_type_name.is_none()
        && matches!(
            scalar_type,
            "int2"
                | "int4"
                | "int8"
                | "oid"
                | "numeric"
                | "uuid"
                | "timestamp"
                | "timestamptz"
                | "bytea"
        )
    {
        format!("({identifier})::text")
    } else {
        identifier
    };
    format!("'{key}',to_jsonb({expression})")
}

fn encode_seed_value(value: &Value, column: &SeedColumn) -> Result<Value, CatalogError> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    if let Some(element_type) = &column.element_type_name {
        let items = value.as_array().ok_or_else(|| {
            CatalogError::InvalidCatalog(format!(
                "array seed column {} is not an array",
                column.name
            ))
        })?;
        let element = SeedColumn {
            name: column.name.clone(),
            type_name: element_type.clone(),
            element_type_name: None,
        };
        return items
            .iter()
            .map(|value| encode_seed_array_item(value, &element))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    match column.type_name.as_str() {
        "bool" => value
            .as_bool()
            .map(Value::Bool)
            .ok_or_else(|| CatalogError::InvalidCatalog(format!("{} is not boolean", column.name))),
        "int2" | "int4" | "int8" | "oid" | "numeric" => {
            let decimal = match value {
                Value::Number(number) => number.to_string(),
                Value::String(value) => value.clone(),
                _ => {
                    return Err(CatalogError::InvalidCatalog(format!(
                        "{} is not numeric",
                        column.name
                    )));
                }
            };
            Ok(Value::String(canonical_decimal(&decimal)?))
        }
        "float4" | "float8" => Err(CatalogError::UnsupportedValue(format!(
            "floating seed column {}",
            column.name
        ))),
        "uuid" => Ok(Value::String(
            value
                .as_str()
                .ok_or_else(|| {
                    CatalogError::InvalidCatalog(format!("{} is not UUID", column.name))
                })?
                .to_ascii_lowercase(),
        )),
        "timestamptz" => normalize_timestamptz(value, &column.name),
        "timestamp" => normalize_timestamp(value, &column.name),
        "bytea" => {
            let text = value.as_str().ok_or_else(|| {
                CatalogError::InvalidCatalog(format!("{} is not bytea text", column.name))
            })?;
            Ok(Value::String(
                text.strip_prefix("\\x")
                    .unwrap_or(text)
                    .to_ascii_lowercase(),
            ))
        }
        "json" | "jsonb" => Ok(value.clone()),
        _ => Ok(value.clone()),
    }
}

fn encode_seed_array_item(value: &Value, element: &SeedColumn) -> Result<Value, CatalogError> {
    if let Value::Array(items) = value {
        return items
            .iter()
            .map(|value| encode_seed_array_item(value, element))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    encode_seed_value(value, element)
}

fn canonical_decimal(value: &str) -> Result<String, CatalogError> {
    if value.contains(['e', 'E']) {
        return Err(CatalogError::UnsupportedValue(format!(
            "exponent decimal {value}"
        )));
    }
    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or((false, value), |value| (true, value));
    let mut parts = unsigned.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|value| value.is_ascii_digit())
        || fraction.is_some_and(|value| !value.bytes().all(|value| value.is_ascii_digit()))
    {
        return Err(CatalogError::UnsupportedValue(format!(
            "invalid decimal {value}"
        )));
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.unwrap_or_default().trim_end_matches('0');
    let zero = integer == "0" && fraction.is_empty();
    Ok(format!(
        "{}{}{}",
        if negative && !zero { "-" } else { "" },
        integer,
        if fraction.is_empty() {
            String::new()
        } else {
            format!(".{fraction}")
        }
    ))
}

fn normalize_timestamptz(value: &Value, column: &str) -> Result<Value, CatalogError> {
    let text = value
        .as_str()
        .ok_or_else(|| CatalogError::InvalidCatalog(format!("{column} is not a timestamp")))?;
    let parsed = DateTime::parse_from_rfc3339(text)
        .or_else(|_| DateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f%#z"))
        .map_err(|error| CatalogError::InvalidCatalog(format!("{column}: {error}")))?;
    Ok(Value::String(
        parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Micros, true),
    ))
}
fn normalize_timestamp(value: &Value, column: &str) -> Result<Value, CatalogError> {
    let text = value
        .as_str()
        .ok_or_else(|| CatalogError::InvalidCatalog(format!("{column} is not a timestamp")))?;
    let parsed = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f"))
        .map_err(|error| CatalogError::InvalidCatalog(format!("{column}: {error}")))?;
    Ok(Value::String(
        parsed
            .and_utc()
            .to_rfc3339_opts(SecondsFormat::Micros, true),
    ))
}
pub fn extension_definition_is_excluded(
    namespace: &str,
    is_temporary: bool,
    is_extension_owned: bool,
) -> bool {
    namespace == "pg_catalog"
        || namespace == "information_schema"
        || namespace.starts_with("pg_toast")
        || namespace.starts_with("pg_temp_")
        || is_temporary
        || is_extension_owned
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_record(
        kind: CatalogKind,
        name: &str,
        identity: &str,
        definition: CatalogDefinition,
    ) -> CatalogRecord {
        CatalogRecord {
            kind,
            schema: Some("public".into()),
            name: name.into(),
            identity: identity.into(),
            owner: Some(APP_OWNER.into()),
            definition,
            acl: vec![],
            dependencies: vec![],
        }
    }

    #[test]
    fn utf16_key_order_is_rfc8785_order_not_unicode_scalar_order() {
        let value = json!({"\u{e000}": 1, "\u{10000}": 2, "a": 3});
        assert_eq!(
            String::from_utf8(jcs_canonical_bytes(&value).unwrap()).unwrap(),
            "{\"a\":3,\"𐀀\":2,\"\":1}"
        );
        assert_eq!(
            String::from_utf8(
                jcs_canonical_bytes(&json!({"fraction": 1.5, "large": 1e30, "small": 1e-7}))
                    .unwrap()
            )
            .unwrap(),
            "{\"fraction\":1.5,\"large\":1e+30,\"small\":1e-7}"
        );
    }

    #[test]
    fn acl_and_dependency_goldens_use_utf8_byte_tuple_order() {
        let mut record = base_record(
            CatalogKind::Relation,
            "items",
            "public.items",
            CatalogDefinition::Relation(RelationDefinition {
                relkind: "r".into(),
                persistence: "p".into(),
                row_security: false,
                force_row_security: false,
                replica_identity: "d".into(),
                partition_key: None,
            }),
        );
        record.acl = vec![
            AclEntry {
                grantee: "z".into(),
                grantor: "a".into(),
                privilege: "SELECT".into(),
                is_grantable: false,
            },
            AclEntry {
                grantee: "PUBLIC".into(),
                grantor: "z".into(),
                privilege: "SELECT".into(),
                is_grantable: false,
            },
            AclEntry {
                grantee: "z".into(),
                grantor: "a".into(),
                privilege: "INSERT".into(),
                is_grantable: true,
            },
        ];
        record.dependencies = vec![
            DependencyRef {
                kind: CatalogKind::Type,
                schema: Some("public".into()),
                name: "z".into(),
                identity: "public.z".into(),
            },
            DependencyRef {
                kind: CatalogKind::Schema,
                schema: None,
                name: "public".into(),
                identity: "public".into(),
            },
        ];
        sort_catalog_records(std::slice::from_mut(&mut record));
        assert_eq!(
            record
                .acl
                .iter()
                .map(|value| value.grantee.as_str())
                .collect::<Vec<_>>(),
            ["PUBLIC", "z", "z"]
        );
        assert_eq!(record.acl[1].privilege, "INSERT");
        assert_eq!(
            record
                .dependencies
                .iter()
                .map(|value| value.kind)
                .collect::<Vec<_>>(),
            [CatalogKind::Schema, CatalogKind::Type]
        );
    }

    #[test]
    fn inherited_owner_child_and_overloaded_function_identities_are_distinct() {
        let child = base_record(
            CatalogKind::Column,
            "payload",
            "public.items.payload",
            CatalogDefinition::Column(ColumnDefinition {
                ordinal: 2,
                data_type: "jsonb".into(),
                not_null: true,
                collation: None,
                identity_kind: "".into(),
                generated_kind: "".into(),
            }),
        );
        assert_eq!(child.owner.as_deref(), Some(APP_OWNER));
        let first = base_record(
            CatalogKind::Function,
            "lookup",
            "public.lookup(uuid)",
            CatalogDefinition::Function(FunctionDefinition {
                kind: "f".into(),
                return_type: "text".into(),
                language: "sql".into(),
                volatility: "s".into(),
                security_definer: false,
                strict: true,
                parallel: "s".into(),
                definition: "CREATE FUNCTION lookup(uuid)".into(),
            }),
        );
        let second = CatalogRecord {
            identity: "public.lookup(text)".into(),
            ..first.clone()
        };
        assert_ne!(first.identity, second.identity);
    }

    #[test]
    fn seed_typed_values_and_identity_are_byte_stable() {
        let numeric = SeedColumn {
            name: "amount".into(),
            type_name: "numeric".into(),
            element_type_name: None,
        };
        let timestamp = SeedColumn {
            name: "at".into(),
            type_name: "timestamptz".into(),
            element_type_name: None,
        };
        let bytes = SeedColumn {
            name: "bytes".into(),
            type_name: "bytea".into(),
            element_type_name: None,
        };
        assert_eq!(
            encode_seed_value(
                &json!("12345678901234567890.12345678901234567890"),
                &numeric
            )
            .unwrap(),
            json!("12345678901234567890.1234567890123456789")
        );
        assert_eq!(
            encode_seed_value(&json!("2026-01-01T08:00:00+08:00"), &timestamp).unwrap(),
            json!("2026-01-01T00:00:00.000000Z")
        );
        assert_eq!(
            encode_seed_value(&json!("\\xA0ff"), &bytes).unwrap(),
            json!("a0ff")
        );
        let numeric_array = SeedColumn {
            name: "matrix".into(),
            type_name: "_numeric".into(),
            element_type_name: Some("numeric".into()),
        };
        assert_eq!(
            encode_seed_value(&json!([["1.2300", null], ["-0.00", "2"]]), &numeric_array).unwrap(),
            json!([["1.23", null], ["0", "2"]])
        );
        let key = json!({"version":"1","contract_key":"document.process"});
        assert_eq!(
            String::from_utf8(jcs_canonical_bytes(&key).unwrap()).unwrap(),
            "{\"contract_key\":\"document.process\",\"version\":\"1\"}"
        );
    }

    #[test]
    fn extension_and_system_definition_omission_is_explicit() {
        assert!(extension_definition_is_excluded("public", false, true));
        assert!(extension_definition_is_excluded("pg_catalog", false, false));
        assert!(extension_definition_is_excluded("pg_temp_4", true, false));
        assert!(!extension_definition_is_excluded("public", false, false));
    }

    #[test]
    fn domain_hash_replays_exact_canonical_bytes() {
        let record = base_record(
            CatalogKind::Schema,
            "public",
            "public",
            CatalogDefinition::Schema(SchemaDefinition {}),
        );
        let records = vec![record];
        let bytes = jcs_canonical_bytes(&records).unwrap();
        let mut replay = Sha256::new();
        replay.update(b"KB:PlatformCatalogManifest:v1\0");
        replay.update(&bytes);
        assert_eq!(
            catalog_manifest_sha256(&records).unwrap(),
            hex::encode(replay.finalize())
        );
        assert_eq!(
            catalog_manifest_sha256(&records).unwrap(),
            "3c95bd75c891894ee5d7af7e2efab1a8dcad65ca120300556684ab48b68c069a"
        );
    }

    #[test]
    fn frozen_seed_spec_is_closed_sorted_and_exact() {
        let spec = load_frozen_seed_spec().unwrap();
        assert_eq!(spec.schema_version, 2);
        assert_eq!(
            spec.tables
                .iter()
                .map(|table| (
                    table.schema.as_str(),
                    table.table.as_str(),
                    table.primary_key.join(",")
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "public",
                    "application_maintenance_gate",
                    "singleton_key".into()
                ),
                (
                    "public",
                    "bid_attachment_preparation_contract_artifacts",
                    "id".into()
                ),
                ("public", "bid_authoring_contract_artifacts", "id".into()),
                ("public", "bid_render_style_contract_artifacts", "id".into()),
                ("public", "bid_renderer_contract_artifacts", "id".into()),
                ("public", "platform_role_contracts", "role_name".into()),
                (
                    "public",
                    "queue_contract_artifacts",
                    "contract_key,version".into()
                ),
                ("public", "queue_contract_current", "contract_key".into()),
            ]
        );
        assert!(
            serde_json::from_str::<FrozenSeedSpec>(
                r#"{"schema_version":1,"tables":[],"extra":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn selected_seed_keys_reject_incomplete_duplicate_and_unsorted_tuples() {
        let mut table = FrozenSeedTable {
            schema: "public".into(),
            table: "contracts".into(),
            primary_key: vec!["name".into(), "version".into()],
            primary_key_values: None,
        };
        assert!(validate_seed_keys(&table).is_ok());
        for keys in [
            json!([]),
            json!([["a"]]),
            json!([["a", ""]]),
            json!([["a", "1"], ["a", "1"]]),
            json!([["b", "1"], ["a", "1"]]),
        ] {
            table.primary_key_values = Some(serde_json::from_value(keys).unwrap());
            assert!(validate_seed_keys(&table).is_err());
        }
        table.primary_key_values = Some(vec![
            vec!["a".into(), "1".into()],
            vec!["b".into(), "2".into()],
        ]);
        assert!(validate_seed_keys(&table).is_ok());
    }

    fn top_level_insert_tables(sql: &str) -> Vec<String> {
        let mut inside_dollar_quote = false;
        let mut tables = Vec::new();
        for line in sql.lines() {
            if !inside_dollar_quote
                && let Some(rest) = line.trim_start().strip_prefix("INSERT INTO ")
            {
                tables.push(
                    rest.split(|value: char| value.is_ascii_whitespace() || value == '(')
                        .next()
                        .unwrap()
                        .trim_start_matches("public.")
                        .to_owned(),
                );
            }
            if line.matches("$$").count() % 2 == 1 {
                inside_dollar_quote = !inside_dollar_quote;
            }
        }
        tables.sort();
        tables.dedup();
        tables
    }

    #[test]
    fn frozen_seed_artifact_equals_top_level_baseline_inserts() {
        let spec = load_frozen_seed_spec().unwrap();
        let mut artifact: Vec<_> = spec.tables.into_iter().map(|table| table.table).collect();
        artifact.sort();
        let mut actual = Vec::new();
        for sql in [
            SHARED_PLATFORM_BASELINE,
            KNOWLEDGE_BASE_BASELINE,
            BIDDING_BASELINE,
        ] {
            actual.extend(top_level_insert_tables(sql));
        }
        actual.sort();
        actual.dedup();
        assert_eq!(artifact, actual);
    }

    #[test]
    fn compiled_baseline_hashes_replay_exact_embedded_bytes() {
        assert_eq!(
            shared_baseline_sha256(),
            exact_baseline_sha256(SHARED_PLATFORM_BASELINE.as_bytes())
        );
        assert_eq!(
            knowledge_baseline_sha256(),
            exact_baseline_sha256(KNOWLEDGE_BASE_BASELINE.as_bytes())
        );
        assert_eq!(
            bidding_baseline_sha256(),
            exact_baseline_sha256(BIDDING_BASELINE.as_bytes())
        );
    }

    fn all_kind_fixture_values() -> Vec<Value> {
        let definitions = vec![
            (
                "database",
                json!({"encoding":"UTF8","collate":"C","ctype":"C","locale_provider":"c","is_template":false,"allow_connections":true,"connection_limit":-1}),
            ),
            ("schema", json!({})),
            (
                "relation",
                json!({"relkind":"r","persistence":"p","row_security":false,"force_row_security":false,"replica_identity":"d","partition_key":null}),
            ),
            (
                "view",
                json!({"materialized":false,"check_option":"","security_barrier":false,"definition":" SELECT 1;"}),
            ),
            (
                "sequence",
                json!({"data_type":"bigint","start":"1","min":"1","max":"9223372036854775807","increment":"1","cycle":false,"cache":"1"}),
            ),
            (
                "type",
                json!({"type_kind":"e","category":"E","preferred":false,"delimiter":",","element_type":null,"base_type":null,"not_null":false,"default":null,"enum_labels":["a","b"]}),
            ),
            (
                "column",
                json!({"ordinal":1,"type":"text","not_null":true,"collation":null,"identity_kind":"","generated_kind":""}),
            ),
            ("default", json!({"expression":"'x'::text"})),
            ("constraint", json!({"definition":"PRIMARY KEY (id)"})),
            (
                "index",
                json!({"definition":"CREATE UNIQUE INDEX x ON public.t USING btree (id)"}),
            ),
            (
                "trigger",
                json!({"definition":"CREATE TRIGGER x BEFORE INSERT ON public.t FOR EACH ROW EXECUTE FUNCTION f()"}),
            ),
            (
                "policy",
                json!({"command":"r","permissive":true,"roles":["PUBLIC"],"using_expr":null,"check_expr":null}),
            ),
            (
                "function",
                json!({"kind":"f","return_type":"integer","language":"sql","volatility":"v","security_definer":false,"strict":false,"parallel":"u","definition":"CREATE FUNCTION public.f() RETURNS integer LANGUAGE sql AS $$ SELECT 1 $$\n"}),
            ),
            (
                "role",
                json!({"superuser":false,"inherit":true,"create_role":false,"create_db":false,"can_login":true,"replication":false,"bypass_rls":false,"connection_limit":-1,"valid_until":null}),
            ),
            (
                "membership",
                json!({"role":"kb_app_owner","member":"kb_migrator","grantor":"postgres","admin_option":false}),
            ),
            ("default_acl", json!({"entries":[]})),
            (
                "seed_row",
                json!({"primary_key":{"id":"1"},"values":{"id":"1","payload":{"fraction":1.25,"exponent":1e30}}}),
            ),
        ];
        definitions
            .into_iter()
            .map(|(kind, definition)| {
                let (schema, owner) = match kind {
                    "database" => (Value::Null, json!("postgres")),
                    "role" | "membership" => (Value::Null, Value::Null),
                    "default_acl" => (Value::Null, json!(APP_OWNER)),
                    _ => (json!("public"), json!(APP_OWNER)),
                };
                json!({
                    "kind":kind,"schema":schema,"name":kind,"identity":kind,"owner":owner,
                    "definition":definition,"acl":[],"dependencies":[]
                })
            })
            .collect()
    }

    #[test]
    fn every_kind_has_positive_and_owner_schema_negative_validation_fixtures() {
        let schema: Value = serde_json::from_str(CATALOG_MANIFEST_SCHEMA).unwrap();
        let validator = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&schema)
            .unwrap();
        let fixtures = all_kind_fixture_values();
        for positive in &fixtures {
            assert!(
                validator.is_valid(&json!([positive.clone()])),
                "positive {positive}"
            );
            let typed: CatalogRecord = serde_json::from_value(positive.clone()).unwrap();
            validate_catalog_manifest(std::slice::from_ref(&typed)).unwrap();
            let mut negative = positive.clone();
            let kind = negative["kind"].as_str().unwrap();
            if kind == "database" {
                negative["schema"] = json!("public");
            } else if matches!(kind, "role" | "membership") {
                negative["owner"] = json!(APP_OWNER);
            } else {
                negative["owner"] = json!("wrong_owner");
            }
            assert!(
                !validator.is_valid(&json!([negative.clone()])),
                "negative {negative}"
            );
            let typed: CatalogRecord = serde_json::from_value(negative).unwrap();
            assert!(validate_catalog_manifest(std::slice::from_ref(&typed)).is_err());
        }

        for (kind, field) in [("database", "owner"), ("default_acl", "schema")] {
            let mut negative = fixtures
                .iter()
                .find(|fixture| fixture["kind"] == kind)
                .unwrap()
                .clone();
            negative[field] = json!("");
            assert!(
                !validator.is_valid(&json!([negative.clone()])),
                "empty field must fail schema validation: {negative}"
            );
            let typed: CatalogRecord = serde_json::from_value(negative).unwrap();
            assert!(validate_catalog_manifest(std::slice::from_ref(&typed)).is_err());
        }
    }

    #[test]
    fn manifest_schema_accepts_all_typed_golden_records_and_is_closed() {
        let schema: Value = serde_json::from_str(CATALOG_MANIFEST_SCHEMA).unwrap();
        let validator = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&schema)
            .unwrap();
        assert_eq!(
            schema["$defs"]["record"]["oneOf"].as_array().unwrap().len(),
            17
        );
        for shape in [
            "databaseRecordShape",
            "schemaRecordShape",
            "relationRecordShape",
            "viewRecordShape",
            "sequenceRecordShape",
            "typeRecordShape",
            "columnRecordShape",
            "defaultRecordShape",
            "constraintRecordShape",
            "indexRecordShape",
            "triggerRecordShape",
            "policyRecordShape",
            "functionRecordShape",
            "roleRecordShape",
            "membershipRecordShape",
            "defaultAclRecordShape",
            "seedRowRecordShape",
        ] {
            assert_eq!(
                schema["$defs"][shape]["additionalProperties"], false,
                "{shape}"
            );
            assert_eq!(
                schema["$defs"][shape]["required"].as_array().unwrap().len(),
                8,
                "{shape}"
            );
        }
        let records = vec![base_record(
            CatalogKind::Schema,
            "public",
            "public",
            CatalogDefinition::Schema(SchemaDefinition {}),
        )];
        let value = serde_json::to_value(records).unwrap();
        assert!(validator.is_valid(&value));
        let mut invalid = value;
        invalid[0]
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), Value::Null);
        assert!(!validator.is_valid(&invalid));
    }

    #[test]
    fn extraction_query_freezes_all_kinds_and_non_pretty_definitions() {
        for kind in [
            "database",
            "schema",
            "relation",
            "view",
            "sequence",
            "type",
            "column",
            "default",
            "constraint",
            "index",
            "trigger",
            "policy",
            "function",
            "role",
            "membership",
            "default_acl",
        ] {
            assert!(CATALOG_SQL.contains(&format!("'{kind}'")), "missing {kind}");
        }
        for non_pretty in [
            "pg_get_expr(d.adbin,d.adrelid,false)",
            "pg_get_constraintdef(k.oid,false)",
            "pg_get_indexdef(index_value.oid,0,false)",
            "pg_get_triggerdef(t.oid,false)",
            "pg_get_viewdef(c.oid,false)",
        ] {
            assert!(CATALOG_SQL.contains(non_pretty), "missing {non_pretty}");
        }
        assert!(CATALOG_SQL.contains("deptype='e'"));
        assert!(CATALOG_SQL.contains(
            "role_value.rolname = ANY($2::text[]) OR member_value.rolname = ANY($2::text[])"
        ));
        assert!(!CATALOG_SQL.contains(
            "role_value.rolname = ANY($2::text[]) AND member_value.rolname = ANY($2::text[])"
        ));
        assert!(DEPENDENCIES_SQL.contains("deptype NOT IN ('i','e')"));
        assert!(!CATALOG_SQL.contains("acldefault"));
        assert!(!CATALOG_SQL.contains("jsonb::text"));
    }
}
