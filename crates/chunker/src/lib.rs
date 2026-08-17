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

use domain::Chunk;
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
        });
    }
    out
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
}
