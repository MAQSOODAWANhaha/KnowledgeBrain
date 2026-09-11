//! Canonical bidding queue contract.
//!
//! The platform crate owns queue payload serialization and Oxana uniqueness.
//! Bidding reexports that single implementation to prevent contract drift.

pub use platform::{
    BID_AUTHORING_V2_CONCURRENCY, BID_AUTHORING_V2_MAX_RETRIES,
    BID_AUTHORING_V2_RESURRECT_ON_REPLAY, BID_CONTENT_GENERATE_V2_TASK,
    BID_REQUIREMENT_SET_COMPILE_V2_TASK, BID_SUBMISSION_EXPORT_V2_TASK,
    BID_TENDER_DOCUMENT_PROCESS_V2_TASK, BidAuthoringJobKindV2, BidAuthoringJobPayloadV2,
    BidAuthoringRequestIdentityV2, BidAuthoringV2Queue, ContentGenerateJobV2,
    ContentGenerateOperationV2, RequirementSetCompileJobV2, SubmissionExportJobV2,
    SubmissionOutputModeV2, TenderDocumentProcessJobV2,
};
