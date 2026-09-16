//! Clean-slate tender-to-submission V2 domain.

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
pub mod export_review;
pub mod docx_template;
pub mod mutation;
pub mod onlyoffice_conversion;
pub mod quote_snapshot;
pub mod render_v2;
pub mod submission_export;
pub mod template_grid;
pub mod tender_analysis;
pub mod tender_process;
pub mod tender_upload;
pub mod workspace;

pub use mutation::{MutationContext, RequestIdentity};
