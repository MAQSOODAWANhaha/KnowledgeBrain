//! Brain-faithful adaptive chunker: auto → heading / heuristic → legacy.

mod header_tracker;
mod heading;
mod heading_hierarchy;
mod heuristic;
mod patterns;
mod profiler;
mod splitter;
mod strategy;
mod tokens;
mod validator;

pub use splitter::{DEFAULT_CHUNK_OVERLAP, DEFAULT_CHUNK_SIZE, HARD_CAP};
pub use strategy::{ParentChildResult, resolve_chain, split as split_raw, split_parent_child};

/// Brain `buildParentChildConfigs` defaults.
pub const PARENT_CHUNK_SIZE: usize = 4096;
pub const CHILD_CHUNK_SIZE: usize = 384;

use crate::Chunk;
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextChunk {
    pub content: String,
    pub context_header: String,
    pub seq: usize,
    pub start: usize,
    pub end: usize,
}

impl TextChunk {
    pub fn embedding_content(&self) -> String {
        let body = self.content.trim();
        if self.context_header.is_empty() {
            body.to_string()
        } else {
            format!("{}\n\n{body}", self.context_header)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SplitterConfig {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub separators: Vec<String>,
    pub strategy: String,
    pub token_limit: usize,
    pub languages: Vec<String>,
}

impl SplitterConfig {
    pub(crate) fn seps(&self) -> Vec<&str> {
        self.separators.iter().map(|s| s.as_str()).collect()
    }
}

/// New ProductVersion default: `strategy=auto`.
pub fn split(
    markdown: &str,
    version_id: Uuid,
    document_id: Uuid,
    size: usize,
    overlap: usize,
) -> Vec<Chunk> {
    split_with(markdown, version_id, document_id, size, overlap, "auto")
}

pub fn split_with(
    markdown: &str,
    version_id: Uuid,
    document_id: Uuid,
    size: usize,
    overlap: usize,
    strategy: &str,
) -> Vec<Chunk> {
    let cfg = SplitterConfig {
        chunk_size: size,
        chunk_overlap: overlap,
        strategy: strategy.to_string(),
        ..SplitterConfig::default()
    };
    let raw = strategy::split(markdown, cfg);
    to_domain(raw, version_id, document_id)
}

/// Brain `buildParentChildConfigs`: parent overlap = base overlap; child overlap = child_size/5.
pub fn parent_child_configs(
    base: &SplitterConfig,
    parent_size: usize,
    child_size: usize,
) -> (SplitterConfig, SplitterConfig) {
    let parent_size = if parent_size == 0 {
        PARENT_CHUNK_SIZE
    } else {
        parent_size
    };
    let child_size = if child_size == 0 {
        CHILD_CHUNK_SIZE
    } else {
        child_size
    };
    let parent = SplitterConfig {
        chunk_size: parent_size,
        chunk_overlap: base.chunk_overlap,
        separators: base.separators.clone(),
        strategy: base.strategy.clone(),
        token_limit: 0,
        languages: base.languages.clone(),
    };
    let child = SplitterConfig {
        chunk_size: child_size,
        chunk_overlap: child_size / 5,
        separators: base.separators.clone(),
        strategy: base.strategy.clone(),
        token_limit: 0,
        languages: base.languages.clone(),
    };
    (parent, child)
}

/// Parent 4096 / overlap=base overlap; child 384 / overlap=child_size/5 when `parent_child`.
pub fn split_configured(
    markdown: &str,
    version_id: Uuid,
    document_id: Uuid,
    size: usize,
    overlap: usize,
    strategy: &str,
    parent_child: bool,
) -> Vec<Chunk> {
    split_from_config(
        markdown,
        version_id,
        document_id,
        SplitterConfig {
            chunk_size: size,
            chunk_overlap: overlap,
            strategy: strategy.to_string(),
            ..SplitterConfig::default()
        },
        parent_child,
        0,
        0,
    )
}

pub fn split_from_config(
    markdown: &str,
    version_id: Uuid,
    document_id: Uuid,
    cfg: SplitterConfig,
    parent_child: bool,
    parent_size: usize,
    child_size: usize,
) -> Vec<Chunk> {
    if !parent_child {
        return to_domain(strategy::split(markdown, cfg), version_id, document_id);
    }
    let (parent_cfg, child_cfg) = parent_child_configs(&cfg, parent_size, child_size);
    let result = split_parent_child(markdown, parent_cfg, child_cfg);
    let mut parent_ids = Vec::new();
    let mut out = Vec::new();
    for p in result.parents {
        let id = Uuid::new_v4();
        parent_ids.push(id);
        out.push(Chunk {
            id,
            document_id,
            product_version_id: version_id,
            chunk_type: "parent_text".into(),
            content: p.content,
            context_header: p.context_header,
            start_at: p.start as i32,
            end_at: p.end as i32,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
            source_locator: None,
        });
    }
    for c in result.children {
        let parent_chunk_id = c.parent_index.and_then(|i| parent_ids.get(i).copied());
        out.push(Chunk {
            id: Uuid::new_v4(),
            document_id,
            product_version_id: version_id,
            chunk_type: "text".into(),
            content: c.chunk.content,
            context_header: c.chunk.context_header,
            start_at: c.chunk.start as i32,
            end_at: c.chunk.end as i32,
            parent_chunk_id,
            generated_questions: Vec::new(),
            source_locator: None,
        });
    }
    out
}

/// Attach authoritative rendered-span intersections. Missing mappings retain
/// native units as explicitly unresolved metadata instead of guessed offsets.
pub fn annotate_source_locators(
    chunks: &mut [Chunk],
    markdown: &str,
    units: &[docparser::StructuredSourceUnit],
    contract: Option<&docparser::SourceContract>,
) {
    let mut char_bytes: Vec<_> = markdown.char_indices().map(|(i, _)| i).collect();
    char_bytes.push(markdown.len());
    for (chunk_index, chunk) in chunks.iter_mut().enumerate() {
        let start = char_bytes
            .get(chunk.start_at.max(0) as usize)
            .copied()
            .unwrap_or(markdown.len());
        let end = char_bytes
            .get(chunk.end_at.max(0) as usize)
            .copied()
            .unwrap_or(markdown.len());
        let mut hits = Vec::new();
        for unit in units {
            let identity = contract.and_then(|c| c.unit(&unit.key));
            let mapped = identity.is_some_and(|u| !u.rendered_spans.is_empty());
            let intersects = identity.is_some_and(|u| {
                u.rendered_spans
                    .iter()
                    .any(|span| span.start_byte < end && start < span.end_byte)
            });
            if intersects || (!mapped && chunk_index == 0) {
                hits.push(serde_json::json!({"key":unit.key,"unit_id":unit.key,"ordinal":unit.ordinal,"kind":unit.kind,
                    "locator":unit.locator,"grid":unit.grid,"identity":identity,
                    "resolution":if intersects {"resolved"} else {"unresolved"}}));
            }
        }
        if !hits.is_empty() {
            chunk.source_locator = Some(serde_json::Value::Array(hits));
        }
    }
}

fn to_domain(raw: Vec<TextChunk>, version_id: Uuid, document_id: Uuid) -> Vec<Chunk> {
    raw.into_iter()
        .map(|c| Chunk {
            id: Uuid::new_v4(),
            document_id,
            product_version_id: version_id,
            chunk_type: "text".into(),
            content: c.content,
            context_header: c.context_header,
            start_at: c.start as i32,
            end_at: c.end as i32,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
            source_locator: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_strategy_is_legacy_only() {
        assert_eq!(resolve_chain("", "x"), vec!["legacy"]);
        assert_eq!(resolve_chain("legacy", "x"), vec!["legacy"]);
        assert_eq!(resolve_chain("recursive", "x"), vec!["legacy"]);
        assert_eq!(resolve_chain("heading", "x")[0], "heading");
    }

    #[test]
    fn auto_with_headings_includes_heading_tier() {
        let md = "# A\n\nbody\n\n# B\n\nmore\n\n# C\n\nstill more text here\n";
        let chain = resolve_chain("auto", md);
        assert_eq!(chain[0], "heading");
        assert_eq!(*chain.last().unwrap(), "legacy");
    }

    #[test]
    fn rune_invariant_and_splits() {
        let md = "hello\n\nworld。again";
        let chunks = split(md, Uuid::new_v4(), Uuid::new_v4(), 8, 1);
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert_eq!(c.end_at - c.start_at, c.content.chars().count() as i32);
            assert_eq!(c.chunk_type, "text");
        }
    }

    #[test]
    fn hard_cap_slices_oversized_piece() {
        let md: String = std::iter::repeat_n('字', 8000).collect();
        let chunks = split_with(&md, Uuid::new_v4(), Uuid::new_v4(), 512, 80, "legacy");
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert!(c.content.chars().count() <= HARD_CAP);
            assert_eq!(c.end_at - c.start_at, c.content.chars().count() as i32);
        }
    }

    #[test]
    fn parent_child_emits_parent_text_and_links() {
        let body = "Lorem ipsum dolor sit amet consectetur adipiscing elit. ".repeat(80);
        let chunks = split_configured(
            &body,
            Uuid::new_v4(),
            Uuid::new_v4(),
            512,
            80,
            "legacy",
            true,
        );
        let parents: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == "parent_text")
            .collect();
        let children: Vec<_> = chunks.iter().filter(|c| c.chunk_type == "text").collect();
        assert!(!children.is_empty());
        if !parents.is_empty() {
            assert!(children.iter().any(|c| c.parent_chunk_id.is_some()));
        }
        for c in &chunks {
            assert_eq!(c.end_at - c.start_at, c.content.chars().count() as i32);
        }
    }

    #[test]
    fn parent_child_configs_match_brain() {
        let base = SplitterConfig {
            chunk_size: 512,
            chunk_overlap: 80,
            strategy: "auto".into(),
            separators: vec!["\n\n".into(), "\n".into(), "。".into()],
            ..SplitterConfig::default()
        };
        let (p, c) = parent_child_configs(&base, 0, 0);
        assert_eq!(p.chunk_size, 4096);
        assert_eq!(p.chunk_overlap, 80);
        assert_eq!(p.strategy, "auto");
        assert_eq!(p.separators, base.separators);
        assert_eq!(c.chunk_size, 384);
        assert_eq!(c.chunk_overlap, 384 / 5);
        assert_eq!(c.strategy, "auto");
        let (p2, c2) = parent_child_configs(&base, 2000, 200);
        assert_eq!(p2.chunk_size, 2000);
        assert_eq!(p2.chunk_overlap, 80);
        assert_eq!(c2.chunk_size, 200);
        assert_eq!(c2.chunk_overlap, 40);
    }

    #[test]
    fn heading_breadcrumb_in_header_heading_line_in_content() {
        let body = "Lorem ipsum dolor sit amet consectetur adipiscing elit. ".repeat(4);
        let md = format!(
            "# Top\n{body}\n\n## Section A\n{body}\n\n## Section B\nBravo body plus {body}"
        );
        let chunks = split_with(&md, Uuid::new_v4(), Uuid::new_v4(), 300, 0, "heading");
        assert!(!chunks.is_empty());
        let b = chunks
            .iter()
            .find(|c| c.content.contains("Bravo"))
            .expect("section B");
        assert!(b.context_header.contains("# Top"), "{}", b.context_header);
        assert!(
            b.context_header.contains("## Section B"),
            "{}",
            b.context_header
        );
        assert!(!b.context_header.contains("## Section A"));
        assert!(!b.content.contains("# Top"));
        assert!(b.content.contains("## Section B"));
        assert!(!b.content.contains(&b.context_header));
    }
    #[test]
    fn annotate_source_locators_attaches_table_grid_and_page() {
        use docparser::{
            StructuredSourceLocator, StructuredSourceUnit, StructuredSourceUnitKind, TableGrid,
        };
        let md = "# Report\n\nIntro text here.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let units = vec![
            StructuredSourceUnit {
                key: "u0".into(),
                ordinal: 0,
                kind: StructuredSourceUnitKind::Section,
                text: "Intro text here.".into(),
                locator: StructuredSourceLocator::Page {
                    page_ordinal: 1,
                    left: None,
                    top: None,
                    right: None,
                    bottom: None,
                },
                grid: None,
            },
            StructuredSourceUnit {
                key: "u1".into(),
                ordinal: 1,
                kind: StructuredSourceUnitKind::TableRegion,
                text: String::new(),
                locator: StructuredSourceLocator::PageTable {
                    page_ordinal: 1,
                    table_ordinal: 0,
                    left: 10.0,
                    top: 20.0,
                    right: 100.0,
                    bottom: 60.0,
                },
                grid: Some(TableGrid {
                    row_count: 2,
                    column_count: 2,
                    cells: vec![],
                    widths_mm: None,
                }),
            },
        ];
        let mut chunks = split(md, Uuid::new_v4(), Uuid::new_v4(), 512, 0);
        assert!(!chunks.is_empty());
        annotate_source_locators(&mut chunks, md, &units, None);
        // section chunk carries the Page locator
        let sec = chunks
            .iter()
            .find(|c| c.content.contains("Intro text here."))
            .expect("section chunk");
        let loc = sec.source_locator.as_ref().expect("section locator");
        assert_eq!(loc[0]["locator"]["locator_kind"], "page");
        assert_eq!(loc[0]["locator"]["page_ordinal"], 1);
        // table chunk carries the grid with row_count (find the table hit by kind)
        let tbl = chunks
            .iter()
            .find(|c| c.content.contains("| a | b |"))
            .expect("table chunk");
        let tloc = tbl.source_locator.as_ref().expect("table locator");
        let thit = tloc
            .as_array()
            .expect("locator array")
            .iter()
            .find(|h| h["kind"] == "table_region")
            .expect("table_region hit");
        assert_eq!(thit["grid"]["row_count"], 2);
        assert_eq!(thit["resolution"], "unresolved");
        assert_eq!(thit["locator"]["locator_kind"], "page_table");
        let contract = docparser::SourceContract {
            schema_version: 2,
            document_revision: "a".repeat(64),
            parser_version: "fixture".into(),
            markdown_sha256: platform::sha256_hex(md.as_bytes()),
            page_manifest: vec![],
            glyph_normalizations: vec![],
            units: units
                .iter()
                .map(|unit| docparser::SourceUnitIdentity {
                    unit_id: unit.key.clone(),
                    ordinal: unit.ordinal,
                    kind: unit.kind.clone(),
                    text_sha256: platform::sha256_hex(unit.text.as_bytes()),
                    grid_sha256: unit.grid.as_ref().map(docparser::table_grid_digest),
                    section_id: None,
                    parent_section_id: None,
                    heading_level: None,
                    heading_path: String::new(),
                    physical_locator: Some(unit.locator.clone()),
                    physical_path: None,
                    physical_locator_unavailable_reason: None,
                    rendered_spans: vec![docparser::RenderedSpan {
                        start_byte: if unit.ordinal == 0 { 10 } else { 27 },
                        end_byte: if unit.ordinal == 0 { 26 } else { md.len() },
                    }],
                    completeness: docparser::SourceCompleteness::Complete,
                    reasons: vec![],
                    table_id: unit.grid.as_ref().map(|_| "table-1".into()),
                    header_cells: vec![],
                })
                .collect(),
        };
        let mut mapped = split(md, Uuid::new_v4(), Uuid::new_v4(), 512, 0);
        annotate_source_locators(&mut mapped, md, &units, Some(&contract));
        let native_grid = mapped
            .iter()
            .flat_map(|c| {
                c.source_locator
                    .as_ref()
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
            })
            .find(|u| u["unit_id"] == "u1")
            .unwrap();
        assert_eq!(native_grid["resolution"], "resolved");
        assert_eq!(native_grid["grid"]["row_count"], 2);
        // unknown units leave chunks untouched
        let mut chunks2 = split(md, Uuid::new_v4(), Uuid::new_v4(), 512, 0);
        annotate_source_locators(&mut chunks2, md, &[], None);
        assert!(chunks2.iter().all(|c| c.source_locator.is_none()));
    }
}
