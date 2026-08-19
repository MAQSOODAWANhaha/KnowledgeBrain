//! Candidate slugs + source citations. LLM JSON when available, else graph/heuristic.

use std::collections::HashMap;

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
    #[serde(default)]
    pub details: String,
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
    pub details: String,
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
        if !junk_item_name(&e.name) {
            out.push(e);
        }
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
        if !junk_item_name(&c.name) {
            out.push(c);
        }
    }
    out
}

/// Figure IDs, hashes, lone numbers — not wiki items.
pub fn junk_item_name(name: &str) -> bool {
    let t = name.trim();
    if t.is_empty() {
        return true;
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if t.len() >= 16 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("图") && t.chars().any(|c| c.is_ascii_digit()) && t.chars().count() <= 8 {
        return true;
    }
    matches!(
        lower.as_str(),
        "tcp" | "udp" | "http" | "https" | "ipv4" | "ipv6"
    )
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(&raw[start..=end])
}

pub fn candidates_from_graph(nodes: &[&GraphNode], _chunks: &[Chunk]) -> Vec<Candidate> {
    nodes
        .iter()
        .filter(|n| !n.name.trim().is_empty())
        .map(|n| {
            let slug = typed_slug(PAGE_ENTITY, &n.name);
            Candidate {
                title: n.name.clone(),
                slug,
                page_type: PAGE_ENTITY.into(),
                aliases: Vec::new(),
                about: n.name.clone(),
                details: String::new(),
                source_refs: Vec::new(),
            }
        })
        .collect()
}

pub fn cover_like_text(s: &str) -> bool {
    let head: String = s.chars().take(480).collect();
    (head.contains("版权声明") || head.contains("Copyright"))
        && (head.contains("完全公开") || head.contains("邮编") || head.contains("地址"))
}

pub fn cite_chunks(chunks: &[Chunk], name: &str, aliases: &[String]) -> Vec<Uuid> {
    let mut needles = vec![name.to_string()];
    needles.extend(aliases.iter().cloned());
    needles.retain(|n| !n.trim().is_empty());
    if needles.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(i32, bool, Uuid)> = Vec::new();
    for ch in chunks {
        if ch.chunk_type != "text" {
            continue;
        }
        let lower = ch.content.to_ascii_lowercase();
        let mut hits = 0i32;
        for n in &needles {
            let needle = n.to_ascii_lowercase();
            let mut from = 0;
            while let Some(at) = lower[from..].find(&needle) {
                hits += 1;
                from += at + needle.len();
                if from >= lower.len() {
                    break;
                }
            }
        }
        if hits == 0 {
            continue;
        }
        scored.push((hits, cover_like_text(&ch.content), ch.id));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let preferred: Vec<Uuid> = scored
        .iter()
        .filter(|(_, cover, _)| !*cover)
        .take(CITE_SNIPPETS_MAX)
        .map(|(_, _, id)| *id)
        .collect();
    if !preferred.is_empty() {
        return preferred;
    }
    scored
        .into_iter()
        .take(CITE_SNIPPETS_MAX)
        .map(|(_, _, id)| id)
        .collect()
}

/// Brain `maxRunesPerCitationBatch`.
pub const MAX_RUNES_PER_CITATION_BATCH: usize = 12000;
/// Brain `maxCitationBatchConcurrency`.
pub const MAX_CITATION_BATCH_CONCURRENCY: usize = 4;

#[derive(Debug, Default, Deserialize)]
struct CitationFile {
    #[serde(default)]
    citations: HashMap<String, Vec<String>>,
    #[serde(default)]
    new_slugs: Vec<NewSlugItem>,
}

#[derive(Debug, Default, Deserialize)]
struct NewSlugItem {
    #[serde(default, rename = "type")]
    page_type: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    details: String,
    #[serde(default)]
    source_chunks: Vec<String>,
}

pub fn split_citation_batches(chunks: &[Chunk]) -> Vec<Vec<&Chunk>> {
    let mut batches: Vec<Vec<&Chunk>> = Vec::new();
    let mut current: Vec<&Chunk> = Vec::new();
    let mut runes = 0usize;
    for c in chunks {
        if c.chunk_type != "text" || c.content.trim().is_empty() {
            continue;
        }
        let n = c.content.chars().count();
        if !current.is_empty() && runes + n > MAX_RUNES_PER_CITATION_BATCH {
            batches.push(std::mem::take(&mut current));
            runes = 0;
        }
        current.push(c);
        runes += n;
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

pub fn render_candidate_slugs(items: &[Candidate]) -> String {
    items
        .iter()
        .filter(|c| c.page_type == PAGE_ENTITY || c.page_type == PAGE_CONCEPT)
        .map(|c| {
            let aliases = if c.aliases.is_empty() {
                String::new()
            } else {
                format!(" aliases=\"{}\"", c.aliases.join(", "))
            };
            format!(
                "- slug: {}, type: {}, name: {:?}{aliases}, description: {}",
                c.slug, c.page_type, c.title, c.about
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_chunks_xml(batch: &[&Chunk]) -> (String, Vec<(String, Uuid)>) {
    let mut xml = String::new();
    let mut map = Vec::new();
    for (i, c) in batch.iter().enumerate() {
        let alias = format!("c{i:03}");
        map.push((alias.clone(), c.id));
        xml.push_str(&format!(
            "<c id=\"{alias}\" index=\"{i}\">\n{}\n</c>\n",
            c.content
        ));
    }
    (xml, map)
}

pub struct CitationParse {
    pub citations: HashMap<String, Vec<String>>,
    pub new_slugs: Vec<(ExtractedItem, Vec<String>)>,
}

pub fn parse_citation_json(raw: &str) -> CitationParse {
    let trimmed = raw.trim();
    let json = extract_json_object(trimmed).unwrap_or(trimmed);
    let parsed: CitationFile = serde_json::from_str(json).unwrap_or_default();
    let mut news = Vec::new();
    for n in parsed.new_slugs {
        if n.name.trim().is_empty() {
            continue;
        }
        let mut page_type = n.page_type;
        if page_type.is_empty() {
            page_type = if n.slug.starts_with("concept/") {
                PAGE_CONCEPT.into()
            } else {
                PAGE_ENTITY.into()
            };
        }
        let slug = if n.slug.contains('/') {
            n.slug
        } else if n.slug.is_empty() {
            typed_slug(&page_type, &n.name)
        } else {
            format!("{page_type}/{}", n.slug)
        };
        news.push((
            ExtractedItem {
                name: n.name,
                slug,
                aliases: n.aliases,
                description: n.description,
                details: n.details,
                page_type,
            },
            n.source_chunks,
        ));
    }
    CitationParse {
        citations: parsed.citations,
        new_slugs: news,
    }
}

fn resolve_aliases(aliases: &[String], alias_to_id: &[(String, Uuid)]) -> Vec<Uuid> {
    aliases
        .iter()
        .filter_map(|a| alias_to_id.iter().find(|(k, _)| k == a).map(|(_, id)| *id))
        .collect()
}

fn drop_cover_refs(refs: &mut Vec<Uuid>, chunks: &[Chunk]) {
    let kept: Vec<Uuid> = refs
        .iter()
        .copied()
        .filter(|id| {
            chunks
                .iter()
                .find(|c| c.id == *id)
                .is_none_or(|c| !cover_like_text(&c.content))
        })
        .collect();
    if !kept.is_empty() {
        *refs = kept;
    }
}

fn classify_one_batch(
    model: &str,
    slugs_xml: &str,
    batch: &[&Chunk],
    language: &str,
) -> (HashMap<String, Vec<Uuid>>, Vec<Candidate>) {
    let (chunks_xml, alias_map) = render_chunks_xml(batch);
    let user = format!(
        "<instructions>\nWrite any new_slugs names/descriptions/details in {language}. For each candidate slug, list chunk ids from <chunks> that substantively discuss it (a concrete fact, not a passing mention). Use the id attribute verbatim (c000). Omit candidates with no hits. A chunk may be cited by several slugs. If a significant item is missing from <candidate_slugs>, add it under new_slugs.\nOutput ONLY JSON: {{\"citations\":{{\"entity/x\":[\"c000\"]}},\"new_slugs\":[{{\"type\":\"entity\",\"name\":\"\",\"slug\":\"\",\"aliases\":[],\"description\":\"\",\"details\":\"\",\"source_chunks\":[\"c000\"]}}]}}\n</instructions>\n\n<candidate_slugs>\n{slugs_xml}\n</candidate_slugs>\n\n<chunks>\n{chunks_xml}\n</chunks>"
    );
    let raw = enrichment::chat_complete_wiki(
        &format!("You are a precise citation system. Output JSON only. Never invent chunk ids. Write new_slugs text in {language}."),
        &user,
        model,
    )
    .unwrap_or_default();
    let parsed = parse_citation_json(&raw);
    let mut citations: HashMap<String, Vec<Uuid>> = HashMap::new();
    let mut news: Vec<Candidate> = Vec::new();
    for (slug, aliases) in parsed.citations {
        citations.insert(slug, resolve_aliases(&aliases, &alias_map));
    }
    for (item, aliases) in parsed.new_slugs {
        if junk_item_name(&item.name) {
            continue;
        }
        let page_type = if ALL_PAGE_TYPES.contains(&item.page_type.as_str()) {
            item.page_type
        } else {
            PAGE_ENTITY.into()
        };
        news.push(Candidate {
            title: item.name,
            slug: item.slug,
            page_type,
            aliases: item.aliases,
            about: item.description,
            details: item.details,
            source_refs: resolve_aliases(&aliases, &alias_map),
        });
    }
    (citations, news)
}

/// Brain Pass 1..N: `classifyChunkCitations`. No HTTP → empty (caller uses substring).
pub fn cite_with_llm(
    model: &str,
    candidates: &[Candidate],
    chunks: &[Chunk],
    language: &str,
) -> (HashMap<String, Vec<Uuid>>, Vec<Candidate>) {
    if !enrichment::chat_http_configured() {
        return (HashMap::new(), Vec::new());
    }
    let slugs_xml = render_candidate_slugs(candidates);
    if slugs_xml.trim().is_empty() {
        return (HashMap::new(), Vec::new());
    }
    let batches = split_citation_batches(chunks);
    if batches.is_empty() {
        return (HashMap::new(), Vec::new());
    }
    type BatchOut = (HashMap<String, Vec<Uuid>>, Vec<Candidate>);
    let collected = std::sync::Mutex::new(Vec::<BatchOut>::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers = MAX_CITATION_BATCH_CONCURRENCY.min(batches.len()).max(1);
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if i >= batches.len() {
                        break;
                    }
                    let one = classify_one_batch(model, &slugs_xml, &batches[i], language);
                    collected.lock().expect("cite batch mutex").push(one);
                }
            });
        }
    });
    let mut citations: HashMap<String, Vec<Uuid>> = HashMap::new();
    let mut news: Vec<Candidate> = Vec::new();
    for (part, extra) in collected.into_inner().expect("cite batch mutex") {
        for (slug, ids) in part {
            citations.entry(slug).or_default().extend(ids);
        }
        news.extend(extra);
    }
    for ids in citations.values_mut() {
        ids.sort();
        ids.dedup();
    }
    (citations, news)
}

/// Brain `mergeCitationsIntoItems` + substring fallback for uncited slugs.
pub fn attach_citations(
    mut items: Vec<Candidate>,
    citations: HashMap<String, Vec<Uuid>>,
    news: Vec<Candidate>,
    chunks: &[Chunk],
) -> Vec<Candidate> {
    for it in &mut items {
        if let Some(ids) = citations.get(&it.slug) {
            it.source_refs = ids.clone();
        }
        if it.source_refs.is_empty() {
            it.source_refs = cite_chunks(chunks, &it.title, &it.aliases);
        }
        drop_cover_refs(&mut it.source_refs, chunks);
    }
    let existing: std::collections::HashSet<String> =
        items.iter().map(|c| c.slug.clone()).collect();
    for mut n in news {
        if existing.contains(&n.slug) {
            if let Some(it) = items.iter_mut().find(|c| c.slug == n.slug) {
                for r in n.source_refs {
                    if !it.source_refs.contains(&r) {
                        it.source_refs.push(r);
                    }
                }
            }
            continue;
        }
        if n.source_refs.is_empty() {
            n.source_refs = cite_chunks(chunks, &n.title, &n.aliases);
        }
        drop_cover_refs(&mut n.source_refs, chunks);
        items.push(n);
    }
    items
}

/// Brain `collectCitedChunkContent`: verbatim cited text, cover blocks dropped.
pub fn cited_verbatim(cited: &[&Chunk]) -> String {
    cited
        .iter()
        .filter(|c| !cover_like_text(&c.content))
        .map(|c| c.content.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn document_language(s: &str) -> &'static str {
    let total = s.chars().filter(|c| !c.is_whitespace()).count();
    if total == 0 {
        return "Chinese";
    }
    let cjk = s
        .chars()
        .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
        .count();
    if cjk * 5 >= total {
        "Chinese"
    } else {
        "English"
    }
}

pub fn candidate_slug_prompt(
    body: &str,
    previous: &str,
    title: &str,
    language: &str,
) -> (String, String) {
    let prev = if previous.trim().is_empty() {
        "(none — this is a new document)"
    } else {
        previous
    };
    let system = format!(
        "You are a knowledge extraction system. Return JSON only. This pass lists a lightweight candidate set; another pass attaches supporting chunks, so details are a short fallback only. Write ALL names, descriptions, and details in {language}."
    );
    let user = format!(
        "<document>\n<content>\n{body}\n</content>\n</document>\n\n<previous_slugs>\n{prev}\n</previous_slugs>\n\n# {title}\n\nReturn {{\"entities\":[],\"concepts\":[]}}. Each item: name, slug (entity/... or concept/...), aliases (same-thing only), description (one sentence, 15-40 words, WHAT this item is), details (1-3 sentences, under 300 characters, fallback only).\nReuse exact slugs from <previous_slugs> for the same item. Skip figure IDs, hashes, lone numbers, bare protocol names, and names only mentioned in passing. Write ALL names/descriptions/details in {language}. If there is no real text, return empty arrays."
    );
    (system, user)
}

pub fn wiki_summary_prompt(body: &str, listing: &str, language: &str) -> (String, String) {
    let system = format!(
        "You are a wiki editor. FIRST line MUST be: SUMMARY: {{one sentence, 15-40 words}}. Then structured Markdown. No preamble. Write the SUMMARY line and all Markdown in {language}."
    );
    let user = format!(
        "<document>\n<content>\n{body}\n</content>\n</document>\n\n<available_wiki_pages>\n{listing}\n</available_wiki_pages>\n\n<instructions>\n1. FIRST line: SUMMARY: {{one sentence, 15-40 words}}.\n2. Then a comprehensive Markdown summary of THIS document (key facts, arguments, conclusions).\n3. Use ## / ### only when the source has sections. End with ## Key Takeaways.\n4. Wiki-link only slugs listed above as [[slug|name]]. Do not invent slugs.\n5. Do not copy cover, address, copyright, or postal blocks.\n6. Write ALL output in {language}.\n7. If content has no real text: SUMMARY: No textual content was extractable from this document.\n</instructions>"
    );
    (system, user)
}

pub fn has_sufficient_text(body: &str) -> bool {
    let stripped = regex_lite_images(body);
    stripped.chars().filter(|c| !c.is_whitespace()).count() >= 10
}

fn regex_lite_images(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !s.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if s[i..].starts_with("![")
            && let Some(end) = s[i..].find(')')
        {
            i += end + 1;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn split_summary_line(raw: &str) -> (String, String) {
    let t = raw.trim();
    for prefix in ["SUMMARY:", "Summary:", "summary:"] {
        if let Some(rest) = t.strip_prefix(prefix) {
            let rest = rest.trim_start();
            if let Some((line, body)) = rest.split_once('\n') {
                return (line.trim().to_string(), body.trim().to_string());
            }
            return (rest.trim().to_string(), String::new());
        }
    }
    (String::new(), t.to_string())
}

pub fn first_lede(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    let body = if t.starts_with('#') {
        t.split_once('\n')
            .map(|(_, rest)| rest.trim())
            .unwrap_or("")
    } else {
        t
    };
    let para = body
        .split("\n\n")
        .find(|p| !p.trim().is_empty())
        .unwrap_or(body);
    clip_runes(para.trim(), 240)
}

pub fn norm_item_name(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '（' && *c != '）' && *c != '(' && *c != ')')
        .collect::<String>()
        .to_lowercase()
}

pub fn dedup_candidates(items: Vec<Candidate>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for it in items {
        if let Some(ex) = out
            .iter_mut()
            .find(|e| norm_item_name(&e.title) == norm_item_name(&it.title))
        {
            for a in it.aliases {
                if !ex.aliases.iter().any(|x| x == &a) {
                    ex.aliases.push(a);
                }
            }
            for r in it.source_refs {
                if !ex.source_refs.contains(&r) {
                    ex.source_refs.push(r);
                }
            }
            if ex.about.len() < it.about.len() {
                ex.about = it.about;
            }
            if ex.details.len() < it.details.len() {
                ex.details = it.details;
            }
        } else {
            out.push(it);
        }
    }
    out
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

/// Per-item page cap. Never dump the 32k assemble_body into a slug.
pub const ITEM_PAGE_RUNE_CAP: usize = 4000;
pub const SUMMARY_FALLBACK_RUNES: usize = 800;
const CITE_SNIPPETS_MAX: usize = 3;

fn clip_runes(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

/// Template fallback when Reduce LLM is off or failed. Never dump assemble_body.
pub fn page_content_for(
    page_type: &str,
    title: &str,
    about: &str,
    _cited: &[&Chunk],
    summary_llm: &str,
    full_body: &str,
) -> String {
    if page_type == PAGE_SUMMARY {
        let s = summary_llm.trim();
        if !s.is_empty() {
            return clip_runes(s, crate::ASSEMBLE_RUNE_CAP);
        }
        let lead = if !about.trim().is_empty() {
            about
        } else {
            full_body
        };
        return format!(
            "# {title}\n\n{}",
            clip_runes(lead.trim(), SUMMARY_FALLBACK_RUNES)
        );
    }
    let desc = about.trim();
    if desc.is_empty() {
        return format!("# {title}\n");
    }
    format!("# {title}\n\n{}", clip_runes(desc, ITEM_PAGE_RUNE_CAP))
}

pub struct ReduceInput<'a> {
    pub model: &'a str,
    pub slug: &'a str,
    pub title: &'a str,
    pub page_type: &'a str,
    pub existing: &'a str,
    pub about: &'a str,
    pub details: &'a str,
    pub cited: &'a [&'a Chunk],
    pub doc_title: &'a str,
    pub doc_summary: &'a str,
    pub valid_links: &'a str,
    pub language: &'a str,
    pub deleted_content: &'a str,
    pub remaining_sources: &'a str,
}

/// Brain `reduceSlugUpdates` / WikiPageModify: compile verbatim cited chunks into the page.
/// Summary pages are written from WikiSummaryPrompt in map, not here.
pub fn reduce_page(input: ReduceInput<'_>) -> (String, String) {
    let ReduceInput {
        model,
        slug,
        title,
        page_type,
        existing,
        about,
        details,
        cited,
        doc_title,
        doc_summary,
        valid_links,
        language,
        deleted_content,
        remaining_sources,
    } = input;
    let facts = if !details.trim().is_empty() {
        details
    } else {
        about
    };
    let draft = page_content_for(page_type, title, facts, cited, "", "");
    if page_type == PAGE_SUMMARY || !enrichment::chat_http_configured() {
        let lede = if !about.trim().is_empty() {
            first_lede(about)
        } else {
            first_lede(&draft)
        };
        return (draft, lede);
    }
    let evidence = cited_verbatim(cited);
    let new_body = if evidence.is_empty() {
        facts
    } else {
        &evidence
    };
    let existing = if existing.trim().is_empty() {
        "(New page)"
    } else {
        existing
    };
    let shared = if doc_summary.trim().is_empty() {
        String::new()
    } else {
        format!(
            "<shared_source_contexts>\n<document>\n<title>{doc_title}</title>\n<context>\n{doc_summary}\n</context>\n</document>\n</shared_source_contexts>\n\n"
        )
    };
    let has_add = !new_body.trim().is_empty() || !about.trim().is_empty();
    let new_block = if has_add {
        format!(
            "<new_information>\n<document>\n<title>{doc_title}</title>\n<content>\n**{title}**: {about}\n\n{new_body}\n</content>\n</document>\n</new_information>\n\nThe <new_information> block is assembled from VERBATIM source chunks already cited as supporting this page. <shared_source_contexts> is framing only, not evidence.\n"
        )
    } else {
        String::new()
    };
    let retract_block = if deleted_content.trim().is_empty() {
        String::new()
    } else {
        let remain = if remaining_sources.trim().is_empty() {
            "(no remaining sources)"
        } else {
            remaining_sources
        };
        format!(
            "<deleted_documents>\n{deleted_content}\n</deleted_documents>\n\n<remaining_source_documents>\n{remain}\n</remaining_source_documents>\n"
        )
    };
    let links = if valid_links.trim().is_empty() {
        "(none)"
    } else {
        valid_links
    };
    let prompt_user = format!(
        "{shared}<page_metadata>\n  <slug>{slug}</slug>\n  <title>{title}</title>\n  <type>{page_type}</type>\n</page_metadata>\n\nThis wiki page is specifically about **{title}** (a {page_type}). Every statement MUST be about this exact {page_type}.\n\n<existing_page_content>\n{existing}\n</existing_page_content>\n\n{new_block}{retract_block}\n<valid_wiki_links>\n{links}\n</valid_wiki_links>\n\n<instructions>\n1. FIRST line MUST be: SUMMARY: {{one sentence, 15-40 words}}\n2. MERGE facts from <new_information> about {title} only when that block is present. Compiler, not creative writer. Stay close to verbatim wording; do not invent transitions or filler.\n3. If new info is clearly about a different thing, reject it.\n4. REMOVE facts that were ONLY sourced from <deleted_documents> and are NOT in remaining sources or new information.\n5. Preserve still-valid existing facts about {title}. Do not copy cover/copyright/address blocks.\n6. Use \"# {title}\" as the top heading. Do not invent extra heading hierarchy. Do not output ## Sources.\n7. Keep [[slug|name]] only when the slug is in <valid_wiki_links>. Never link the page to itself. Never invent slugs.\n8. Write ALL output in {language}.\n</instructions>"
    );
    match enrichment::chat_complete_wiki(
        &format!(
            "You are a wiki editor updating one page. You are a COMPILER, not a creative writer. Output SUMMARY: line then Markdown. No preamble. Ground every new claim in <new_information>. Never output inline chunk ids. Do not invent slugs. Write in {language}."
        ),
        &prompt_user,
        model,
    ) {
        Ok(raw) if !raw.trim().is_empty() => {
            let (lede, body) = split_summary_line(&raw);
            let content = if body.is_empty() { raw } else { body };
            let lede = if lede.is_empty() {
                first_lede(&content)
            } else {
                lede
            };
            (content, lede)
        }
        _ => {
            let lede = first_lede(&draft);
            (draft, lede)
        }
    }
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
    fn document_language_detects_cjk() {
        assert_eq!(document_language("云安全管理平台产品白皮书"), "Chinese");
        assert_eq!(
            document_language("Cloud Security Management Platform"),
            "English"
        );
    }

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
        let raw = r#"{"entities":[{"name":"Alpha Switch","slug":"entity/alpha-switch","description":"A campus switch."}],"concepts":[{"name":"Throughput"}]}"#;
        let items = parse_extraction(raw);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].page_type, "entity");
        assert_eq!(items[0].description, "A campus switch.");
        assert_eq!(items[1].page_type, "concept");
        assert_eq!(items[1].slug, "concept/throughput");
        let junk = parse_extraction(
            r#"{"entities":[{"name":"65535"},{"name":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},{"name":"图193"},{"name":"TCP"}]}"#,
        );
        assert!(junk.is_empty(), "{junk:?}");
    }

    #[test]
    fn page_content_never_dumps_full_body() {
        let body = "封面 完全公开\n".repeat(800);
        let ch = Chunk {
            id: Uuid::new_v4(),
            document_id: Uuid::new_v4(),
            product_version_id: Uuid::new_v4(),
            chunk_type: "text".into(),
            content: "态势感知汇聚告警并展示风险。".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 14,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let page = page_content_for(
            PAGE_CONCEPT,
            "态势感知",
            "汇聚多源日志做风险可视化。",
            &[&ch],
            "",
            &body,
        );
        assert!(page.starts_with("# 态势感知"));
        assert!(page.contains("风险可视化"));
        assert!(!page.contains("## Sources"));
        assert!(page.chars().count() < 2000);
        assert!(!page.contains("完全公开"));
        let empty = page_content_for(PAGE_ENTITY, "Alpha", "", &[], "", &body);
        assert_eq!(empty, "# Alpha\n");
        assert!(empty.chars().count() < body.chars().count() / 10);
    }

    #[test]
    fn parse_citation_aliases_and_new_slugs() {
        let raw = r#"```json
{"citations":{"entity/qianxin":["c000","c002"]},"new_slugs":[{"type":"concept","name":"态势感知","slug":"concept/situational-awareness","aliases":["SOC"],"description":"汇聚告警。","details":"多源日志。","source_chunks":["c001"]}]}
```"#;
        let parsed = parse_citation_json(raw);
        assert_eq!(parsed.citations["entity/qianxin"], vec!["c000", "c002"]);
        assert_eq!(parsed.new_slugs.len(), 1);
        assert_eq!(parsed.new_slugs[0].0.slug, "concept/situational-awareness");
        assert_eq!(parsed.new_slugs[0].1, vec!["c001"]);
    }

    #[test]
    fn citation_batches_split_on_rune_budget() {
        let vid = Uuid::new_v4();
        let did = Uuid::new_v4();
        let big = "字".repeat(MAX_RUNES_PER_CITATION_BATCH / 2 + 10);
        let mk = |i: i32, text: &str| Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: text.into(),
            context_header: String::new(),
            start_at: i,
            end_at: i + 1,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let chunks = vec![mk(0, &big), mk(1, &big), mk(2, "short")];
        let batches = split_citation_batches(&chunks);
        assert!(batches.len() >= 2, "{batches:?}");
        assert_eq!(batches.iter().map(|b| b.len()).sum::<usize>(), 3);
    }

    #[test]
    fn attach_falls_back_to_substring_and_skips_cover() {
        let cover = Chunk {
            id: Uuid::new_v4(),
            document_id: Uuid::new_v4(),
            product_version_id: Uuid::new_v4(),
            chunk_type: "text".into(),
            content: "完全公开\n版权声明\nCopyright 2020\n地址：北京\n邮编：100044\n奇安信集团"
                .into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 40,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let body = Chunk {
            id: Uuid::new_v4(),
            document_id: cover.document_id,
            product_version_id: cover.product_version_id,
            chunk_type: "text".into(),
            content: "奇安信集团提供云安全管理平台与安全资源池。".into(),
            context_header: String::new(),
            start_at: 40,
            end_at: 60,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let items = vec![Candidate {
            title: "奇安信集团".into(),
            slug: "entity/qianxin".into(),
            page_type: PAGE_ENTITY.into(),
            aliases: Vec::new(),
            about: "安全厂商".into(),
            details: String::new(),
            source_refs: Vec::new(),
        }];
        let out = attach_citations(items, HashMap::new(), Vec::new(), &[cover, body.clone()]);
        assert_eq!(out[0].source_refs, vec![body.id]);
        assert_eq!(cited_verbatim(&[&body]), body.content);
    }

    #[test]
    fn cite_skips_cover_block() {
        let cover = Chunk {
            id: Uuid::new_v4(),
            document_id: Uuid::new_v4(),
            product_version_id: Uuid::new_v4(),
            chunk_type: "text".into(),
            content: "完全公开\n版权声明\nCopyright 2020\n地址：北京\n邮编：100044\n奇安信集团"
                .into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 40,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let body = Chunk {
            id: Uuid::new_v4(),
            document_id: cover.document_id,
            product_version_id: cover.product_version_id,
            chunk_type: "text".into(),
            content: "奇安信集团提供云安全管理平台与安全资源池。".into(),
            context_header: String::new(),
            start_at: 40,
            end_at: 60,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let ids = cite_chunks(&[cover.clone(), body.clone()], "奇安信集团", &[]);
        assert_eq!(ids, vec![body.id]);
    }

    #[test]
    fn split_summary_and_lede() {
        let (line, body) = split_summary_line("SUMMARY: 云安全平台产品白皮书\n# 标题\n\n正文");
        assert_eq!(line, "云安全平台产品白皮书");
        assert!(body.starts_with("# 标题"));
        assert_eq!(
            first_lede("# 态势感知\n\n用于展示风险。\n\n更多"),
            "用于展示风险。"
        );
        assert_eq!(
            first_lede("用于展示整体网络安全态势。\n\n后段"),
            "用于展示整体网络安全态势。"
        );
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
