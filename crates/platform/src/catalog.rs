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
