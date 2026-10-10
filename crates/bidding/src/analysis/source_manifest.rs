//! The frozen input's sole document/member and relation contracts.
use docparser::SourceContract;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentRole {
    Primary,
    Supplement,
    PrescribedForm,
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentAvailability {
    Available,
    Missing,
    Failed,
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenDocument {
    pub document_id: String,
    pub document_revision: String,
    pub parser_contract_version: String,
    pub page_count: usize,
    pub role: DocumentRole,
    pub availability: DocumentAvailability,
    pub source_contract: Option<SourceContract>,
    pub parse_coverage: Option<crate::outline::frozen::ParseCoverage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationEndpoint {
    pub document_id: String,
    /// Native parser unit key; None means the whole document, never a guessed cell.
    pub unit_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentRelationKind {
    ExplicitReference,
    InferredCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentRelationStatus {
    Confirmed,
    Unconfirmed,
    Missing,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentRelation {
    pub id: String,
    pub from: RelationEndpoint,
    pub to: Option<RelationEndpoint>,
    pub kind: DocumentRelationKind,
    pub status: DocumentRelationStatus,
    pub required: bool,
    /// Exact original reference text, not a model assertion that a relation exists.
    pub basis: String,
    /// Exact source locator supporting the reference; retained without interpretation.
    pub locator: Value,
}

impl super::FrozenInput {
    pub fn validate_document_relations(&self) -> Result<(), String> {
        let mut document_ids = std::collections::BTreeSet::new();
        for document in &self.documents {
            if document.document_id.trim().is_empty() || !document_ids.insert(&document.document_id)
            {
                return Err("frozen document identity missing or duplicated".into());
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        let endpoint_valid = |endpoint: &RelationEndpoint| {
            self.documents.iter().any(|document| {
                document.document_id == endpoint.document_id
                    && document.availability == DocumentAvailability::Available
                    && endpoint.unit_id.as_ref().is_none_or(|key| {
                        document
                            .source_contract
                            .as_ref()
                            .is_some_and(|contract| contract.unit(key).is_some())
                    })
            })
        };
        for relation in &self.document_relations {
            if relation.id.trim().is_empty()
                || !ids.insert(&relation.id)
                || relation.basis.trim().is_empty()
                || !endpoint_valid(&relation.from)
            {
                return Err("document relation identity, basis or source endpoint invalid".into());
            }
            if relation.status == DocumentRelationStatus::Confirmed
                && (relation.kind != DocumentRelationKind::ExplicitReference
                    || !relation.to.as_ref().is_some_and(endpoint_valid))
            {
                return Err(
                    "confirmed relation requires explicit reference and available target".into(),
                );
            }
            if relation.status == DocumentRelationStatus::Confirmed {
                let grounded = self.source_units.iter().any(|source| {
                    source.document_id == relation.from.document_id
                        && relation
                            .from
                            .unit_id
                            .as_ref()
                            .is_none_or(|key| source.locator["unit_id"] == *key)
                        && relation.locator.as_object().is_some_and(|locator| {
                            !locator.is_empty()
                                && locator.iter().all(|(key, value)| {
                                    source.locator.get(key) == Some(value)
                                        || source.locator["physical_locator"].get(key)
                                            == Some(value)
                                })
                        })
                        && (source.text.contains(&relation.basis)
                            || self.structured_forms.iter().any(|form| {
                                form["source_unit_revision_id"] == source.source_unit_revision_id
                                    && form["definition"]["cells"].as_array().is_some_and(|cells| {
                                        cells.iter().any(|cell| {
                                            cell["text"]
                                                .as_str()
                                                .is_some_and(|text| text.contains(&relation.basis))
                                        })
                                    })
                            }))
                });
                if !grounded {
                    return Err(
                        "confirmed relation basis or locator is not present in its original source"
                            .into(),
                    );
                }
            }
            if relation.status != DocumentRelationStatus::Missing
                && relation
                    .to
                    .as_ref()
                    .is_some_and(|endpoint| !endpoint_valid(endpoint))
            {
                return Err("document relation target endpoint invalid".into());
            }
        }
        Ok(())
    }

    pub fn required_relations_ready(&self) -> bool {
        self.validate_document_relations().is_ok()
            && self.document_relations.iter().all(|relation| {
                !relation.required || relation.status == DocumentRelationStatus::Confirmed
            })
    }

    pub fn related_documents(&self, left: &str, right: &str) -> bool {
        left == right
            || self.document_relations.iter().any(|relation| {
                relation.kind == DocumentRelationKind::ExplicitReference
                    && relation.status == DocumentRelationStatus::Confirmed
                    && relation.to.as_ref().is_some_and(|target| {
                        (relation.from.document_id == left && target.document_id == right)
                            || (relation.from.document_id == right && target.document_id == left)
                    })
            })
    }

    pub fn metadata_collection(&self, kind: &str) -> Result<Vec<Value>, String> {
        match kind {
            "documents" => self
                .documents
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string()),
            "document_relations" => self
                .document_relations
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string()),
            "decisions" => Ok(self.decisions.clone()),
            _ => Err("unknown collection".into()),
        }
    }
}

#[cfg(test)]
impl FrozenDocument {
    pub(crate) fn fixture(id: &str) -> Self {
        Self {
            document_id: id.into(),
            document_revision: String::new(),
            parser_contract_version: "source-v2".into(),
            page_count: 0,
            role: DocumentRole::Unspecified,
            availability: DocumentAvailability::Available,
            source_contract: None,
            parse_coverage: None,
        }
    }
}
