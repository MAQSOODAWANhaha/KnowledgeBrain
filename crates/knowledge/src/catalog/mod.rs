//! Knowledge-asset SQL. Split from persist.rs; names and behavior unchanged.

mod chunks;
mod document;
mod error;
mod models;
mod product;
mod schema;
mod spans;
mod tags;
mod version;
mod workspace;

pub use chunks::*;
pub use document::*;
pub use error::*;
pub use models::*;
pub use product::*;
pub use schema::*;
pub use spans::*;
pub use tags::*;
pub use version::*;
pub use workspace::*;

pub use crate::graph::sql::{
    PgGraphHit, delete_graph_for_document, graph_hits_pg, persist_graph_maps,
};
pub use crate::identity::*;
pub use crate::search::hybrid::{PgSearchHit, hybrid_search_pg, resolve_pg_assembly_targets};
pub use crate::wiki::sql::{
    list_wiki_folders, persist_wiki_changes_atomic, replace_wiki_page_chunks, upsert_wiki_folder,
    upsert_wiki_page,
};
