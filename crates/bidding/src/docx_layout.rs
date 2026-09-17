//! S4-B page-locate and point-edit ports (§12.3).
//! This is not an Agent and does not publish a triad.
//! Text search is not a locate implementation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocateError {
    Unavailable {
        reason: String,
    },
    MissingAnchor {
        anchor_id: String,
    },
    Ambiguous {
        anchor_id: String,
        candidates: usize,
    },
    DuplicateAnchor {
        anchor_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    Unavailable { reason: String },
    StaleBaseline { expected_sha256: String },
    MissingAnchor { anchor_id: String },
    ConcurrentEdit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocateHit {
    pub anchor_id: String,
    pub page: u32,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveReceipt {
    pub version_id: String,
    pub docx_sha256: String,
}

pub trait PageLocatePort {
    fn locate(
        &self,
        docx: &[u8],
        pdf: &[u8],
        anchors: &[Anchor],
    ) -> Result<Vec<LocateHit>, LocateError>;
}

pub trait PointEditPort {
    fn edit(
        &self,
        session: &str,
        baseline_sha256: &str,
        anchor: &Anchor,
        expected_old: &str,
        new_text: &str,
    ) -> Result<SaveReceipt, EditError>;
}

#[derive(Debug, Default)]
pub struct UnavailableLocate;

impl PageLocatePort for UnavailableLocate {
    fn locate(
        &self,
        _docx: &[u8],
        _pdf: &[u8],
        _anchors: &[Anchor],
    ) -> Result<Vec<LocateHit>, LocateError> {
        Err(LocateError::Unavailable {
            reason:
                "page-locate port is not proven on this deployment; text search is not a substitute"
                    .into(),
        })
    }
}

#[derive(Debug, Default)]
pub struct UnavailableEdit;

impl PointEditPort for UnavailableEdit {
    fn edit(
        &self,
        _session: &str,
        _baseline_sha256: &str,
        _anchor: &Anchor,
        _expected_old: &str,
        _new_text: &str,
    ) -> Result<SaveReceipt, EditError> {
        Err(EditError::Unavailable {
            reason: "point-edit port is not proven on this deployment".into(),
        })
    }
}

/// Diagnostic only. Duplicate hits prove this cannot be a locate port.
pub fn pdf_text_search_pages(pdf: &[u8], query: &str) -> Result<Vec<u32>, String> {
    let document = lopdf::Document::load_mem(pdf).map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    for (number, _) in document.get_pages() {
        let text = document.extract_text(&[number]).unwrap_or_default();
        if text.contains(query) {
            hits.push(number);
        }
    }
    Ok(hits)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutState {
    Measure,
    AwaitEditor,
    AwaitSave,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutCheckpoint {
    pub revision: u64,
    pub state: LayoutState,
    pub baseline_sha256: String,
    pub iteration: u32,
    pub max_iterations: u32,
    pub operation_id: Option<String>,
    pub expected_old: Option<String>,
    pub export_request_id: Option<String>,
    pub diagnosis: Option<String>,
}

impl LayoutCheckpoint {
    pub fn start(baseline_sha256: String, max_iterations: u32) -> Self {
        Self {
            revision: 1,
            state: LayoutState::Measure,
            baseline_sha256,
            iteration: 0,
            max_iterations,
            operation_id: None,
            expected_old: None,
            export_request_id: None,
            diagnosis: None,
        }
    }

    fn bump(mut self, state: LayoutState, diagnosis: Option<String>) -> Self {
        self.revision += 1;
        self.state = state;
        self.diagnosis = diagnosis;
        self
    }

    /// No-model measure step. Unavailable locate cannot become ready when pages need updates.
    pub fn measure(
        self,
        locate: &dyn PageLocatePort,
        docx: &[u8],
        pdf: &[u8],
        anchors: &[Anchor],
        pages_need_update: bool,
    ) -> Self {
        if self.state != LayoutState::Measure {
            return self.bump(
                LayoutState::Failed,
                Some("measure called out of state".into()),
            );
        }
        if self.iteration >= self.max_iterations {
            return self.bump(LayoutState::Failed, Some("iteration limit".into()));
        }
        if !pages_need_update {
            return self.bump(LayoutState::Ready, None);
        }
        match locate.locate(docx, pdf, anchors) {
            Ok(_) => self.bump(
                LayoutState::Failed,
                Some("locate succeeded but point-edit port is not proven; cannot open unattended edit".into()),
            ),
            Err(LocateError::Unavailable { reason }) => self.bump(
                LayoutState::Failed,
                Some(reason),
            ),
            Err(error) => self.bump(LayoutState::Failed, Some(format!("{error:?}"))),
        }
    }

    pub fn can_publish_triad(&self) -> bool {
        false
    }
}

pub async fn load_checkpoint(
    pool: &sqlx::PgPool,
    request_artifact_id: uuid::Uuid,
    frozen_input_sha256: &str,
) -> Result<Option<LayoutCheckpoint>, crate::agent_error::AgentError> {
    let value: Option<sqlx::types::Json<LayoutCheckpoint>> =
        sqlx::query_scalar("SELECT kb_bid_v2_layout_checkpoint_get($1,$2::kb_sha256)")
            .bind(request_artifact_id)
            .bind(frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(|error| crate::agent_error::AgentError::new("INTERNAL", error.to_string()))?;
    Ok(value.map(|row| row.0))
}

pub async fn save_checkpoint(
    pool: &sqlx::PgPool,
    request_artifact_id: uuid::Uuid,
    frozen_input_sha256: &str,
    attempt: i32,
    token: uuid::Uuid,
    state: &LayoutCheckpoint,
) -> Result<(), crate::agent_error::AgentError> {
    sqlx::query("SELECT kb_bid_v2_layout_checkpoint_put($1,$2::kb_sha256,$3,$4,$5)")
        .bind(request_artifact_id)
        .bind(frozen_input_sha256)
        .bind(attempt)
        .bind(token)
        .bind(sqlx::types::Json(state))
        .execute(pool)
        .await
        .map_err(|error| crate::agent_error::AgentError::new("INTERNAL", error.to_string()))?;
    Ok(())
}

pub const CHECKPOINT_SQL_KEYS: &[&str] = &[
    "revision",
    "state",
    "baseline_sha256",
    "iteration",
    "max_iterations",
    "operation_id",
    "expected_old",
    "export_request_id",
    "diagnosis",
];

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document, Object, dictionary};
    use sha2::{Digest, Sha256};

    fn pdf_with_repeated_text() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let content = b"BT /F1 12 Tf 10 10 Td (PROOF) Tj ET";
        let make_page = |doc: &mut Document, pages, font, content: &[u8]| {
            let stream = doc.add_object(lopdf::Stream::new(dictionary! {}, content.to_vec()));
            doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages,
                "MediaBox" => vec![Object::from(0), Object::from(0), Object::from(200), Object::from(200)],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
                "Contents" => stream
            })
        };
        let pages = doc.new_object_id();
        let p1 = make_page(&mut doc, pages, font, content);
        let p2 = make_page(&mut doc, pages, font, content);
        doc.objects.insert(
            pages,
            dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(p1), Object::Reference(p2)],
                "Count" => 2
            }
            .into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn unavailable_locate_port_is_not_acceptance() {
        let err = UnavailableLocate
            .locate(
                b"docx",
                b"pdf",
                &[Anchor {
                    id: "proof-1".into(),
                }],
            )
            .unwrap_err();
        assert!(matches!(err, LocateError::Unavailable { .. }));
    }

    #[test]
    fn duplicate_text_search_cannot_be_a_locate_port() {
        let pdf = pdf_with_repeated_text();
        let hits = pdf_text_search_pages(&pdf, "PROOF").unwrap();
        assert_ne!(
            hits.len(),
            1,
            "text search must not uniquely locate repeated wording, got {hits:?}"
        );
        if hits.len() >= 2 {
            assert!(matches!(
                LocateError::Ambiguous {
                    anchor_id: "proof-1".into(),
                    candidates: hits.len(),
                },
                LocateError::Ambiguous { .. }
            ));
        }
    }

    #[test]
    fn unavailable_point_edit_rejects_without_rewriting_document() {
        let baseline = hex::encode(Sha256::digest(b"docx"));
        let err = UnavailableEdit
            .edit(
                "session",
                &baseline,
                &Anchor { id: "toc-1".into() },
                "3",
                "12",
            )
            .unwrap_err();
        assert!(matches!(err, EditError::Unavailable { .. }));
    }

    #[test]
    fn measure_without_page_updates_can_ready_without_editor() {
        let ck = LayoutCheckpoint::start("a".repeat(64), 3).measure(
            &UnavailableLocate,
            b"docx",
            b"pdf",
            &[],
            false,
        );
        assert_eq!(ck.state, LayoutState::Ready);
        assert!(!ck.can_publish_triad());
    }

    #[test]
    fn measure_with_updates_cannot_ready_when_locate_is_unavailable() {
        let ck = LayoutCheckpoint::start("a".repeat(64), 3).measure(
            &UnavailableLocate,
            b"docx",
            b"pdf",
            &[Anchor {
                id: "proof-1".into(),
            }],
            true,
        );
        assert_eq!(ck.state, LayoutState::Failed);
        assert!(ck.diagnosis.as_ref().unwrap().contains("not proven"));
        assert!(!ck.can_publish_triad());
    }

    #[test]
    fn layout_checkpoint_object_keys_match_sql_exact_list() {
        let value = serde_json::to_value(LayoutCheckpoint::start("a".repeat(64), 3)).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        let mut expected: Vec<_> = CHECKPOINT_SQL_KEYS
            .iter()
            .map(|k| (*k).to_string())
            .collect();
        expected.sort();
        assert_eq!(keys, expected);
        assert_eq!(value["state"], "measure");
        assert!(value["export_request_id"].is_null());
    }
}
