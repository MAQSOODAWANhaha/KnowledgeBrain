//! Publish a frozen outline into the bidding baseline.

use super::tools::Draft;
use super::{OutlineArtifact, canonical_sha256, project_draft};
use crate::analysis::FrozenInput;
use crate::outline::chapters::AttachmentBinding;
use serde_json::{Value, json};
use sqlx::PgPool;

/// Identity of one acquired worker lease. Epochs never repeat, even when a
/// caller reuses a token after expiry.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
pub struct OutlineLease {
    pub lease_token: Uuid,
    pub lease_epoch: i64,
}
use uuid::Uuid;

/// Project a finished tool draft and write that artifact. Unfinished drafts are not written.
pub async fn publish_finished(
    pool: &PgPool,
    project_id: Uuid,
    run_id: Uuid,
    lease: OutlineLease,
    input: &FrozenInput,
    frozen_input_sha256: &str,
    draft: &Draft,
) -> Result<Value, String> {
    let projected = project_draft(input, frozen_input_sha256, draft)?;
    publish(
        pool,
        project_id,
        run_id,
        lease,
        input,
        &projected.artifact,
        &projected.bindings,
    )
    .await
    .map_err(|error| error.to_string())
}

pub async fn publish(
    pool: &PgPool,
    project_id: Uuid,
    run_id: Uuid,
    lease: OutlineLease,
    input: &FrozenInput,
    artifact: &OutlineArtifact,
    bindings: &[AttachmentBinding],
) -> Result<Value, sqlx::Error> {
    super::validate_publication(input, artifact, bindings).map_err(sqlx::Error::Protocol)?;
    let bytes = canonical_sha256_bytes(artifact)?;
    let binding_json = json!(
        bindings
            .iter()
            .map(|binding| json!({
                "form_id": binding.form_id,
                "chapter_id": binding.chapter_id,
            }))
            .collect::<Vec<_>>()
    );
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,$5,$6,$7,NULL::kb_actor_identity)",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(&artifact.frozen_input_sha256)
    .bind(lease.lease_token)
    .bind(lease.lease_epoch)
    .bind(bytes)
    .bind(binding_json)
    .fetch_one(pool)
    .await
}

fn canonical_sha256_bytes(artifact: &OutlineArtifact) -> Result<Vec<u8>, sqlx::Error> {
    serde_json_canonicalizer::to_vec(artifact)
        .map_err(|error| sqlx::Error::Protocol(format!("outline canonical JSON: {error}")))
}

pub fn outline_sha256(artifact: &OutlineArtifact) -> Result<String, String> {
    canonical_sha256(artifact)
}

/// Load the most recently published outline artifact for a project.
///
/// Used by `bid:content_generate:v2`: response generation always builds on the
/// latest published outline. Returns the full [`OutlineArtifact`] (deserialized
/// from the stored canonical JSON), not just the digest.
pub async fn load_published(pool: &PgPool, project_id: Uuid) -> Result<OutlineArtifact, String> {
    let artifact: Option<Value> = sqlx::query_scalar(
        "SELECT outline.artifact FROM bid_outline_runs run
         JOIN bid_outline_artifacts outline ON outline.sha256=run.outline_sha256
         WHERE run.project_id=$1 AND run.status='published'
         ORDER BY run.updated_at DESC, run.id DESC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("load published outline: {error}"))?;
    let value = artifact.ok_or_else(|| "no published outline for project".to_string())?;
    serde_json::from_value::<OutlineArtifact>(value)
        .map_err(|error| format!("published outline decode: {error}"))
}

#[derive(Debug, Clone, serde::Serialize)]
struct StagedInputObject {
    staging_id: Option<Uuid>,
    occurrence: String,
    object_ref: String,
    digest: String,
    media_type: String,
    byte_length: i64,
}

/// One raw-input preparation session. Every byte write is preceded by a staging
/// reference. Publication promotes the complete manifest in one DB transaction;
/// a crashed preparation leaves only expiring platform-owned staging objects.
pub struct PreparedInputStore {
    pool: PgPool,
    project_id: Uuid,
    publication_id: Uuid,
    objects: tokio::sync::Mutex<std::collections::BTreeMap<String, StagedInputObject>>,
}

impl PreparedInputStore {
    pub fn new(pool: PgPool, project_id: Uuid) -> Self {
        Self {
            pool,
            project_id,
            publication_id: Uuid::new_v4(),
            objects: tokio::sync::Mutex::new(std::collections::BTreeMap::new()),
        }
    }

    async fn stage_bytes(
        &self,
        occurrence: String,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<String, String> {
        if bytes.is_empty() {
            return Err("cannot stage empty tender evidence".into());
        }
        let digest = platform::sha256_hex(bytes);
        let object_ref = platform::object_ref(&digest);
        let mut objects = self.objects.lock().await;
        if let Some(existing) = objects.get(&occurrence) {
            if existing.digest != digest || existing.media_type != media_type {
                return Err("tender preparation occurrence changed content".into());
            }
            if existing.staging_id.is_some() {
                platform::write_blob_async(&digest, bytes)
                    .await
                    .map_err(|e| format!("retry staged tender object write: {e}"))?;
            }
            return Ok(existing.object_ref.clone());
        }
        let staged = StagedInputObject {
            staging_id: Some(Uuid::new_v4()),
            occurrence: occurrence.clone(),
            object_ref: object_ref.clone(),
            digest: digest.clone(),
            media_type: media_type.into(),
            byte_length: i64::try_from(bytes.len()).map_err(|e| e.to_string())?,
        };
        platform::stage_object_upload(
            &self.pool,
            staged.staging_id.expect("new staging identity"),
            &object_ref,
            &digest,
            media_type,
            staged.byte_length,
            None,
        )
        .await
        .map_err(|e| format!("stage tender object: {e}"))?;
        // Keep the cleanup identity even if physical persistence fails.
        objects.insert(occurrence, staged);
        platform::write_blob_async(&digest, bytes)
            .await
            .map_err(|e| format!("write staged tender object: {e}"))?;
        Ok(object_ref)
    }

    pub async fn stage_source_document(
        &self,
        document_id: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<String, String> {
        let extension = std::path::Path::new(file_name)
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let media = match extension.as_str() {
            "pdf" => "application/pdf",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "doc" => "application/msword",
            "xls" => "application/vnd.ms-excel",
            "txt" | "md" => "text/plain",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            _ => "application/octet-stream",
        };
        self.stage_bytes(source_occurrence(document_id), media, bytes)
            .await
    }

    pub async fn publish_prepared_input(&self, input: &FrozenInput) -> Result<String, String> {
        super::frozen::validate_frozen_input_contract(input)?;
        if input.project_id != self.project_id.to_string() {
            return Err("frozen input project differs from preparation owner".into());
        }
        validate_prepared_manifest(input, &*self.objects.lock().await)?;
        let bytes = serde_json_canonicalizer::to_vec(input).map_err(|e| e.to_string())?;
        let digest = platform::sha256_hex(&bytes);
        self.stage_bytes("frozen-json".into(), "application/json", &bytes)
            .await?;
        let mut objects = self.objects.lock().await;
        validate_prepared_manifest(input, &objects)?;
        // Verify current physical bytes before turning temporary references into
        // durable ones. A registry row alone is never proof of a successful upload.
        for object in objects.values() {
            let hash = object.digest.clone();
            let content = tokio::task::spawn_blocking(move || platform::read_blob(&hash))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("verify staged object: {e}"))?;
            if content.len() as i64 != object.byte_length
                || platform::sha256_hex(&content) != object.digest
            {
                return Err("staged tender object digest/length mismatch".into());
            }
        }
        let manifest = serde_json::to_value(objects.values().collect::<Vec<_>>())
            .map_err(|e| e.to_string())?;
        let stored: String = sqlx::query_scalar(
            "SELECT kb_bid_v2_publish_frozen_input($1,$2::kb_sha256,$3,$4,$5,$6,NULL)",
        )
        .bind(self.project_id)
        .bind(&digest)
        .bind(&bytes)
        .bind(&input.document_set_id)
        .bind(self.publication_id)
        .bind(manifest)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("publish frozen input manifest: {e}"))?;
        for object in objects.values_mut() {
            object.staging_id = None;
        }
        Ok(stored)
    }

    /// Request durable expiry on failure. The retained staging row is the work
    /// item, so a crash or unavailable queue cannot strand a deletion handoff.
    /// A committed publication consumed staging and is unaffected by a lost ACK.
    pub async fn abandon(&self) -> Result<(), String> {
        let mut objects = self.objects.lock().await;
        for object in objects.values() {
            if let Some(staging_id) = object.staging_id {
                sqlx::query("SELECT kb_object_upload_request_expiry($1,NULL)")
                    .bind(staging_id)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        objects.clear();
        Ok(())
    }
}

fn validate_prepared_manifest(
    input: &FrozenInput,
    objects: &std::collections::BTreeMap<String, StagedInputObject>,
) -> Result<(), String> {
    let mut required = std::collections::BTreeSet::new();
    if objects.contains_key("frozen-json") {
        required.insert("frozen-json".into());
    }
    for document in &input.documents {
        let id = document.document_id.as_str();
        if id.trim().is_empty() {
            return Err("frozen document has no identity".into());
        }
        let occurrence = source_occurrence(id);
        if objects.get(&occurrence).is_none_or(|object| {
            document.document_revision != object.digest
                || document
                    .source_contract
                    .as_ref()
                    .is_none_or(|contract| contract.document_revision != object.digest)
        }) {
            return Err("frozen document revision differs from staged source bytes".into());
        }
        required.insert(occurrence);
    }
    for source in &input.source_units {
        if let Some(reference) = source.locator["image_ref"].as_str() {
            let unit = source.locator["unit_id"]
                .as_str()
                .ok_or("frozen image has no unit identity")?;
            let occurrence = image_occurrence(&source.document_id, unit);
            if objects
                .get(&occurrence)
                .is_none_or(|entry| entry.object_ref != reference)
            {
                return Err("frozen image does not match its staged object".into());
            }
            required.insert(occurrence);
        }
    }
    if objects
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        != required
    {
        return Err("frozen input object manifest is incomplete or contains extra evidence".into());
    }
    Ok(())
}

fn source_occurrence(document: &str) -> String {
    format!("source:{}", platform::sha256_hex(document.as_bytes()))
}
fn image_occurrence(document: &str, unit: &str) -> String {
    // Length prefixes make (document,unit) unambiguous even if IDs contain ':' or NUL.
    format!(
        "image:{}",
        platform::sha256_hex(
            format!("{}:{document}{}:{unit}", document.len(), unit.len()).as_bytes()
        )
    )
}

#[async_trait::async_trait]
impl super::frozen::TenderImageStore for PreparedInputStore {
    async fn persist_image(
        &self,
        document_id: &str,
        unit_id: &str,
        image: &docparser::ImageRef,
    ) -> Result<String, String> {
        if !matches!(
            image.mime_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp"
        ) {
            return Err("unsupported tender image media type".into());
        }
        self.stage_bytes(
            image_occurrence(document_id, unit_id),
            &image.mime_type,
            &image.data,
        )
        .await
    }
}

#[cfg(test)]
mod prepared_manifest_tests {
    use super::*;

    #[test]
    fn independently_valid_original_bytes_must_match_frozen_source_revision() {
        let expected_bytes = b"the parsed original";
        let different_bytes = b"another valid original";
        let expected = platform::sha256_hex(expected_bytes);
        let different = platform::sha256_hex(different_bytes);
        let input = FrozenInput {
            schema_version: crate::outline::frozen::FROZEN_SCHEMA_VERSION,
            project_id: Uuid::new_v4().to_string(),
            document_set_id: "set".into(),
            documents: vec![{
                let mut document = crate::outline::frozen::tests::python_fixture_input(0)
                    .documents
                    .remove(0);
                document.document_id = "doc".into();
                document.document_revision = expected.clone();
                document.source_contract.as_mut().unwrap().document_revision = expected.clone();
                document
            }],
            document_relations: vec![],
            source_units: vec![],
            structured_forms: vec![],
            decisions: vec![],
        };
        let occurrence = source_occurrence("doc");
        let mut objects = std::collections::BTreeMap::from([(
            occurrence.clone(),
            StagedInputObject {
                staging_id: Some(Uuid::new_v4()),
                occurrence: occurrence.clone(),
                object_ref: platform::object_ref(&different),
                digest: different,
                media_type: "application/pdf".into(),
                byte_length: different_bytes.len() as i64,
            },
        )]);
        assert!(
            validate_prepared_manifest(&input, &objects)
                .unwrap_err()
                .contains("staged source bytes")
        );
        let object = objects.get_mut(&occurrence).unwrap();
        object.digest = expected.clone();
        object.object_ref = platform::object_ref(&expected);
        object.byte_length = expected_bytes.len() as i64;
        assert!(validate_prepared_manifest(&input, &objects).is_ok());
        let mut forged = input;
        forged.documents[0]
            .source_contract
            .as_mut()
            .unwrap()
            .document_revision = "0".repeat(64);
        assert!(validate_prepared_manifest(&forged, &objects).is_err());
    }
}
