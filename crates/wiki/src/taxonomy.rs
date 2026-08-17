//! Candidate slugs + source citations. LLM JSON when available, else graph/heuristic.

use domain::{Chunk, GraphNode, Store};
use serde::Deserialize;
use uuid::Uuid;

pub const PAGE_SUMMARY: &str = "summary";
pub const PAGE_ENTITY: &str = "entity";
pub const PAGE_CONCEPT: &str = "concept";
pub const PAGE_INDEX: &str = "index";
pub const PAGE_LOG: &str = "log";
pub const PAGE_SYNTHESIS: &str = "synthesis";
pub const PAGE_COMPARISON: &str = "comparison";

pub const ALL_PAGE_TYPES: [&str; 7] = [
    PAGE_SUMMARY,
    PAGE_ENTITY,
    PAGE_CONCEPT,
    PAGE_INDEX,
    PAGE_LOG,
    PAGE_SYNTHESIS,
    PAGE_COMPARISON,
];

#[derive(Debug, Clone, Deserialize)]
pub struct ExtractedItem {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "type")]
    pub page_type: String,
}

#[derive(Debug, Default, Deserialize)]
struct CombinedExtraction {
    #[serde(default)]
    entities: Vec<ExtractedItem>,
    #[serde(default)]
    concepts: Vec<ExtractedItem>,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub title: String,
    pub slug: String,
    pub page_type: String,
    pub aliases: Vec<String>,
    pub about: String,
    pub source_refs: Vec<Uuid>,
}

pub fn typed_slug(page_type: &str, title: &str) -> String {
    let base: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "page" } else { base };
    format!("{page_type}/{base}")
}

pub fn parse_extraction(raw: &str) -> Vec<ExtractedItem> {
    let trimmed = raw.trim();
    let json = extract_json_object(trimmed).unwrap_or(trimmed);
    let parsed: CombinedExtraction = serde_json::from_str(json).unwrap_or_default();
    let mut out = Vec::new();
    for mut e in parsed.entities {
        if e.name.is_empty() {
            continue;
        }
        if e.page_type.is_empty() {
            e.page_type = PAGE_ENTITY.into();
        }
        if e.slug.is_empty() {
            e.slug = typed_slug(&e.page_type, &e.name);
        }
        out.push(e);
    }
    for mut c in parsed.concepts {
        if c.name.is_empty() {
            continue;
        }
        if c.page_type.is_empty() {
            c.page_type = PAGE_CONCEPT.into();
        }
        if c.slug.is_empty() {
            c.slug = typed_slug(&c.page_type, &c.name);
        }
        out.push(c);
    }
    out
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(&raw[start..=end])
}

pub fn candidates_from_graph(nodes: &[&GraphNode], chunks: &[Chunk]) -> Vec<Candidate> {
    nodes
        .iter()
        .filter(|n| !n.name.trim().is_empty())
        .map(|n| {
            let slug = typed_slug(PAGE_ENTITY, &n.name);
            let refs = cite_chunks(chunks, &n.name, &[]);
            Candidate {
                title: n.name.clone(),
                slug,
                page_type: PAGE_ENTITY.into(),
                aliases: Vec::new(),
                about: n.name.clone(),
                source_refs: refs,
            }
        })
        .collect()
}

pub fn cite_chunks(chunks: &[Chunk], name: &str, aliases: &[String]) -> Vec<Uuid> {
    let mut needles = vec![name.to_string()];
    needles.extend(aliases.iter().cloned());
    let mut ids = Vec::new();
    for ch in chunks {
        if ch.chunk_type != "text" {
            continue;
        }
        let lower = ch.content.to_ascii_lowercase();
        if needles
            .iter()
            .any(|n| !n.is_empty() && lower.contains(&n.to_ascii_lowercase()))
        {
            ids.push(ch.id);
        }
    }
    ids
}

pub fn collect_text_chunks(store: &Store, document_id: Uuid) -> Vec<Chunk> {
    let mut chunks: Vec<_> = store
        .chunks
        .values()
        .filter(|c| c.document_id == document_id && c.chunk_type == "text")
        .cloned()
        .collect();
    chunks.sort_by_key(|c| c.start_at);
    chunks
}

pub fn assemble_body(chunks: &[Chunk]) -> String {
    let mut body: String = chunks
        .iter()
        .map(|c| c.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if body.chars().count() > crate::ASSEMBLE_RUNE_CAP {
        body = body.chars().take(crate::ASSEMBLE_RUNE_CAP).collect();
    }
    body
}

pub const CATEGORY_MAX_DEPTH: usize = 3;

pub fn clean_category_path(parts: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for part in parts {
        for label in part.split(['/', '|']) {
            let label = label.trim();
            if label.is_empty() {
                continue;
            }
            if out.iter().any(|x: &String| x.eq_ignore_ascii_case(label)) {
                continue;
            }
            out.push(label.to_string());
            if out.len() >= CATEGORY_MAX_DEPTH {
                return out;
            }
        }
    }
    out
}

#[derive(Debug, Default, Deserialize)]
struct AssignmentFile {
    #[serde(default)]
    assignments: Vec<Assignment>,
}

#[derive(Debug, Deserialize)]
struct Assignment {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    path: Vec<String>,
}

pub fn parse_taxonomy_assignments(raw: &str) -> std::collections::HashMap<String, Vec<String>> {
    let json = extract_json_object(raw.trim()).unwrap_or(raw.trim());
    let parsed: AssignmentFile = serde_json::from_str(json).unwrap_or_default();
    let mut out = std::collections::HashMap::new();
    for a in parsed.assignments {
        let slug = a.slug.trim().to_string();
        if slug.is_empty() {
            continue;
        }
        out.insert(slug, clean_category_path(&a.path));
    }
    out
}

/// Brain `wikiTaxonomyFeedAllMaxFolders`.
pub const TAXONOMY_FEED_ALL: usize = 60;
/// Brain `wikiTaxonomyPromptMaxPaths`.
pub const TAXONOMY_PROMPT_MAX: usize = 150;
/// Brain `wikiTaxonomyRelevantTopK`.
pub const TAXONOMY_TOP_K: usize = 3;

pub fn existing_folder_paths(store: &Store, version_id: Uuid) -> Vec<Vec<String>> {
    store
        .wiki_folders
        .values()
        .filter(|f| f.product_version_id == version_id && !f.path.is_empty())
        .map(|f| {
            f.path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .collect()
}

pub fn cap_folders(paths: &[Vec<String>], max: usize) -> Vec<Vec<String>> {
    if max > 0 && paths.len() > max {
        paths[..max].to_vec()
    } else {
        paths.to_vec()
    }
}

/// Brain `selectRelevantFolders`: small trees go whole; large trees keep every
/// level-1 anchor and the deeper folders nearest each item (cosine).
pub fn select_relevant_folders(
    pool: &[Vec<String>],
    items: &[(String, String)],
) -> Vec<Vec<String>> {
    if pool.len() <= TAXONOMY_FEED_ALL {
        return cap_folders(pool, TAXONOMY_PROMPT_MAX);
    }
    let mut l1_seen = std::collections::BTreeSet::new();
    let mut l1 = Vec::new();
    let mut deeper = Vec::new();
    for p in pool {
        if p.is_empty() {
            continue;
        }
        if l1_seen.insert(p[0].clone()) {
            l1.push(vec![p[0].clone()]);
        }
        if p.len() >= 2 {
            deeper.push(p.clone());
        }
    }
    if deeper.is_empty() || items.is_empty() {
        return cap_folders(pool, TAXONOMY_PROMPT_MAX);
    }
    let folder_vecs: Vec<Vec<f32>> = deeper
        .iter()
        .map(|p| index::embed(&p.join(" / ")))
        .collect();
    let item_vecs: Vec<Vec<f32>> = items
        .iter()
        .map(|(title, about)| {
            let about: String = about.chars().take(120).collect();
            index::embed(&format!("{title} {about}"))
        })
        .collect();
    let mut selected = l1;
    selected.extend(select_folders_by_vectors(
        &deeper,
        &folder_vecs,
        &item_vecs,
        TAXONOMY_TOP_K,
    ));
    cap_folders(&selected, TAXONOMY_PROMPT_MAX)
}

fn select_folders_by_vectors(
    deeper: &[Vec<String>],
    folder_vecs: &[Vec<f32>],
    item_vecs: &[Vec<f32>],
    top_k: usize,
) -> Vec<Vec<String>> {
    if deeper.len() != folder_vecs.len() || item_vecs.is_empty() || top_k == 0 {
        return Vec::new();
    }
    let mut chosen = vec![false; deeper.len()];
    for iv in item_vecs {
        let mut ranking: Vec<(usize, f64)> = folder_vecs
            .iter()
            .enumerate()
            .map(|(i, fv)| (i, index::cosine(iv, fv)))
            .collect();
        ranking.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (i, _) in ranking.into_iter().take(top_k) {
            chosen[i] = true;
        }
    }
    deeper
        .iter()
        .enumerate()
        .filter(|(i, _)| chosen[*i])
        .map(|(_, p)| p.clone())
        .collect()
}

pub fn nearest_folder(title: &str, about: &str, candidates: &[Vec<String>]) -> Option<Vec<String>> {
    let deeper: Vec<Vec<String>> = candidates
        .iter()
        .filter(|p| p.len() >= 2)
        .cloned()
        .collect();
    if deeper.is_empty() {
        return None;
    }
    let about: String = about.chars().take(120).collect();
    let iv = index::embed(&format!("{title} {about}"));
    deeper.into_iter().max_by(|a, b| {
        let sa = index::cosine(&iv, &index::embed(&a.join(" / ")));
        let sb = index::cosine(&iv, &index::embed(&b.join(" / ")));
        sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
    })
}

pub fn fallback_path(page_type: &str, title: &str, existing: &[Vec<String>]) -> Vec<String> {
    if let Some(near) = nearest_folder(title, title, existing) {
        return near;
    }
    let want = match page_type {
        PAGE_ENTITY => "Entities",
        PAGE_CONCEPT => "Concepts",
        PAGE_SUMMARY => "Summaries",
        PAGE_SYNTHESIS => "Synthesis",
        PAGE_COMPARISON => "Comparisons",
        _ => "",
    };
    if !want.is_empty()
        && let Some(hit) = existing
            .iter()
            .find(|p| p.first().is_some_and(|s| s.eq_ignore_ascii_case(want)))
    {
        return hit.clone();
    }
    category_for(page_type, title)
}

pub fn category_for(page_type: &str, title: &str) -> Vec<String> {
    match page_type {
        PAGE_ENTITY => vec!["Entities".into(), title.chars().take(1).collect()],
        PAGE_CONCEPT => vec!["Concepts".into()],
        PAGE_SUMMARY => vec!["Summaries".into()],
        PAGE_SYNTHESIS => vec!["Synthesis".into()],
        PAGE_COMPARISON => vec!["Comparisons".into()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_types_match_spec() {
        assert!(ALL_PAGE_TYPES.contains(&"summary"));
        assert!(ALL_PAGE_TYPES.contains(&"entity"));
        assert!(ALL_PAGE_TYPES.contains(&"concept"));
        assert!(ALL_PAGE_TYPES.contains(&"index"));
        assert!(ALL_PAGE_TYPES.contains(&"log"));
        assert!(ALL_PAGE_TYPES.contains(&"synthesis"));
        assert!(ALL_PAGE_TYPES.contains(&"comparison"));
    }

    #[test]
    fn parse_entities_and_concepts() {
        let raw = r#"{"entities":[{"name":"Alpha Switch","slug":"entity/alpha-switch"}],"concepts":[{"name":"Throughput"}]}"#;
        let items = parse_extraction(raw);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].page_type, "entity");
        assert_eq!(items[1].page_type, "concept");
        assert_eq!(items[1].slug, "concept/throughput");
    }

    #[test]
    fn cite_matches_alias() {
        let ch = Chunk {
            id: Uuid::new_v4(),
            document_id: Uuid::new_v4(),
            product_version_id: Uuid::new_v4(),
            chunk_type: "text".into(),
            content: "ISO9001 factory audit".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 20,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let ids = cite_chunks(std::slice::from_ref(&ch), "ISO 9001", &["ISO9001".into()]);
        assert_eq!(ids, vec![ch.id]);
    }

    #[test]
    fn parse_assignments_and_clean_path() {
        let raw = r#"{"assignments":[{"slug":"entity/alpha","path":["Entities","Hardware","Entities"]}]}"#;
        let map = parse_taxonomy_assignments(raw);
        assert_eq!(
            map.get("entity/alpha").unwrap(),
            &vec!["Entities".to_string(), "Hardware".to_string()]
        );
        assert_eq!(
            clean_category_path(&["Entities/Switches".into(), "".into()]),
            vec!["Entities".to_string(), "Switches".to_string()]
        );
    }

    #[test]
    fn fallback_reuses_existing_bucket() {
        let existing = vec![vec!["Entities".into(), "Network".into()]];
        assert_eq!(
            fallback_path(PAGE_ENTITY, "Alpha", &existing),
            vec!["Entities".to_string(), "Network".to_string()]
        );
    }

    #[test]
    fn small_folder_pool_is_fed_whole() {
        let pool = vec![
            vec!["Entities".into()],
            vec!["Entities".into(), "Network".into()],
        ];
        let selected = select_relevant_folders(&pool, &[("Switch".into(), String::new())]);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn large_pool_keeps_l1_and_nearest_deeper() {
        let mut pool = vec![vec!["Entities".into()], vec!["Concepts".into()]];
        for i in 0..TAXONOMY_FEED_ALL {
            pool.push(vec!["Entities".into(), format!("Folder{i}")]);
        }
        pool.push(vec!["Entities".into(), "Network Switch".into()]);
        let selected = select_relevant_folders(
            &pool,
            &[("Alpha Switch".into(), "campus switch throughput".into())],
        );
        assert!(selected.iter().any(|p| p == &vec!["Entities".to_string()]));
        assert!(
            selected
                .iter()
                .any(|p| p.last().is_some_and(|s| s.contains("Switch"))),
            "{selected:?}"
        );
        assert!(selected.len() <= TAXONOMY_PROMPT_MAX);
        assert!(selected.len() < pool.len());
    }
}
