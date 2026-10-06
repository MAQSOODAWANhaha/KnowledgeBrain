//! Tender-to-submission domain.
//!
//! Phase 1 parses the tender and publishes outline chapters plus template
//! content: `tender_upload`, `tender_process`, `tender_analysis`,
//! `docx_template`, `template_grid`, `docx_composition`.
//! Phase 2 matches knowledge-base evidence and publishes response data:
//! `content_generate`, `content_runtime`, `content_block`.
//! [`phase1`] publishes the outline and template artifact. [`phase2`] matches
//! knowledge-base evidence onto that artifact and publishes response data.

pub mod bid_authoring_contract;
pub use bid_authoring_contract::*;
pub mod agent_error;
pub mod agent_runtime;
pub mod authoring_runtime;
pub mod bid_authoring_v2;
pub mod content_block;
pub mod content_generate;
pub mod content_runtime;
pub mod docx_composition;
pub mod docx_layout;
pub mod docx_round;
pub mod docx_template;
pub mod export_review;
pub mod mutation;
pub mod onlyoffice_conversion;
pub mod phase1;
pub mod phase2;
pub mod quote_snapshot;
pub mod render_v2;
pub mod submission_export;
pub mod template_grid;
pub mod tender_analysis;
pub mod tender_process;
pub mod tender_upload;
pub mod workspace;

pub use mutation::{MutationContext, RequestIdentity};
