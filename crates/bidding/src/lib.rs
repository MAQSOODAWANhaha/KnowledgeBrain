//! Tender outline and the response written against it.
//!
//! [`outline`] parses the tender and publishes chapters plus template content.
//! [`response`] matches knowledge-base evidence onto that outline.

pub mod agent_error;
pub mod agent_runtime;
pub mod authoring_runtime;
pub mod content_block;
pub mod outline;
pub mod response;
pub mod template_grid;
pub mod tender_analysis;
