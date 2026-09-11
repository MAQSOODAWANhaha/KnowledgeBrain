//! Initial DOCX publication for a fresh authoring round. No legacy Workspace
//! body, quote, assessment, page map or editor session is accepted as input.
//! The caller supplies freshly generated or explicitly selected template bytes,
//! stages/writes them through ObjectRegistry, then publishes the exact identity.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::tender_upload::{TenderUploadError, validate_docx_document};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocxRoundBasis {
    pub document_set_id: Uuid,
    pub document_set_sha256: String,
    pub requirement_set_id: Uuid,
    pub requirement_set_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocxVersionIdentity {
    pub version_id: Uuid,
    pub docx_sha256: String,
}

/// Owns validated bytes so the content hash cannot drift before publication.
/// This validates the Office container, not template suitability or font fidelity.
pub struct InitialDocx {
    bytes: Vec<u8>,
    sha256: String,
}

impl InitialDocx {
    pub fn new(bytes: Vec<u8>) -> Result<Self, TenderUploadError> {
        validate_docx_document(&bytes)?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        Ok(Self { bytes, sha256 })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn object_ref(&self) -> String {
        platform::object_ref(&self.sha256)
    }

    fn publication_input(
        &self,
        basis: &DocxRoundBasis,
        expected: Option<&DocxVersionIdentity>,
    ) -> Value {
        json!({
            "document_set_id": basis.document_set_id,
            "document_set_sha256": basis.document_set_sha256,
            "requirement_set_id": basis.requirement_set_id,
            "requirement_set_sha256": basis.requirement_set_sha256,
            "expected_version_id": expected.map(|value| value.version_id),
            "expected_docx_sha256": expected.map(|value| &value.docx_sha256),
            "docx_sha256": self.sha256,
            "byte_length": self.bytes.len()
        })
    }
}

pub struct CreateDocxRound<'a> {
    pub workspace_id: Uuid,
    pub staging_id: Uuid,
    pub basis: &'a DocxRoundBasis,
    pub expected: Option<&'a DocxVersionIdentity>,
    pub initial_docx: &'a InitialDocx,
    pub actor: &'a str,
    pub idempotency_key: &'a str,
}

/// Publish only after the caller has staged and written `initial_docx.bytes()`.
/// Keep the existing staging cleanup guard armed until this returns success;
/// both a first publication and a successful replay consume matching staging.
/// A failed SQL transaction leaves staging owned by that cleanup guard.
pub async fn create_docx_round(
    pool: &PgPool,
    input: CreateDocxRound<'_>,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_create_docx_round($1,$2,$3,$4::kb_actor_identity,$5)")
        .bind(input.workspace_id)
        .bind(input.staging_id)
        .bind(
            input
                .initial_docx
                .publication_input(input.basis, input.expected),
        )
        .bind(input.actor)
        .bind(input.idempotency_key)
        .fetch_one(pool)
        .await
}

pub async fn get_docx_round_basis(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Option<DocxRoundBasis>, sqlx::Error> {
    let value: Option<sqlx::types::Json<DocxRoundBasis>> =
        sqlx::query_scalar("SELECT kb_bid_v2_get_docx_round_basis($1,$2::kb_actor_identity)")
            .bind(workspace_id)
            .bind(actor)
            .fetch_one(pool)
            .await?;
    Ok(value.map(|value| value.0))
}

pub async fn get_docx_version(
    pool: &PgPool,
    workspace_id: Uuid,
    version_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_docx_version($1,$2,$3::kb_actor_identity)")
        .bind(workspace_id)
        .bind(version_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

/// Authorize and replay a completed create before writing its immutable bytes.
pub async fn replay_docx_round(
    pool: &PgPool,
    input: &CreateDocxRound<'_>,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_replay_docx_round($1,$2,$3::kb_actor_identity,$4)")
        .bind(input.workspace_id)
        .bind(
            input
                .initial_docx
                .publication_input(input.basis, input.expected),
        )
        .bind(input.actor)
        .bind(input.idempotency_key)
        .fetch_one(pool)
        .await
}

pub async fn get_current_docx(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_current_docx($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_editor(
    pool: &PgPool,
    workspace_id: Uuid,
    editor_key: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_docx_editor($1,$2,$3::kb_actor_identity)")
        .bind(workspace_id)
        .bind(editor_key)
        .bind(actor)
        .fetch_one(pool)
        .await
}

/// Exact published version for an acknowledged forcesave; never reads current.
pub async fn get_saved_receipt(
    pool: &PgPool,
    workspace_id: Uuid,
    editor_key: Uuid,
    save_id: Uuid,
    actor: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_docx_saved_receipt($1,$2,$3,$4::kb_actor_identity)")
        .bind(workspace_id)
        .bind(editor_key)
        .bind(save_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

/// Closed DOCX actions validated atomically by the domain function. The HTTP
/// layer supplies typed requests and verifies service capabilities before save/status.
pub async fn editor_command(
    pool: &PgPool,
    workspace_id: Uuid,
    action: &str,
    input: &Value,
    actor: &str,
    key: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_docx_editor_command($1,$2,$3,$4::kb_actor_identity,$5)")
        .bind(workspace_id)
        .bind(action)
        .bind(input)
        .bind(actor)
        .bind(key)
        .fetch_one(pool)
        .await
}

pub async fn replay_editor_save(
    pool: &PgPool,
    workspace_id: Uuid,
    input: &Value,
    actor: &str,
    key: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_replay_docx_editor_save($1,$2,$3::kb_actor_identity,$4)")
        .bind(workspace_id)
        .bind(input)
        .bind(actor)
        .bind(key)
        .fetch_one(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::{Docx, Paragraph, Run};
    use std::io::Cursor;

    fn docx(text: &str) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        Docx::new()
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text(text)))
            .build()
            .pack(&mut buffer)
            .unwrap();
        buffer.into_inner()
    }

    #[test]
    fn validated_initial_document_preserves_exact_bytes_and_hash() {
        let bytes = docx(&Uuid::new_v4().to_string());
        let expected_hash = hex::encode(Sha256::digest(&bytes));
        let document = InitialDocx::new(bytes.clone()).unwrap();
        assert_eq!(document.bytes(), bytes);
        assert_eq!(document.sha256(), expected_hash);
        assert_eq!(document.object_ref(), platform::object_ref(&expected_hash));
    }

    #[test]
    fn corrupt_or_wrong_format_bytes_cannot_be_prepared_for_publication() {
        for bytes in [
            Vec::new(),
            b"not an Office document".to_vec(),
            b"PK".to_vec(),
        ] {
            assert!(InitialDocx::new(bytes).is_err());
        }
        let mut bytes = docx("content");
        bytes.truncate(bytes.len() / 2);
        assert!(InitialDocx::new(bytes).is_err());
    }

    #[test]
    fn round_input_binds_full_basis_and_optional_version_without_legacy_fields() {
        let basis = DocxRoundBasis {
            document_set_id: Uuid::new_v4(),
            document_set_sha256: hex::encode(Sha256::digest(b"collection")),
            requirement_set_id: Uuid::new_v4(),
            requirement_set_sha256: hex::encode(Sha256::digest(b"requirements")),
        };
        let document = InitialDocx::new(docx("new draft")).unwrap();
        let first = document.publication_input(&basis, None);
        assert!(first["expected_version_id"].is_null());
        assert_eq!(first["document_set_id"], basis.document_set_id.to_string());
        assert_eq!(
            first["requirement_set_id"],
            basis.requirement_set_id.to_string()
        );
        let previous = DocxVersionIdentity {
            version_id: Uuid::new_v4(),
            docx_sha256: hex::encode(Sha256::digest(b"old draft")),
        };
        let next = document.publication_input(&basis, Some(&previous));
        assert_eq!(next["expected_version_id"], previous.version_id.to_string());
        assert_eq!(next["expected_docx_sha256"], previous.docx_sha256);
        assert_eq!(next["docx_sha256"], document.sha256());
        assert_eq!(next.as_object().unwrap().len(), 8);
        let mut with_legacy = serde_json::to_value(&basis).unwrap();
        with_legacy["workspace_snapshot"] = json!({"blocks": ["old response"]});
        assert!(serde_json::from_value::<DocxRoundBasis>(with_legacy).is_err());
    }
}
