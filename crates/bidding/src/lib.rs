//! Clean-slate tender-to-submission V2 domain.

pub mod bid_authoring_contract;
pub use bid_authoring_contract::*;
pub mod bid_authoring_v2;
pub mod content_block;
pub mod mutation;
pub mod outline_agent;
mod outline_validation;
pub mod quote_snapshot;
pub mod render_v2;
mod requirement_compile;
pub mod tender_process;
pub mod tender_upload;
pub mod workspace;

pub use mutation::{MutationContext, RequestIdentity};
