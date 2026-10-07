//! Three packages, three outputs.
//!
//! [`analysis`] reads the frozen tender into reviewed records, relations, and repairs.
//! [`outline`] publishes the chapter tree and prescribed template slots.
//! [`response`] fills those response slots from the knowledge base.
//!
//! Analysis does not publish chapters or write knowledge responses.
//! Outline does not review records or match the knowledge base.
//! Response does not parse the tender or change chapters.

pub mod agent_error;
pub mod agent_runtime;
pub mod analysis;
pub mod authoring_runtime;
pub mod content_block;
pub mod outline;
pub mod response;
pub mod template_grid;
