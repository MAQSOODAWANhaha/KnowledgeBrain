//! Inactive Target V2 bidding job contracts.
//!
//! Phase 0 freezes payload, identity, and Oxana-owned transport policy only.
//! Oxana's `Job::name()` is an associated static function, so one tagged enum
//! cannot expose five task names through one `Job` implementation. Phase 7
//! therefore constructs five thin Job/Worker adapters after the active cutover.
//! Nothing in this module is registered or dispatched by the V1 API/worker.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const BID_AUTHORING_V2_PAYLOAD_SCHEMA: &str = "bid-authoring/v2";
/// Registry envelope version. The `/v2` suffix identifies the business schema major.
pub const BID_AUTHORING_V2_PAYLOAD_VERSION: u16 = 1;
pub const BID_AUTHORING_V2_QUEUE: &str = "bid-authoring-v2";
pub const BID_AUTHORING_V2_CONCURRENCY: usize = 4;
pub const BID_AUTHORING_V2_MAX_RETRIES: u32 = 3;
pub const BID_AUTHORING_V2_RETRY_BACKOFF_SECONDS: [u64; 3] = [10, 30, 90];
pub const BID_AUTHORING_V2_UNIQUE_CONFLICT_POLICY: &str = "skip";
pub const BID_AUTHORING_V2_RESURRECT_ON_REPLAY: bool = true;

/// Inactive Phase 0 transport policy consumed by the five Phase 7 adapters.
pub struct BidAuthoringV2OxanaPolicy;

impl BidAuthoringV2OxanaPolicy {
    pub const fn retry_delay_seconds(retries: u32) -> u64 {
        match retries {
            0 => BID_AUTHORING_V2_RETRY_BACKOFF_SECONDS[0],
            1 => BID_AUTHORING_V2_RETRY_BACKOFF_SECONDS[1],
            _ => BID_AUTHORING_V2_RETRY_BACKOFF_SECONDS[2],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BidAuthoringJobKindV2 {
    TenderDocumentProcess,
    RequirementSetCompile,
    OutlineGenerate,
    ContentGenerate,
    SubmissionExport,
}

impl BidAuthoringJobKindV2 {
    pub const ALL: [Self; 5] = [
        Self::TenderDocumentProcess,
        Self::RequirementSetCompile,
        Self::OutlineGenerate,
        Self::ContentGenerate,
        Self::SubmissionExport,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TenderDocumentProcess => "tender_document_process",
            Self::RequirementSetCompile => "requirement_set_compile",
            Self::OutlineGenerate => "outline_generate",
            Self::ContentGenerate => "content_generate",
            Self::SubmissionExport => "submission_export",
        }
    }
}

impl std::str::FromStr for BidAuthoringJobKindV2 {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| format!("unknown bidding authoring v2 job kind {value}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentGenerateOperationV2 {
    MatchOnly,
    Generate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionOutputModeV2 {
    ReviewDraft,
    Submission,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BidAuthoringRequestIdentityV2 {
    pub request_artifact_id: Uuid,
    pub request_revision: i64,
    pub frozen_input_sha256: String,
}

impl BidAuthoringRequestIdentityV2 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_revision < 1 {
            return Err("request revision must be positive");
        }
        if !is_sha256(&self.frozen_input_sha256) {
            return Err("frozen input sha256 must be lowercase hexadecimal");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "job_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BidAuthoringJobPayloadV2 {
    TenderDocumentProcess {
        request: BidAuthoringRequestIdentityV2,
        project_id: Uuid,
        document_revision_id: Uuid,
    },
    RequirementSetCompile {
        request: BidAuthoringRequestIdentityV2,
        project_id: Uuid,
        document_set_revision_id: Uuid,
        disposition_set_revision_id: Uuid,
    },
    OutlineGenerate {
        request: BidAuthoringRequestIdentityV2,
        project_id: Uuid,
        workspace_id: Uuid,
        base_workspace_revision_id: Uuid,
    },
    ContentGenerate {
        request: BidAuthoringRequestIdentityV2,
        project_id: Uuid,
        workspace_id: Uuid,
        base_workspace_revision_id: Uuid,
        operation: ContentGenerateOperationV2,
    },
    SubmissionExport {
        request: BidAuthoringRequestIdentityV2,
        project_id: Uuid,
        workspace_id: Uuid,
        workspace_revision_id: Uuid,
        output_mode: SubmissionOutputModeV2,
    },
}

pub const BID_TENDER_DOCUMENT_PROCESS_V2_TASK: &str = "bid:tender_document_process:v2";
pub const BID_REQUIREMENT_SET_COMPILE_V2_TASK: &str = "bid:requirement_set_compile:v2";
pub const BID_OUTLINE_GENERATE_V2_TASK: &str = "bid:outline_generate:v2";
pub const BID_CONTENT_GENERATE_V2_TASK: &str = "bid:content_generate:v2";
pub const BID_SUBMISSION_EXPORT_V2_TASK: &str = "bid:submission_export:v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenderDocumentProcessJobV2 {
    pub request: BidAuthoringRequestIdentityV2,
    pub project_id: Uuid,
    pub document_revision_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementSetCompileJobV2 {
    pub request: BidAuthoringRequestIdentityV2,
    pub project_id: Uuid,
    pub document_set_revision_id: Uuid,
    pub disposition_set_revision_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineGenerateJobV2 {
    pub request: BidAuthoringRequestIdentityV2,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub base_workspace_revision_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentGenerateJobV2 {
    pub request: BidAuthoringRequestIdentityV2,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub base_workspace_revision_id: Uuid,
    pub operation: ContentGenerateOperationV2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionExportJobV2 {
    pub request: BidAuthoringRequestIdentityV2,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub workspace_revision_id: Uuid,
    pub output_mode: SubmissionOutputModeV2,
}

macro_rules! request_scoped_job {
    ($job:ty, $name:expr) => {
        impl oxana::Job for $job {
            fn name() -> &'static str {
                $name
            }

            fn unique_id(&self) -> Option<String> {
                Some(format!(
                    "{}:{}:{}",
                    $name,
                    self.request.request_artifact_id.hyphenated(),
                    self.request.request_revision
                ))
            }

            fn on_conflict(&self) -> oxana::JobConflictStrategy {
                oxana::JobConflictStrategy::Skip
            }

            fn should_resurrect() -> bool {
                BID_AUTHORING_V2_RESURRECT_ON_REPLAY
            }
        }
    };
}

request_scoped_job!(
    TenderDocumentProcessJobV2,
    BID_TENDER_DOCUMENT_PROCESS_V2_TASK
);
request_scoped_job!(OutlineGenerateJobV2, BID_OUTLINE_GENERATE_V2_TASK);
request_scoped_job!(ContentGenerateJobV2, BID_CONTENT_GENERATE_V2_TASK);
request_scoped_job!(SubmissionExportJobV2, BID_SUBMISSION_EXPORT_V2_TASK);

impl oxana::Job for RequirementSetCompileJobV2 {
    fn name() -> &'static str {
        BID_REQUIREMENT_SET_COMPILE_V2_TASK
    }

    fn unique_id(&self) -> Option<String> {
        Some(format!(
            "requirement_set_compile:{}:{}:{}",
            self.project_id.hyphenated(),
            self.document_set_revision_id.hyphenated(),
            self.disposition_set_revision_id.hyphenated()
        ))
    }

    fn on_conflict(&self) -> oxana::JobConflictStrategy {
        oxana::JobConflictStrategy::Skip
    }

    fn should_resurrect() -> bool {
        BID_AUTHORING_V2_RESURRECT_ON_REPLAY
    }
}

#[derive(oxana::Queue)]
#[oxana(key = "bid-authoring-v2", concurrency = Dynamic(4))]
pub struct BidAuthoringV2Queue;

impl BidAuthoringJobPayloadV2 {
    pub const fn kind(&self) -> BidAuthoringJobKindV2 {
        match self {
            Self::TenderDocumentProcess { .. } => BidAuthoringJobKindV2::TenderDocumentProcess,
            Self::RequirementSetCompile { .. } => BidAuthoringJobKindV2::RequirementSetCompile,
            Self::OutlineGenerate { .. } => BidAuthoringJobKindV2::OutlineGenerate,
            Self::ContentGenerate { .. } => BidAuthoringJobKindV2::ContentGenerate,
            Self::SubmissionExport { .. } => BidAuthoringJobKindV2::SubmissionExport,
        }
    }

    pub fn request(&self) -> &BidAuthoringRequestIdentityV2 {
        match self {
            Self::TenderDocumentProcess { request, .. }
            | Self::RequirementSetCompile { request, .. }
            | Self::OutlineGenerate { request, .. }
            | Self::ContentGenerate { request, .. }
            | Self::SubmissionExport { request, .. } => request,
        }
    }

    /// Stable Oxana uniqueness material for the Phase 7 adapter. The inactive
    /// Phase 0 contract deliberately does not implement `oxana::Job`.
    pub fn unique_material(&self) -> String {
        match self {
            Self::RequirementSetCompile {
                project_id,
                document_set_revision_id,
                disposition_set_revision_id,
                ..
            } => format!(
                "requirement_set_compile:{}:{}:{}",
                project_id.hyphenated(),
                document_set_revision_id.hyphenated(),
                disposition_set_revision_id.hyphenated()
            ),
            other => {
                let request = other.request();
                format!(
                    "{}:{}:{}",
                    other.kind().as_str(),
                    request.request_artifact_id.hyphenated(),
                    request.request_revision
                )
            }
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        self.request().validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BidAuthoringErrorCodeV2 {
    InputSchemaInvalid,
    FrozenInputMissing,
    FrozenInputDigestMismatch,
    WorkspaceCasConflict,
    AgentOutputInvalid,
    EvidenceUnavailable,
    AssetMissing,
    AssetDigestMismatch,
    AttachmentPreparationFailed,
    RenderSchemaInvalid,
    RendererFailed,
    ObjectCommitFailed,
}

impl BidAuthoringErrorCodeV2 {
    pub const ALL: [Self; 12] = [
        Self::InputSchemaInvalid,
        Self::FrozenInputMissing,
        Self::FrozenInputDigestMismatch,
        Self::WorkspaceCasConflict,
        Self::AgentOutputInvalid,
        Self::EvidenceUnavailable,
        Self::AssetMissing,
        Self::AssetDigestMismatch,
        Self::AttachmentPreparationFailed,
        Self::RenderSchemaInvalid,
        Self::RendererFailed,
        Self::ObjectCommitFailed,
    ];
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> BidAuthoringRequestIdentityV2 {
        BidAuthoringRequestIdentityV2 {
            request_artifact_id: Uuid::from_u128(1),
            request_revision: 7,
            frozen_input_sha256: "a".repeat(64),
        }
    }

    fn round_trip(payload: BidAuthoringJobPayloadV2, expected: serde_json::Value) {
        assert_eq!(serde_json::to_value(&payload).unwrap(), expected);
        let encoded = serde_json::to_vec(&payload).unwrap();
        assert_eq!(
            serde_json::from_slice::<BidAuthoringJobPayloadV2>(&encoded).unwrap(),
            payload
        );
        assert_eq!(payload.validate(), Ok(()));
    }

    #[test]
    fn exactly_five_closed_job_kinds_round_trip() {
        assert_eq!(BidAuthoringJobKindV2::ALL.len(), 5);
        for kind in BidAuthoringJobKindV2::ALL {
            assert_eq!(kind.as_str().parse::<BidAuthoringJobKindV2>(), Ok(kind));
        }
        assert!("evidence_match".parse::<BidAuthoringJobKindV2>().is_err());
    }

    #[test]
    fn every_payload_operation_and_output_mode_has_golden_json() {
        let request_json = serde_json::json!({
            "request_artifact_id":"00000000-0000-0000-0000-000000000001",
            "request_revision":7,
            "frozen_input_sha256":"a".repeat(64)
        });
        let ids = |kind: &str| serde_json::json!({"job_kind":kind,"request":request_json.clone()});
        let mut expected = ids("tender_document_process");
        expected.as_object_mut().unwrap().extend(serde_json::json!({"project_id":"00000000-0000-0000-0000-000000000002","document_revision_id":"00000000-0000-0000-0000-000000000003"}).as_object().unwrap().clone());
        round_trip(
            BidAuthoringJobPayloadV2::TenderDocumentProcess {
                request: request(),
                project_id: Uuid::from_u128(2),
                document_revision_id: Uuid::from_u128(3),
            },
            expected,
        );
        let mut expected = ids("requirement_set_compile");
        expected.as_object_mut().unwrap().extend(serde_json::json!({"project_id":"00000000-0000-0000-0000-000000000002","document_set_revision_id":"00000000-0000-0000-0000-000000000003","disposition_set_revision_id":"00000000-0000-0000-0000-000000000004"}).as_object().unwrap().clone());
        round_trip(
            BidAuthoringJobPayloadV2::RequirementSetCompile {
                request: request(),
                project_id: Uuid::from_u128(2),
                document_set_revision_id: Uuid::from_u128(3),
                disposition_set_revision_id: Uuid::from_u128(4),
            },
            expected,
        );
        let mut expected = ids("outline_generate");
        expected.as_object_mut().unwrap().extend(serde_json::json!({"project_id":"00000000-0000-0000-0000-000000000002","workspace_id":"00000000-0000-0000-0000-000000000003","base_workspace_revision_id":"00000000-0000-0000-0000-000000000004"}).as_object().unwrap().clone());
        round_trip(
            BidAuthoringJobPayloadV2::OutlineGenerate {
                request: request(),
                project_id: Uuid::from_u128(2),
                workspace_id: Uuid::from_u128(3),
                base_workspace_revision_id: Uuid::from_u128(4),
            },
            expected,
        );
        for operation in [
            ContentGenerateOperationV2::MatchOnly,
            ContentGenerateOperationV2::Generate,
        ] {
            let mut expected = ids("content_generate");
            expected.as_object_mut().unwrap().extend(serde_json::json!({"project_id":"00000000-0000-0000-0000-000000000002","workspace_id":"00000000-0000-0000-0000-000000000003","base_workspace_revision_id":"00000000-0000-0000-0000-000000000004","operation": if operation==ContentGenerateOperationV2::MatchOnly {"match_only"} else {"generate"}}).as_object().unwrap().clone());
            round_trip(
                BidAuthoringJobPayloadV2::ContentGenerate {
                    request: request(),
                    project_id: Uuid::from_u128(2),
                    workspace_id: Uuid::from_u128(3),
                    base_workspace_revision_id: Uuid::from_u128(4),
                    operation,
                },
                expected,
            );
        }
        for output_mode in [
            SubmissionOutputModeV2::ReviewDraft,
            SubmissionOutputModeV2::Submission,
        ] {
            let mode = match output_mode {
                SubmissionOutputModeV2::ReviewDraft => "review_draft",
                SubmissionOutputModeV2::Submission => "submission",
            };
            let mut expected = ids("submission_export");
            expected.as_object_mut().unwrap().extend(serde_json::json!({"project_id":"00000000-0000-0000-0000-000000000002","workspace_id":"00000000-0000-0000-0000-000000000003","workspace_revision_id":"00000000-0000-0000-0000-000000000004","output_mode":mode}).as_object().unwrap().clone());
            round_trip(
                BidAuthoringJobPayloadV2::SubmissionExport {
                    request: request(),
                    project_id: Uuid::from_u128(2),
                    workspace_id: Uuid::from_u128(3),
                    workspace_revision_id: Uuid::from_u128(4),
                    output_mode,
                },
                expected,
            );
        }
        let mut preview_export = ids("submission_export");
        preview_export.as_object_mut().unwrap().extend(serde_json::json!({"project_id":"00000000-0000-0000-0000-000000000002","workspace_id":"00000000-0000-0000-0000-000000000003","workspace_revision_id":"00000000-0000-0000-0000-000000000004","output_mode":"preview"}).as_object().unwrap().clone());
        assert!(serde_json::from_value::<BidAuthoringJobPayloadV2>(preview_export).is_err());
    }

    #[test]
    fn uniqueness_error_codes_and_inactive_oxana_policy_are_closed() {
        let payload = BidAuthoringJobPayloadV2::RequirementSetCompile {
            request: request(),
            project_id: Uuid::from_u128(2),
            document_set_revision_id: Uuid::from_u128(3),
            disposition_set_revision_id: Uuid::from_u128(4),
        };
        assert_eq!(
            payload.unique_material(),
            "requirement_set_compile:00000000-0000-0000-0000-000000000002:00000000-0000-0000-0000-000000000003:00000000-0000-0000-0000-000000000004"
        );
        let expected = [
            "INPUT_SCHEMA_INVALID",
            "FROZEN_INPUT_MISSING",
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "WORKSPACE_CAS_CONFLICT",
            "AGENT_OUTPUT_INVALID",
            "EVIDENCE_UNAVAILABLE",
            "ASSET_MISSING",
            "ASSET_DIGEST_MISMATCH",
            "ATTACHMENT_PREPARATION_FAILED",
            "RENDER_SCHEMA_INVALID",
            "RENDERER_FAILED",
            "OBJECT_COMMIT_FAILED",
        ];
        assert_eq!(BidAuthoringErrorCodeV2::ALL.len(), expected.len());
        for (code, literal) in BidAuthoringErrorCodeV2::ALL.into_iter().zip(expected) {
            let json = format!("\"{literal}\"");
            assert_eq!(serde_json::to_string(&code).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<BidAuthoringErrorCodeV2>(&json).unwrap(),
                code
            );
        }
        assert_eq!(BID_AUTHORING_V2_PAYLOAD_VERSION, 1);
        assert_eq!(BID_AUTHORING_V2_CONCURRENCY, 4);
        assert_eq!(BID_AUTHORING_V2_MAX_RETRIES, 3);
        assert_eq!(BID_AUTHORING_V2_UNIQUE_CONFLICT_POLICY, "skip");
        const {
            assert!(BID_AUTHORING_V2_RESURRECT_ON_REPLAY);
        }
        assert_eq!(
            (0..3)
                .map(BidAuthoringV2OxanaPolicy::retry_delay_seconds)
                .collect::<Vec<_>>(),
            vec![10, 30, 90]
        );
    }
}
