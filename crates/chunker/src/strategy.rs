//! Adaptive entry: resolve chain, validate, fall through (brain `strategy.go`).

use crate::heading::split_by_headings;
use crate::heuristic::split_by_heuristics;
use crate::profiler::{DocProfile, StrategyTier, profile_document, select_strategy};
use crate::splitter::{DEFAULT_CHUNK_OVERLAP, DEFAULT_CHUNK_SIZE, split_text};
use crate::tokens::{LANG_MIXED, chars_for_token_limit};
use crate::validator::validate_chunks;
use crate::{SplitterConfig, TextChunk};

pub const STRATEGY_HEADING: &str = "heading";
pub const STRATEGY_HEURISTIC: &str = "heuristic";
pub const STRATEGY_RECURSIVE: &str = "recursive";
pub const STRATEGY_LEGACY: &str = "legacy";

pub fn split(text: &str, cfg: SplitterConfig) -> Vec<TextChunk> {
    if text.is_empty() {
        return Vec::new();
    }
    let cfg = ensure_defaults(cfg);
    let (chain, profile) = resolve_chain_with_profile(text, &cfg);
    let total = text.chars().count();
    let mut last_legacy = Vec::new();
    for (i, tier) in chain.iter().enumerate() {
        let out = run_tier(*tier, text, &cfg, profile.as_ref());
        if validate_chunks(&out, total, cfg.chunk_size) {
            return out;
        }
        if *tier == StrategyTier::Legacy && i + 1 == chain.len() {
            last_legacy = out;
        }
    }
    if !last_legacy.is_empty() {
        return last_legacy;
    }
    split_text(text, cfg.chunk_size, cfg.chunk_overlap, &cfg.seps())
}

pub fn resolve_chain(strategy: &str, text: &str) -> Vec<&'static str> {
    let cfg = SplitterConfig {
        strategy: strategy.to_string(),
        ..SplitterConfig::default()
    };
    let (chain, _) = resolve_chain_with_profile(text, &cfg);
    chain.into_iter().map(StrategyTier::as_str).collect()
}

pub fn resolve_chain_with_profile(
    text: &str,
    cfg: &SplitterConfig,
) -> (Vec<StrategyTier>, Option<DocProfile>) {
    match cfg.strategy.as_str() {
        STRATEGY_HEADING => (vec![StrategyTier::Heading, StrategyTier::Legacy], None),
        STRATEGY_HEURISTIC => (vec![StrategyTier::Heuristic, StrategyTier::Legacy], None),
        STRATEGY_RECURSIVE | STRATEGY_LEGACY | "" => (vec![StrategyTier::Legacy], None),
        _ => {
            let profile = profile_document(text);
            let chain = select_strategy(&profile);
            (chain, Some(profile))
        }
    }
}

fn run_tier(
    tier: StrategyTier,
    text: &str,
    cfg: &SplitterConfig,
    profile: Option<&DocProfile>,
) -> Vec<TextChunk> {
    match tier {
        StrategyTier::Heading => split_by_headings(text, cfg, profile),
        StrategyTier::Heuristic => split_by_heuristics(text, cfg, profile),
        StrategyTier::Legacy => split_text(text, cfg.chunk_size, cfg.chunk_overlap, &cfg.seps()),
    }
}

pub fn ensure_defaults(mut cfg: SplitterConfig) -> SplitterConfig {
    if cfg.chunk_size == 0 {
        cfg.chunk_size = DEFAULT_CHUNK_SIZE;
    }
    if cfg.chunk_overlap == 0 {
        cfg.chunk_overlap = DEFAULT_CHUNK_OVERLAP;
    }
    if cfg.separators.is_empty() {
        cfg.separators = vec!["\n\n".into(), "\n".into(), "。".into()];
    }
    if cfg.token_limit > 0 {
        let lang = cfg
            .languages
            .first()
            .map(|s| s.as_str())
            .unwrap_or(LANG_MIXED);
        let budget = chars_for_token_limit(cfg.token_limit, lang);
        if budget > 0 && (cfg.chunk_size == 0 || budget < cfg.chunk_size) {
            cfg.chunk_size = budget;
        }
    }
    if cfg.chunk_size > 0 && cfg.chunk_overlap > cfg.chunk_size / 2 {
        cfg.chunk_overlap = cfg.chunk_size / 2;
    }
    cfg
}

pub fn merge_breadcrumbs(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        return child.to_string();
    }
    if child.is_empty() {
        return parent.to_string();
    }
    let parent_lines: Vec<&str> = parent.split('\n').collect();
    let mut child_lines: Vec<&str> = child.split('\n').collect();
    if let (Some(pl), Some(cl)) = (parent_lines.last(), child_lines.first())
        && pl.trim() == cl.trim()
    {
        child_lines.remove(0);
    }
    if child_lines.is_empty() {
        return parent.to_string();
    }
    format!("{parent}\n{}", child_lines.join("\n"))
}

pub struct ChildChunk {
    pub chunk: TextChunk,
    pub parent_index: Option<usize>,
}

pub struct ParentChildResult {
    pub parents: Vec<TextChunk>,
    pub children: Vec<ChildChunk>,
}

pub fn split_parent_child(
    text: &str,
    parent_cfg: SplitterConfig,
    child_cfg: SplitterConfig,
) -> ParentChildResult {
    if text.is_empty() {
        return ParentChildResult {
            parents: Vec::new(),
            children: Vec::new(),
        };
    }
    let parents = split(text, parent_cfg);
    if parents.is_empty() {
        return ParentChildResult {
            parents: Vec::new(),
            children: Vec::new(),
        };
    }
    let mut new_parents = Vec::new();
    let mut children = Vec::new();
    let mut child_seq = 0usize;
    for parent in parents {
        let subs = split(&parent.content, child_cfg.clone());
        let mut parent_index = None;
        if subs.len() > 1 || (subs.len() == 1 && subs[0].content != parent.content) {
            parent_index = Some(new_parents.len());
            new_parents.push(parent.clone());
        }
        for mut sub in subs {
            sub.seq = child_seq;
            sub.start += parent.start;
            sub.end += parent.start;
            sub.context_header = merge_breadcrumbs(&parent.context_header, &sub.context_header);
            children.push(ChildChunk {
                chunk: sub,
                parent_index,
            });
            child_seq += 1;
        }
    }
    ParentChildResult {
        parents: new_parents,
        children,
    }
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
    fn merge_breadcrumb_cases() {
        assert_eq!(merge_breadcrumbs("", "## Sub"), "## Sub");
        assert_eq!(merge_breadcrumbs("# Top", ""), "# Top");
        assert_eq!(merge_breadcrumbs("# Top", "## Other"), "# Top\n## Other");
        assert_eq!(
            merge_breadcrumbs("# Top\n## A", "## A\n### A1"),
            "# Top\n## A\n### A1"
        );
        assert_eq!(merge_breadcrumbs("# Top", "# Top"), "# Top");
    }

    #[test]
    fn ensure_defaults_fill_and_cap_overlap() {
        let cfg = ensure_defaults(SplitterConfig::default());
        assert_eq!(cfg.chunk_size, DEFAULT_CHUNK_SIZE);
        assert_eq!(cfg.chunk_overlap, DEFAULT_CHUNK_OVERLAP);
        assert!(!cfg.separators.is_empty());
        let capped = ensure_defaults(SplitterConfig {
            chunk_size: 100,
            chunk_overlap: 90,
            ..SplitterConfig::default()
        });
        assert_eq!(capped.chunk_overlap, 50);
    }

    #[test]
    fn heading_strategy_keeps_top_level() {
        let doc = "# Intro\nshort intro.\n\n# Usage\nshort usage.\n\n# FAQ\nshort faq.";
        let chunks = split(
            doc,
            SplitterConfig {
                chunk_size: 500,
                chunk_overlap: 0,
                strategy: STRATEGY_HEADING.into(),
                ..SplitterConfig::default()
            },
        );
        assert_eq!(chunks.len(), 3);
        for (i, h) in ["# Intro", "# Usage", "# FAQ"].iter().enumerate() {
            assert!(chunks[i].content.contains(h), "{}", chunks[i].content);
        }
    }

    #[test]
    fn position_invariant_across_tiers() {
        let cases = [
            (
                "heading",
                "# Top\nintro paragraph here.\n\n## Section A\nbody A here.\n\n## Section B\nbody B here.\n\n## Section C\nbody C.",
            ),
            (
                "heuristic",
                &format!(
                    "Kapitel 1: Einleitung\n{} \n\nKapitel 2: Hauptteil\n{}",
                    "Beispieltext. ".repeat(50),
                    "Mehr Text. ".repeat(50)
                ),
            ),
            ("legacy", &"plain prose without structure. ".repeat(100)),
        ];
        for (name, doc) in cases {
            let chunks = split(
                doc,
                SplitterConfig {
                    chunk_size: 300,
                    chunk_overlap: 30,
                    separators: vec!["\n\n".into(), "\n".into(), "。".into(), ". ".into()],
                    strategy: "auto".into(),
                    ..SplitterConfig::default()
                },
            );
            assert!(!chunks.is_empty(), "{name}");
            let runes: Vec<char> = doc.chars().collect();
            for (i, c) in chunks.iter().enumerate() {
                assert_eq!(
                    c.end - c.start,
                    c.content.chars().count(),
                    "{name} chunk {i}"
                );
                let sliced: String = runes[c.start..c.end].iter().collect();
                assert_eq!(sliced, c.content, "{name} chunk {i}");
            }
        }
    }
}
