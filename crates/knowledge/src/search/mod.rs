//! Assembly and matching retrieval over in-memory hybrid hits.

mod answer;

pub use answer::{
    ANSWER_SYSTEM_PROMPT, AnswerRequest, AnswerResponse, Citation, answer, answer_from_hits,
    current_summary_model, render_answer_system,
};

use crate::index::{cosine, embed_index, keyword_score};
use crate::{ProductKind, Store, VersionStatus};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub text: String,
    #[serde(default = "one")]
    pub weight: f64,
    #[serde(default)]
    pub must: bool,
    #[serde(default)]
    pub tag_ids: Vec<Uuid>,
    #[serde(default)]
    pub use_library: bool,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    #[serde(default = "assembly_mode")]
    pub mode: String,
    pub query: Option<String>,
    pub product_id: Option<Uuid>,
    pub version_id: Option<String>,
    #[serde(default)]
    pub include_library: bool,
    #[serde(default)]
    pub tag_ids: Vec<Uuid>,
    #[serde(default = "ten")]
    pub match_count: usize,
    #[serde(default = "yes")]
    pub expand_wiki: bool,
    #[serde(default = "yes")]
    pub expand_graph: bool,
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    #[serde(default = "current_scope")]
    pub version_scope: String,
    #[serde(default)]
    pub product_ids: Vec<Uuid>,
    pub workspace_id: Option<Uuid>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default = "none_group")]
    pub group_by: String,
    #[serde(default)]
    pub tender_text: Option<String>,
}

fn none_group() -> String {
    "none".into()
}

pub fn over_fetch(match_count: usize, num_targets: usize) -> usize {
    let raw = match_count.max(1) * 5 * num_targets.max(1);
    raw.clamp(50, 500)
}

pub fn per_target_limit(match_count: usize, num_targets: usize) -> usize {
    let total = over_fetch(match_count, num_targets);
    (total / num_targets.max(1)).max(match_count.max(1))
}

fn apply_group_by(mut hits: Vec<Hit>, group_by: &str, match_count: usize) -> Vec<Hit> {
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let cap = match_count.max(1);
    if group_by != "version" && group_by != "product" {
        hits.truncate(cap);
        return hits;
    }
    let mut buckets: std::collections::BTreeMap<Uuid, Vec<Hit>> = std::collections::BTreeMap::new();
    for h in hits {
        let key = if group_by == "version" {
            h.version_id
        } else {
            h.product_id
        };
        buckets.entry(key).or_default().push(h);
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    loop {
        let mut added = false;
        for bucket in buckets.values() {
            if let Some(h) = bucket.get(i) {
                out.push(h.clone());
                added = true;
                if out.len() >= cap {
                    return out;
                }
            }
        }
        if !added {
            break;
        }
        i += 1;
    }
    out
}

fn split_tender_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(|c: char| {
                    c.is_ascii_digit() || matches!(c, '.' | ')' | '-' | '、' | ' ')
                })
                .trim()
                .to_string()
        })
        .filter(|l| l.chars().count() >= 4)
        .take(30)
        .collect()
}

fn requirements_from_tender(tender: &str, model_id: &str, tag_ids: &[Uuid]) -> Vec<Requirement> {
    let prompt = "Split the following tender into discrete requirements. \
                  Output one requirement per line. No preamble.";
    let raw = crate::enrichment::chat_complete(prompt, tender, model_id).unwrap_or_default();
    let mut lines = split_tender_lines(&raw);
    if lines.is_empty() {
        lines = split_tender_lines(tender);
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(i, text)| Requirement {
            id: format!("t{i}"),
            text,
            weight: 1.0,
            must: false,
            tag_ids: tag_ids.to_vec(),
            use_library: false,
        })
        .collect()
}

fn assembly_mode() -> String {
    "assembly".into()
}
fn ten() -> usize {
    10
}
fn yes() -> bool {
    true
}
fn current_scope() -> String {
    "current".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub id: Uuid,
    pub content: String,
    pub score: f64,
    pub match_type: String,
    pub chunk_type: String,
    pub document_id: Uuid,
    pub document_title: String,
    pub product_id: Uuid,
    pub product_kind: String,
    pub version_id: Uuid,
    pub version_label: String,
    pub is_current: bool,
    pub tag_ids: Vec<Uuid>,
    pub tag_slugs: Vec<String>,
    pub start_at: i32,
    pub end_at: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_object_ref: Option<String>,
}

pub fn image_key_for_hit(
    chunk_type: &str,
    context_header: &str,
    doc_object_ref: &str,
) -> Option<String> {
    if !matches!(chunk_type, "image_ocr" | "image_caption") {
        return None;
    }
    if !context_header.is_empty() {
        Some(context_header.to_string())
    } else if !doc_object_ref.is_empty() {
        Some(doc_object_ref.to_string())
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblyResponse {
    pub hits: Vec<Hit>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReqResult {
    pub id: String,
    pub hit: bool,
    pub score: f64,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub product_id: Uuid,
    pub product_title: String,
    pub matched_version_id: Uuid,
    pub matched_version_label: String,
    pub score: f64,
    pub coverage: f64,
    pub unmet_must: Vec<String>,
    pub requirements: Vec<ReqResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClauseHit {
    pub id: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<Hit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alts: Vec<ClauseAlt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClauseAlt {
    pub document_id: Uuid,
    pub file_name: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchingResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Candidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clauses: Vec<ClauseHit>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct SearchError {
    pub code: &'static str,
    pub message: String,
}

pub fn assembly(store: &Store, req: &SearchRequest) -> Result<AssemblyResponse, SearchError> {
    let product_id = req.product_id.ok_or(SearchError {
        code: "VALIDATION",
        message: "product_id required for assembly".into(),
    })?;
    let product = store.products.get(&product_id).ok_or(SearchError {
        code: "NOT_FOUND",
        message: "product not found".into(),
    })?;
    let query = req.query.clone().unwrap_or_default();
    if query.trim().is_empty() {
        return Err(SearchError {
            code: "VALIDATION",
            message: "query required".into(),
        });
    }
    let mut targets = resolve_assembly_targets(store, product, req)?;
    if targets.len() > 20 {
        return Err(SearchError {
            code: "TOO_MANY_TARGETS",
            message: "more than 20 target versions".into(),
        });
    }
    check_same_embedding(store, &targets)?;
    let n_targets = targets.len();
    let mut hits = Vec::new();
    let mut warnings = Vec::new();
    for vid in targets.drain(..) {
        match hybrid_search(
            store,
            vid,
            &query,
            req,
            req.include_library && product.kind == ProductKind::Product,
        ) {
            Ok(mut h) => hits.append(&mut h),
            Err(w) => warnings.push(w),
        }
    }
    if hits.is_empty() && !warnings.is_empty() && warnings.len() == n_targets {
        return Err(SearchError {
            code: "UPSTREAM",
            message: warnings.join("; "),
        });
    }
    let hits = apply_group_by(fuse_by_chunk_id(hits), &req.group_by, req.match_count);
    Ok(AssemblyResponse { hits, warnings })
}

fn fuse_by_chunk_id(hits: Vec<Hit>) -> Vec<Hit> {
    let mut best: std::collections::HashMap<Uuid, Hit> = std::collections::HashMap::new();
    for h in hits {
        match best.get(&h.id) {
            Some(old) if old.score >= h.score => {}
            _ => {
                best.insert(h.id, h);
            }
        }
    }
    best.into_values().collect()
}

/// Assembly over 0004 `chunk_embeddings` when the memory store has no hits.
pub async fn assembly_pg(
    pool: &sqlx::PgPool,
    req: &SearchRequest,
) -> Result<AssemblyResponse, SearchError> {
    let product_id = req.product_id.ok_or(SearchError {
        code: "VALIDATION",
        message: "product_id required for assembly".into(),
    })?;
    let query = req.query.clone().unwrap_or_default();
    if query.trim().is_empty() {
        return Err(SearchError {
            code: "VALIDATION",
            message: "query required".into(),
        });
    }
    let targets = crate::resolve_pg_assembly_targets(
        pool,
        product_id,
        req.version_id.as_deref(),
        req.include_library,
    )
    .await
    .map_err(|e| SearchError {
        code: "INTERNAL",
        message: e.to_string(),
    })?;
    if targets.is_empty() {
        let code = if req.version_id.as_deref() == Some("current") {
            "VALIDATION"
        } else {
            "NOT_FOUND"
        };
        return Err(SearchError {
            code,
            message: if code == "VALIDATION" {
                "no current version".into()
            } else {
                "version not found".into()
            },
        });
    }
    if targets.len() > 20 {
        return Err(SearchError {
            code: "TOO_MANY_TARGETS",
            message: "more than 20 target versions".into(),
        });
    }
    let models = crate::embedding_models_for_versions(pool, &targets)
        .await
        .map_err(|e| SearchError {
            code: "INTERNAL",
            message: e.to_string(),
        })?;
    let mut seen: Option<String> = None;
    for (_, m) in &models {
        match &seen {
            None => seen = Some(m.clone()),
            Some(id) if id != m => {
                return Err(SearchError {
                    code: "EMBEDDING_MISMATCH",
                    message: "target versions use different embedding models".into(),
                });
            }
            _ => {}
        }
    }
    let model = seen.unwrap_or_default();
    let qv = embed_index(&query, &model).map_err(|e| SearchError {
        code: "UPSTREAM",
        message: e,
    })?;
    let qv = crate::vector_literal(&qv);
    let limit = per_target_limit(req.match_count, targets.len()) as i64;
    let (vth, kth) = crate::workspace_thresholds_for_product(pool, product_id)
        .await
        .unwrap_or((0.15, 0.3));
    let mut hits = Vec::new();
    let mut warnings = Vec::new();
    let mut failed = 0usize;
    for chunk in targets.chunks(4) {
        let mut handles = Vec::new();
        for vid in chunk {
            let pool = pool.clone();
            let q = query.clone();
            let qv = qv.clone();
            let tags = req.tag_ids.clone();
            let wiki = req.expand_wiki;
            let graph = req.expand_graph;
            let vid = *vid;
            handles.push(tokio::spawn(async move {
                let rows = crate::hybrid_search_pg(&pool, vid, &q, &qv, &tags, wiki, limit).await;
                match rows {
                    Ok(rows) => {
                        let mut h = hits_from_pg_rows(rows, vth, kth);
                        if graph {
                            h.extend(
                                graph_hits_for_version(&pool, vid, &q, limit as usize, kth, &tags)
                                    .await?,
                            );
                        }
                        Ok::<_, SearchError>(h)
                    }
                    Err(e) => Err(SearchError {
                        code: "INTERNAL",
                        message: e.to_string(),
                    }),
                }
            }));
        }
        for h in handles {
            match h.await {
                Ok(Ok(mut got)) => hits.append(&mut got),
                Ok(Err(e)) => {
                    failed += 1;
                    warnings.push(e.message);
                }
                Err(e) => {
                    failed += 1;
                    warnings.push(e.to_string());
                }
            }
        }
    }
    if failed == targets.len() && !targets.is_empty() {
        return Err(SearchError {
            code: "UPSTREAM",
            message: warnings.join("; "),
        });
    }
    let hits = apply_group_by(fuse_by_chunk_id(hits), &req.group_by, req.match_count);
    Ok(AssemblyResponse { hits, warnings })
}

fn hits_from_pg_rows(rows: Vec<crate::PgSearchHit>, vth: f64, kth: f64) -> Vec<Hit> {
    let mut hits = Vec::new();
    for row in rows {
        let mut score = 0.0f64;
        let mut match_type = "none";
        if row.vec_score >= vth && row.vec_score > score {
            score = row.vec_score;
            match_type = "vector";
        }
        if row.kw_score >= kth && row.kw_score > score {
            score = row.kw_score;
            match_type = "keyword";
        }
        if score <= 0.0 {
            continue;
        }
        if row.is_current && row.product_kind == "product" {
            score *= 1.15;
        }
        if row.chunk_type == "wiki_page" {
            score *= 1.3;
        }
        hits.push(Hit {
            id: row.chunk_id,
            content: row.content,
            score,
            match_type: match_type.into(),
            chunk_type: row.chunk_type.clone(),
            document_id: row.document_id,
            document_title: row.document_title,
            product_id: row.product_id,
            product_kind: row.product_kind,
            version_id: row.version_id,
            version_label: row.version_label,
            is_current: row.is_current,
            tag_ids: row.tag_ids,
            tag_slugs: row.tag_slugs,
            start_at: row.start_at,
            end_at: row.end_at,
            image_object_ref: image_key_for_hit(
                &row.chunk_type,
                &row.context_header,
                &row.document_object_ref,
            ),
        });
    }
    hits
}

async fn graph_hits_for_version(
    pool: &sqlx::PgPool,
    version_id: Uuid,
    query: &str,
    match_count: usize,
    kth: f64,
    tag_ids: &[Uuid],
) -> Result<Vec<Hit>, SearchError> {
    let rows = crate::graph_hits_pg(pool, version_id, query, match_count.max(1) as i64, tag_ids)
        .await
        .map_err(|e| SearchError {
            code: "INTERNAL",
            message: e.to_string(),
        })?;
    let mut hits = Vec::new();
    for row in rows {
        let ks = keyword_score(query, &row.name);
        if ks < kth && !query.to_lowercase().contains(&row.name.to_lowercase()) {
            continue;
        }
        let score = if ks >= kth { ks } else { kth };
        hits.push(Hit {
            id: row.chunk_id,
            content: format!("{}: {}", row.name, row.content),
            score,
            match_type: "graph".into(),
            chunk_type: "entity".into(),
            document_id: row.document_id,
            document_title: row.document_title,
            product_id: row.product_id,
            product_kind: row.product_kind,
            version_id: row.version_id,
            version_label: row.version_label,
            is_current: row.is_current,
            tag_ids: row.tag_ids,
            tag_slugs: row.tag_slugs,
            start_at: row.start_at,
            end_at: row.end_at,
            image_object_ref: None,
        });
    }
    Ok(hits)
}

fn resolve_assembly_targets(
    store: &Store,
    product: &crate::Product,
    req: &SearchRequest,
) -> Result<Vec<Uuid>, SearchError> {
    let mut targets = Vec::new();
    if let Some(ref vs) = req.version_id {
        if vs == "current" && product.current_version_id.is_none() {
            return Err(SearchError {
                code: "VALIDATION",
                message: "no current version".into(),
            });
        }
        let vid = store.resolve_version(product.id, vs).ok_or(SearchError {
            code: "NOT_FOUND",
            message: "version not found".into(),
        })?;
        targets.push(vid);
    } else {
        targets.extend(
            store
                .versions
                .values()
                .filter(|v| v.product_id == product.id && v.status == VersionStatus::Active)
                .map(|v| v.id),
        );
    }
    if req.include_library && product.kind == ProductKind::Product {
        for lib in store
            .products
            .values()
            .filter(|p| p.workspace_id == product.workspace_id && p.kind == ProductKind::Library)
        {
            if let Some(cid) = lib.current_version_id
                && store
                    .versions
                    .get(&cid)
                    .is_some_and(|v| v.status == VersionStatus::Active)
            {
                targets.push(cid);
            }
        }
    }
    Ok(targets)
}

fn check_same_embedding(store: &Store, targets: &[Uuid]) -> Result<(), SearchError> {
    let mut seen: Option<String> = None;
    for vid in targets {
        let Some(v) = store.versions.get(vid) else {
            continue;
        };
        match &seen {
            None => seen = Some(v.embedding_model_id.clone()),
            Some(id)
                if !id.is_empty()
                    && !v.embedding_model_id.is_empty()
                    && id != &v.embedding_model_id =>
            {
                return Err(SearchError {
                    code: "EMBEDDING_MISMATCH",
                    message: "target versions use different embedding models".into(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn hybrid_search(
    store: &Store,
    version_id: Uuid,
    query: &str,
    req: &SearchRequest,
    _lib_boost_skip: bool,
) -> Result<Vec<Hit>, String> {
    let version = store
        .versions
        .get(&version_id)
        .ok_or_else(|| "missing version".to_string())?;
    let product = store
        .products
        .get(&version.product_id)
        .ok_or_else(|| "missing product".to_string())?;
    let ws = store.workspaces.get(&product.workspace_id);
    let vth = ws.map(|w| w.retrieval.vector_threshold).unwrap_or(0.15);
    let kth = ws.map(|w| w.retrieval.keyword_threshold).unwrap_or(0.3);
    let qv = embed_index(query, &version.embedding_model_id)?;
    let is_current = product.current_version_id == Some(version_id);
    let mut hits = Vec::new();
    for emb in store
        .embeddings
        .values()
        .filter(|e| e.product_version_id == version_id)
    {
        let Some(chunk) = store.chunks.get(&emb.chunk_id) else {
            continue;
        };
        if !req.expand_wiki && chunk.chunk_type == "wiki_page" {
            continue;
        }
        let Some(doc) = store.documents.get(&emb.document_id) else {
            continue;
        };
        if doc.enable_status != "enabled" {
            continue;
        }
        if !req.tag_ids.is_empty() {
            let tags = store.document_tag_ids(doc.id);
            if !req.tag_ids.iter().any(|t| tags.contains(t)) {
                continue;
            }
        }
        let mut score = 0.0f64;
        let mut match_type = "none";
        if version.vector_enabled && !emb.vector.is_empty() {
            let s = cosine(&qv, &emb.vector);
            if s >= vth && s > score {
                score = s;
                match_type = "vector";
            }
        }
        if version.keyword_enabled {
            let s = keyword_score(query, &emb.content);
            if s >= kth && s > score {
                score = s;
                match_type = "keyword";
            }
        }
        if score <= 0.0 {
            continue;
        }
        if is_current && product.kind == ProductKind::Product {
            score *= 1.15;
        }
        if chunk.chunk_type == "wiki_page" {
            score *= 1.3;
        }
        let tag_ids = store.document_tag_ids(doc.id);
        let tag_slugs = tag_ids
            .iter()
            .filter_map(|id| store.tags.get(id).map(|t| t.slug.clone()))
            .collect();
        hits.push(Hit {
            id: chunk.id,
            content: chunk.content.clone(),
            score,
            match_type: match_type.into(),
            chunk_type: chunk.chunk_type.clone(),
            document_id: doc.id,
            document_title: doc.title.clone(),
            product_id: product.id,
            product_kind: match product.kind {
                ProductKind::Product => "product".into(),
                ProductKind::Library => "library".into(),
            },
            version_id,
            version_label: version.label.clone(),
            is_current,
            tag_ids,
            tag_slugs,
            start_at: chunk.start_at,
            end_at: chunk.end_at,
            image_object_ref: image_key_for_hit(
                &chunk.chunk_type,
                &chunk.context_header,
                &doc.object_ref,
            ),
        });
    }
    if req.expand_graph {
        for node in store.graph.values().filter(|n| n.version_id == version_id) {
            if !req.tag_ids.is_empty() {
                let tags = store.document_tag_ids(node.document_id);
                if !req.tag_ids.iter().any(|t| tags.contains(t)) {
                    continue;
                }
            }
            let name_hit = query.to_lowercase().contains(&node.name.to_lowercase())
                || node.name.to_lowercase().contains(&query.to_lowercase());
            let ks = keyword_score(query, &node.name);
            if name_hit
                && ks >= kth
                && let Some(cid) = node.chunk_ids.first()
                && let Some(ch) = store.chunks.get(cid)
            {
                let tag_ids = store.document_tag_ids(node.document_id);
                let tag_slugs = tag_ids
                    .iter()
                    .filter_map(|id| store.tags.get(id).map(|t| t.slug.clone()))
                    .collect();
                hits.push(Hit {
                    id: ch.id,
                    content: format!("{}: {}", node.name, ch.content),
                    score: ks,
                    match_type: "graph".into(),
                    chunk_type: "entity".into(),
                    document_id: node.document_id,
                    document_title: store
                        .documents
                        .get(&node.document_id)
                        .map(|d| d.title.clone())
                        .unwrap_or_default(),
                    product_id: product.id,
                    product_kind: match product.kind {
                        ProductKind::Product => "product".into(),
                        ProductKind::Library => "library".into(),
                    },
                    version_id,
                    version_label: version.label.clone(),
                    is_current,
                    tag_ids,
                    tag_slugs,
                    start_at: ch.start_at,
                    end_at: ch.end_at,
                    image_object_ref: None,
                });
            }
        }
    }
    let mut hits = fuse_by_chunk_id(hits);
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(hits)
}

pub fn matching(store: &Store, req: &SearchRequest) -> Result<MatchingResponse, SearchError> {
    if req.product_id.is_some() {
        return Err(SearchError {
            code: "VALIDATION",
            message: "product_id is forbidden for matching".into(),
        });
    }
    if req.scope.as_deref() == Some("company") {
        return matching_company(store, req);
    }
    let workspace_id = if req.scope.as_deref() == Some("product_lines") {
        req.workspace_id.unwrap_or(Uuid::nil())
    } else {
        req.workspace_id.ok_or(SearchError {
            code: "VALIDATION",
            message: "workspace required".into(),
        })?
    };
    let mut requirements = req.requirements.clone();
    if requirements.is_empty()
        && let Some(t) = &req.tender_text
        && !t.trim().is_empty()
    {
        let model = workspace_summary_model(store, workspace_id);
        requirements = requirements_from_tender(t, &model, &req.tag_ids);
    }
    if requirements.is_empty()
        && let Some(q) = &req.query
        && !q.trim().is_empty()
    {
        requirements.push(Requirement {
            id: "q0".into(),
            text: q.clone(),
            weight: 1.0,
            must: false,
            tag_ids: req.tag_ids.clone(),
            use_library: req.include_library,
        });
    }
    if requirements.is_empty() || requirements.len() > 30 {
        return Err(SearchError {
            code: "VALIDATION",
            message: "requirements must have 1–30 items".into(),
        });
    }
    for r in &requirements {
        if r.text.trim().is_empty() {
            return Err(SearchError {
                code: "VALIDATION",
                message: "requirement text empty".into(),
            });
        }
    }
    let mut products: Vec<_> = store
        .products
        .values()
        .filter(|p| {
            if p.kind != ProductKind::Product {
                return false;
            }
            if req.scope.as_deref() == Some("product_lines") {
                return store
                    .workspaces
                    .get(&p.workspace_id)
                    .is_some_and(|w| w.kind == crate::WorkspaceKind::ProductLine);
            }
            p.workspace_id == workspace_id
        })
        .cloned()
        .collect();
    if !req.product_ids.is_empty() {
        products.retain(|p| req.product_ids.contains(&p.id));
    }
    if req.scope.as_deref() != Some("product_lines") {
        products.truncate(50);
    }
    if req.scope.as_deref() == Some("product_lines") {
        check_scope_thresholds(store, &products)?;
    }
    let mut all_targets = Vec::new();
    for p in &products {
        all_targets.extend(product_versions(store, p, &req.version_scope));
    }
    if req.include_library || requirements.iter().any(|r| r.use_library) {
        for lib in store
            .products
            .values()
            .filter(|lp| lp.workspace_id == workspace_id && lp.kind == ProductKind::Library)
        {
            if let Some(cid) = lib.current_version_id
                && store
                    .versions
                    .get(&cid)
                    .is_some_and(|v| v.status == VersionStatus::Active)
            {
                all_targets.push(cid);
            }
        }
    }
    check_same_embedding(store, &all_targets)?;
    let mut candidates = Vec::new();
    let warnings = Vec::new();
    for p in &products {
        let versions = product_versions(store, p, &req.version_scope);
        if versions.is_empty() {
            continue;
        }
        let mut best: Option<Candidate> = None;
        for vid in versions {
            let mut req_results = Vec::new();
            let mut weighted = 0.0;
            let mut wsum = 0.0;
            let mut hit_w = 0.0;
            let mut unmet = Vec::new();
            for r in &requirements {
                let sub = SearchRequest {
                    mode: "assembly".into(),
                    query: Some(r.text.clone()),
                    product_id: Some(p.id),
                    version_id: Some(vid.to_string()),
                    include_library: req.include_library || r.use_library,
                    tag_ids: r.tag_ids.clone(),
                    match_count: req.match_count,
                    expand_wiki: req.expand_wiki,
                    expand_graph: req.expand_graph,
                    requirements: Vec::new(),
                    version_scope: req.version_scope.clone(),
                    product_ids: Vec::new(),
                    workspace_id: Some(workspace_id),
                    scope: None,
                    group_by: req.group_by.clone(),
                    tender_text: None,
                };
                // hybrid on this version + optional library
                let mut hits = hybrid_search(store, vid, &r.text, &sub, false).unwrap_or_default();
                if sub.include_library {
                    for lib in store.products.values().filter(|lp| {
                        lp.workspace_id == workspace_id && lp.kind == ProductKind::Library
                    }) {
                        if let Some(cid) = lib.current_version_id
                            && let Ok(mut extra) = hybrid_search(store, cid, &r.text, &sub, true)
                        {
                            hits.append(&mut extra);
                        }
                    }
                }
                hits.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                hits.truncate(per_target_limit(req.match_count, 1));
                hits.truncate(req.match_count.max(1));
                let score = hits.first().map(|h| h.score).unwrap_or(0.0);
                let hit = score > 0.0;
                if r.must && !hit {
                    unmet.push(r.id.clone());
                }
                weighted += r.weight * score;
                wsum += r.weight;
                if hit {
                    hit_w += r.weight;
                }
                req_results.push(ReqResult {
                    id: r.id.clone(),
                    hit,
                    score,
                    hits,
                });
            }
            let score = if wsum == 0.0 { 0.0 } else { weighted / wsum };
            let coverage = if wsum == 0.0 { 0.0 } else { hit_w / wsum };
            let cand = Candidate {
                product_id: p.id,
                product_title: p.name.clone(),
                matched_version_id: vid,
                matched_version_label: store
                    .versions
                    .get(&vid)
                    .map(|v| v.label.clone())
                    .unwrap_or_default(),
                score,
                coverage,
                unmet_must: unmet,
                requirements: req_results,
            };
            let replace = match &best {
                None => true,
                Some(b) => cand.score > b.score,
            };
            if replace {
                best = Some(cand);
            }
        }
        if let Some(c) = best {
            candidates.push(c);
        }
    }
    candidates.sort_by(|a, b| {
        let au = a.unmet_must.is_empty();
        let bu = b.unmet_must.is_empty();
        bu.cmp(&au).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    Ok(MatchingResponse {
        candidates,
        clauses: Vec::new(),
        warnings,
    })
}

fn check_scope_thresholds(store: &Store, products: &[crate::Product]) -> Result<(), SearchError> {
    let mut seen: Option<(f64, f64)> = None;
    for p in products {
        let Some(ws) = store.workspaces.get(&p.workspace_id) else {
            continue;
        };
        let pair = (
            ws.retrieval.vector_threshold,
            ws.retrieval.keyword_threshold,
        );
        match seen {
            None => seen = Some(pair),
            Some(prev) if prev != pair => {
                return Err(SearchError {
                    code: "EMBEDDING_MISMATCH",
                    message: "product_line retrieval thresholds differ".into(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn matching_company(store: &Store, req: &SearchRequest) -> Result<MatchingResponse, SearchError> {
    let mut requirements = req.requirements.clone();
    if requirements.is_empty()
        && let Some(q) = &req.query
        && !q.trim().is_empty()
    {
        requirements.push(Requirement {
            id: "q0".into(),
            text: q.clone(),
            weight: 1.0,
            must: false,
            tag_ids: req.tag_ids.clone(),
            use_library: false,
        });
    }
    if requirements.is_empty() || requirements.len() > 30 {
        return Err(SearchError {
            code: "VALIDATION",
            message: "requirements must have 1–30 items".into(),
        });
    }
    let libs: Vec<_> = store
        .products
        .values()
        .filter(|p| {
            p.kind == ProductKind::Library
                && store
                    .workspaces
                    .get(&p.workspace_id)
                    .is_some_and(|w| w.kind == crate::WorkspaceKind::Company)
        })
        .cloned()
        .collect();
    let mut clauses = Vec::new();
    for r in &requirements {
        let mut all = Vec::new();
        for lib in &libs {
            let Some(vid) = lib.current_version_id else {
                continue;
            };
            let sub = SearchRequest {
                mode: "assembly".into(),
                query: Some(r.text.clone()),
                product_id: Some(lib.id),
                version_id: Some(vid.to_string()),
                include_library: false,
                tag_ids: r.tag_ids.clone(),
                match_count: req.match_count,
                expand_wiki: false,
                expand_graph: false,
                requirements: Vec::new(),
                version_scope: "current".into(),
                product_ids: Vec::new(),
                workspace_id: Some(lib.workspace_id),
                scope: None,
                group_by: "none".into(),
                tender_text: None,
            };
            if let Ok(mut hits) = hybrid_search(store, vid, &r.text, &sub, false) {
                hits.retain(|h| h.chunk_type != "wiki_page" && h.match_type != "graph");
                all.append(&mut hits);
            }
        }
        all.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let best = all.first().cloned();
        let hit = best.as_ref().is_some_and(|h| h.score > 0.0);
        let alts = if all.len() > 1 {
            all.iter()
                .skip(1)
                .filter_map(|h| {
                    store.documents.get(&h.document_id).map(|d| ClauseAlt {
                        document_id: d.id,
                        file_name: d.file_name.clone(),
                        score: h.score,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        clauses.push(ClauseHit {
            id: r.id.clone(),
            outcome: if hit { "hit".into() } else { "miss".into() },
            document_id: best.as_ref().and_then(|h| hit.then_some(h.document_id)),
            version_id: best.as_ref().and_then(|h| hit.then_some(h.version_id)),
            file_name: best.as_ref().and_then(|h| {
                hit.then(|| {
                    store
                        .documents
                        .get(&h.document_id)
                        .map(|d| d.file_name.clone())
                        .unwrap_or_default()
                })
            }),
            score: best.as_ref().and_then(|h| hit.then_some(h.score)),
            product_id: best.as_ref().and_then(|h| hit.then_some(h.product_id)),
            hits: if hit {
                best.clone().into_iter().collect()
            } else {
                Vec::new()
            },
            alts,
        });
    }
    Ok(MatchingResponse {
        candidates: Vec::new(),
        clauses,
        warnings: Vec::new(),
    })
}

/// Matching when the workspace index lives in Postgres, not the in-memory store.
pub async fn matching_pg(
    pool: &sqlx::PgPool,
    req: &SearchRequest,
) -> Result<MatchingResponse, SearchError> {
    if req.product_id.is_some() {
        return Err(SearchError {
            code: "VALIDATION",
            message: "product_id is forbidden for matching".into(),
        });
    }
    if req.scope.as_deref() == Some("company") {
        return matching_company_pg(pool, req).await;
    }
    let scoped_lines = req.scope.as_deref() == Some("product_lines");
    let workspace_id = if scoped_lines {
        req.workspace_id.unwrap_or(Uuid::nil())
    } else {
        req.workspace_id.ok_or(SearchError {
            code: "VALIDATION",
            message: "workspace required".into(),
        })?
    };
    let mut requirements = req.requirements.clone();
    if !scoped_lines
        && requirements.is_empty()
        && let Some(t) = &req.tender_text
        && !t.trim().is_empty()
    {
        let model: String = sqlx::query_scalar(
            "SELECT COALESCE(pv.summary_model_id, 'stub-chat')
             FROM products p
             JOIN product_versions pv ON pv.id = p.current_version_id
             WHERE p.workspace_id = $1 AND p.kind = 'product' AND pv.status = 'active'
             LIMIT 1",
        )
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "stub-chat".into());
        requirements = requirements_from_tender(t, &model, &req.tag_ids);
    }
    if requirements.is_empty()
        && let Some(q) = &req.query
        && !q.trim().is_empty()
    {
        requirements.push(Requirement {
            id: "q0".into(),
            text: q.clone(),
            weight: 1.0,
            must: false,
            tag_ids: req.tag_ids.clone(),
            use_library: req.include_library,
        });
    }
    if requirements.is_empty() || requirements.len() > 30 {
        return Err(SearchError {
            code: "VALIDATION",
            message: "requirements must have 1–30 items".into(),
        });
    }
    for r in &requirements {
        if r.text.trim().is_empty() {
            return Err(SearchError {
                code: "VALIDATION",
                message: "requirement text empty".into(),
            });
        }
    }
    let all_active = req.version_scope == "all_active";
    if scoped_lines {
        check_product_line_thresholds_pg(pool).await?;
    }
    let rows = if scoped_lines {
        sqlx::query(
            "SELECT p.id AS product_id, p.name AS product_name, pv.id AS version_id,
                    pv.label AS version_label, COALESCE(pv.embedding_model_id, '') AS emb
             FROM products p
             JOIN workspaces w ON w.id = p.workspace_id
             JOIN product_versions pv ON pv.product_id = p.id
             WHERE COALESCE(w.kind, 'product_line') = 'product_line'
               AND p.kind = 'product'
               AND pv.deleted_at IS NULL
               AND pv.status = 'active'
               AND ($1 OR pv.id = p.current_version_id)",
        )
        .bind(all_active)
        .fetch_all(pool)
    } else {
        sqlx::query(
            "SELECT p.id AS product_id, p.name AS product_name, pv.id AS version_id,
                    pv.label AS version_label, COALESCE(pv.embedding_model_id, '') AS emb
             FROM products p
             JOIN product_versions pv ON pv.product_id = p.id
             WHERE p.workspace_id = $1
               AND p.kind = 'product'
               AND pv.deleted_at IS NULL
               AND pv.status = 'active'
               AND ($2 OR pv.id = p.current_version_id)",
        )
        .bind(workspace_id)
        .bind(all_active)
        .fetch_all(pool)
    }
    .await
    .map_err(|e| SearchError {
        code: "INTERNAL",
        message: e.to_string(),
    })?;
    #[derive(Clone)]
    struct Ver {
        product_id: Uuid,
        product_name: String,
        version_id: Uuid,
        version_label: String,
        emb: String,
    }
    let mut versions = Vec::new();
    for r in rows {
        use sqlx::Row;
        let pid: Uuid = r.try_get("product_id").map_err(|e| SearchError {
            code: "INTERNAL",
            message: e.to_string(),
        })?;
        if !req.product_ids.is_empty() && !req.product_ids.contains(&pid) {
            continue;
        }
        versions.push(Ver {
            product_id: pid,
            product_name: r.try_get("product_name").unwrap_or_default(),
            version_id: r.try_get("version_id").map_err(|e| SearchError {
                code: "INTERNAL",
                message: e.to_string(),
            })?,
            version_label: r.try_get("version_label").unwrap_or_default(),
            emb: r.try_get("emb").unwrap_or_default(),
        });
    }
    let mut seen: Option<String> = None;
    for v in &versions {
        match &seen {
            None => seen = Some(v.emb.clone()),
            Some(id) if id != &v.emb => {
                return Err(SearchError {
                    code: "EMBEDDING_MISMATCH",
                    message: "target versions use different embedding models".into(),
                });
            }
            _ => {}
        }
    }
    let lib_ids: Vec<Uuid> =
        if !scoped_lines && (req.include_library || requirements.iter().any(|r| r.use_library)) {
            sqlx::query_scalar(
                "SELECT p.current_version_id FROM products p
             JOIN product_versions pv ON pv.id = p.current_version_id
             WHERE p.workspace_id = $1 AND p.kind = 'library'
               AND pv.status = 'active' AND pv.deleted_at IS NULL",
            )
            .bind(workspace_id)
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        } else {
            Vec::new()
        };
    if !lib_ids.is_empty() {
        let libs = crate::embedding_models_for_versions(pool, &lib_ids)
            .await
            .map_err(|e| SearchError {
                code: "INTERNAL",
                message: e.to_string(),
            })?;
        for (_, m) in libs {
            match &seen {
                None => seen = Some(m),
                Some(id) if id != &m => {
                    return Err(SearchError {
                        code: "EMBEDDING_MISMATCH",
                        message: "target versions use different embedding models".into(),
                    });
                }
                _ => {}
            }
        }
    }
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();
    let mut by_product: std::collections::HashMap<Uuid, Vec<Ver>> =
        std::collections::HashMap::new();
    for v in versions {
        by_product.entry(v.product_id).or_default().push(v);
    }
    if !scoped_lines && by_product.len() > 50 {
        let mut keys: Vec<Uuid> = by_product.keys().copied().collect();
        keys.sort();
        keys.truncate(50);
        by_product.retain(|k, _| keys.contains(k));
    }
    for (pid, vers) in by_product {
        let mut best: Option<Candidate> = None;
        let pname = vers
            .first()
            .map(|v| v.product_name.clone())
            .unwrap_or_default();
        for v in &vers {
            let mut req_results = Vec::new();
            let mut weighted = 0.0;
            let mut wsum = 0.0;
            let mut hit_w = 0.0;
            let mut unmet = Vec::new();
            for r in &requirements {
                let mut hits =
                    match pg_version_hits(pool, v.version_id, &v.emb, &r.text, req, &r.tag_ids)
                        .await
                    {
                        Ok(h) => h,
                        Err(e) => {
                            warnings.push(e.message);
                            Vec::new()
                        }
                    };
                if req.include_library || r.use_library {
                    for lid in &lib_ids {
                        match pg_version_hits(pool, *lid, &v.emb, &r.text, req, &r.tag_ids).await {
                            Ok(mut extra) => hits.append(&mut extra),
                            Err(e) => warnings.push(e.message),
                        }
                    }
                }
                hits.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                hits.truncate(req.match_count.max(1));
                let score = hits.first().map(|h| h.score).unwrap_or(0.0);
                let hit = score > 0.0;
                if r.must && !hit {
                    unmet.push(r.id.clone());
                }
                weighted += r.weight * score;
                wsum += r.weight;
                if hit {
                    hit_w += r.weight;
                }
                req_results.push(ReqResult {
                    id: r.id.clone(),
                    hit,
                    score,
                    hits,
                });
            }
            let score = if wsum == 0.0 { 0.0 } else { weighted / wsum };
            let coverage = if wsum == 0.0 { 0.0 } else { hit_w / wsum };
            let cand = Candidate {
                product_id: pid,
                product_title: pname.clone(),
                matched_version_id: v.version_id,
                matched_version_label: v.version_label.clone(),
                score,
                coverage,
                unmet_must: unmet,
                requirements: req_results,
            };
            let replace = match &best {
                None => true,
                Some(b) => cand.score > b.score,
            };
            if replace {
                best = Some(cand);
            }
        }
        if let Some(c) = best {
            candidates.push(c);
        }
    }
    candidates.sort_by(|a, b| {
        let au = a.unmet_must.is_empty();
        let bu = b.unmet_must.is_empty();
        bu.cmp(&au).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    if candidates.is_empty() && !warnings.is_empty() {
        return Err(SearchError {
            code: "UPSTREAM",
            message: warnings.join("; "),
        });
    }
    Ok(MatchingResponse {
        candidates,
        clauses: Vec::new(),
        warnings,
    })
}

async fn check_product_line_thresholds_pg(pool: &sqlx::PgPool) -> Result<(), SearchError> {
    let rows = sqlx::query(
        "SELECT DISTINCT w.retrieval_config
         FROM workspaces w WHERE COALESCE(w.kind, 'product_line') = 'product_line'",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| SearchError {
        code: "INTERNAL",
        message: e.to_string(),
    })?;
    let mut seen: Option<(f64, f64)> = None;
    for r in rows {
        use sqlx::Row;
        let cfg: serde_json::Value = r.try_get("retrieval_config").unwrap_or_default();
        let v = cfg
            .get("vector_threshold")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.15);
        let k = cfg
            .get("keyword_threshold")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.3);
        match seen {
            None => seen = Some((v, k)),
            Some(prev) if prev != (v, k) => {
                return Err(SearchError {
                    code: "EMBEDDING_MISMATCH",
                    message: "product_line retrieval thresholds differ".into(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

async fn matching_company_pg(
    pool: &sqlx::PgPool,
    req: &SearchRequest,
) -> Result<MatchingResponse, SearchError> {
    let mut requirements = req.requirements.clone();
    if requirements.is_empty()
        && let Some(q) = &req.query
        && !q.trim().is_empty()
    {
        requirements.push(Requirement {
            id: "q0".into(),
            text: q.clone(),
            weight: 1.0,
            must: false,
            tag_ids: req.tag_ids.clone(),
            use_library: false,
        });
    }
    if requirements.is_empty() || requirements.len() > 30 {
        return Err(SearchError {
            code: "VALIDATION",
            message: "requirements must have 1–30 items".into(),
        });
    }
    let version_rows = sqlx::query(
        "SELECT p.id AS product_id, pv.id AS version_id,
                COALESCE(pv.embedding_model_id, '') AS emb
         FROM products p
         JOIN workspaces w ON w.id = p.workspace_id
         JOIN product_versions pv ON pv.id = p.current_version_id
         WHERE w.kind = 'company' AND p.kind = 'library'
           AND pv.status = 'active' AND pv.deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| SearchError {
        code: "INTERNAL",
        message: e.to_string(),
    })?;
    let mut versions = Vec::new();
    for r in version_rows {
        use sqlx::Row;
        versions.push((
            r.get::<Uuid, _>("product_id"),
            r.get::<Uuid, _>("version_id"),
            r.get::<String, _>("emb"),
        ));
    }
    let mut clauses = Vec::new();
    for r in &requirements {
        let mut all = Vec::new();
        for (pid, vid, emb) in &versions {
            let _ = pid;
            if let Ok(mut hits) = pg_version_hits(pool, *vid, emb, &r.text, req, &r.tag_ids).await {
                hits.retain(|h| h.chunk_type != "wiki_page" && h.match_type != "graph");
                all.append(&mut hits);
            }
        }
        all.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let best = all.first().cloned();
        let hit = best.as_ref().is_some_and(|h| h.score > 0.0);
        let alts = all
            .iter()
            .skip(1)
            .map(|h| ClauseAlt {
                document_id: h.document_id,
                file_name: h.document_title.clone(),
                score: h.score,
            })
            .collect();
        clauses.push(ClauseHit {
            id: r.id.clone(),
            outcome: if hit { "hit".into() } else { "miss".into() },
            document_id: best.as_ref().and_then(|h| hit.then_some(h.document_id)),
            version_id: best.as_ref().and_then(|h| hit.then_some(h.version_id)),
            file_name: best
                .as_ref()
                .and_then(|h| hit.then(|| h.document_title.clone())),
            score: best.as_ref().and_then(|h| hit.then_some(h.score)),
            product_id: best.as_ref().and_then(|h| hit.then_some(h.product_id)),
            hits: if hit {
                best.clone().into_iter().collect()
            } else {
                Vec::new()
            },
            alts,
        });
    }
    Ok(MatchingResponse {
        candidates: Vec::new(),
        clauses,
        warnings: Vec::new(),
    })
}

async fn pg_version_hits(
    pool: &sqlx::PgPool,
    version_id: Uuid,
    model_id: &str,
    query: &str,
    req: &SearchRequest,
    tag_ids: &[Uuid],
) -> Result<Vec<Hit>, SearchError> {
    let qv = embed_index(query, model_id).map_err(|e| SearchError {
        code: "UPSTREAM",
        message: e,
    })?;
    let qv = crate::vector_literal(&qv);
    let rows = crate::hybrid_search_pg(
        pool,
        version_id,
        query,
        &qv,
        tag_ids,
        req.expand_wiki,
        per_target_limit(req.match_count, 1) as i64,
    )
    .await
    .map_err(|e| SearchError {
        code: "INTERNAL",
        message: e.to_string(),
    })?;
    let (vth, kth) = crate::workspace_thresholds_for_version(pool, version_id)
        .await
        .unwrap_or((0.15, 0.3));
    let mut hits = hits_from_pg_rows(rows, vth, kth);
    if req.expand_graph {
        hits.extend(
            graph_hits_for_version(pool, version_id, query, req.match_count, kth, tag_ids).await?,
        );
    }
    Ok(fuse_by_chunk_id(hits))
}

fn workspace_summary_model(store: &Store, workspace_id: Uuid) -> String {
    store
        .products
        .values()
        .filter(|p| p.workspace_id == workspace_id && p.kind == ProductKind::Product)
        .find_map(|p| {
            p.current_version_id
                .and_then(|id| store.versions.get(&id).map(|v| v.summary_model_id.clone()))
        })
        .unwrap_or_else(|| "stub-chat".into())
}

fn product_versions(store: &Store, p: &crate::Product, scope: &str) -> Vec<Uuid> {
    if scope == "all_active" {
        store
            .versions
            .values()
            .filter(|v| v.product_id == p.id && v.status == VersionStatus::Active)
            .map(|v| v.id)
            .collect()
    } else if let Some(cid) = p.current_version_id {
        if store
            .versions
            .get(&cid)
            .is_some_and(|v| v.status == VersionStatus::Active)
        {
            vec![cid]
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn connect_test_pool() -> Result<sqlx::PgPool, sqlx::Error> {
        let database_url = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").map_err(|_| {
            sqlx::Error::Configuration(
                "KNOWLEDGEBRAIN_TEST_DATABASE_URL is required for destructive PostgreSQL tests"
                    .into(),
            )
        })?;
        if database_url.contains(":15432/") {
            return Err(sqlx::Error::Configuration(
                "destructive PostgreSQL tests refuse the live :15432 database".into(),
            ));
        }
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(16)
            .connect(&database_url)
            .await
    }

    async fn reset_fresh_schema(pool: &sqlx::PgPool) {
        for statement in [
            "DROP SCHEMA public CASCADE",
            "CREATE SCHEMA public",
            "GRANT ALL ON SCHEMA public TO CURRENT_USER",
        ] {
            sqlx::query(statement)
                .execute(pool)
                .await
                .expect("reset fresh test schema");
        }
        platform::apply_fresh_baseline(pool).await.expect("migrate");
    }

    #[test]
    fn over_fetch_caps_and_floor() {
        assert_eq!(over_fetch(10, 1), 50);
        assert_eq!(over_fetch(10, 2), 100);
        assert_eq!(over_fetch(10, 20), 500);
        assert!(per_target_limit(10, 2) >= 10);
    }

    #[test]
    fn assembly_current_empty_is_validation() {
        let mut s = Store::default();
        let ws = crate::Workspace {
            id: Uuid::new_v4(),
            name: "ws".into(),
            slug: "ws".into(),
            kind: Default::default(),
            retrieval: Default::default(),
        };
        let p = crate::Product {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            kind: ProductKind::Product,
            name: "p".into(),
            slug: "p".into(),
            current_version_id: None,
            embedding_model_id: "stub-emb".into(),
        };
        let pid = p.id;
        s.workspaces.insert(ws.id, ws);
        s.products.insert(pid, p);
        let err = assembly(
            &s,
            &SearchRequest {
                mode: "assembly".into(),
                query: Some("q".into()),
                product_id: Some(pid),
                version_id: Some("current".into()),
                include_library: false,
                tag_ids: vec![],
                match_count: 8,
                expand_wiki: true,
                expand_graph: true,
                requirements: vec![],
                version_scope: "current".into(),
                product_ids: vec![],
                workspace_id: None,
                scope: None,
                group_by: "none".into(),
                tender_text: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, "VALIDATION");
    }

    #[test]
    fn group_by_version_round_robins() {
        let mk = |id, vid, score| Hit {
            id,
            content: String::new(),
            score,
            match_type: "vector".into(),
            chunk_type: "text".into(),
            document_id: id,
            document_title: String::new(),
            product_id: Uuid::nil(),
            product_kind: "product".into(),
            version_id: vid,
            version_label: "v".into(),
            is_current: true,
            tag_ids: vec![],
            tag_slugs: vec![],
            start_at: 0,
            end_at: 0,
            image_object_ref: None,
        };
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let hits = vec![
            mk(Uuid::new_v4(), a, 0.9),
            mk(Uuid::new_v4(), a, 0.8),
            mk(Uuid::new_v4(), b, 0.7),
        ];
        let out = apply_group_by(hits, "version", 2);
        assert_eq!(out.len(), 2);
        assert_ne!(out[0].version_id, out[1].version_id);
    }

    #[test]
    fn fuse_keeps_higher_score_per_chunk() {
        let id = Uuid::new_v4();
        let mk = |score, kind: &str| Hit {
            id,
            content: kind.into(),
            score,
            match_type: kind.into(),
            chunk_type: "text".into(),
            document_id: id,
            document_title: String::new(),
            product_id: Uuid::nil(),
            product_kind: "product".into(),
            version_id: id,
            version_label: "v".into(),
            is_current: true,
            tag_ids: vec![],
            tag_slugs: vec![],
            start_at: 0,
            end_at: 0,
            image_object_ref: None,
        };
        let fused = fuse_by_chunk_id(vec![mk(0.2, "graph"), mk(0.8, "vector")]);
        assert_eq!(fused.len(), 1);
        assert!((fused[0].score - 0.8).abs() < 1e-9);
        assert_eq!(fused[0].match_type, "vector");
    }

    #[test]
    fn tender_text_splits_into_requirements() {
        let s = Store::default();
        let ws = Uuid::new_v4();
        let req = SearchRequest {
            mode: "matching".into(),
            query: None,
            product_id: None,
            version_id: None,
            include_library: false,
            tag_ids: vec![],
            match_count: 5,
            expand_wiki: true,
            expand_graph: true,
            requirements: vec![],
            version_scope: "current".into(),
            product_ids: vec![],
            workspace_id: Some(ws),
            scope: None,
            group_by: "none".into(),
            tender_text: Some(
                "Throughput must be 40Gbps.\nMust have ISO9001 certification.".into(),
            ),
        };
        let out = matching(&s, &req).unwrap();
        assert!(out.candidates.is_empty());
        let lines =
            split_tender_lines("Throughput must be 40Gbps.\nMust have ISO9001 certification.");
        assert!(lines.len() >= 2, "{lines:?}");
    }

    #[test]
    fn company_scope_flattens_library_docs() {
        let mut s = Store::default();
        let ws = crate::Workspace {
            id: Uuid::new_v4(),
            name: "co".into(),
            slug: "company".into(),
            kind: crate::WorkspaceKind::Company,
            retrieval: Default::default(),
        };
        let p = crate::Product {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            kind: ProductKind::Library,
            name: "iso".into(),
            slug: "iso".into(),
            current_version_id: None,
            embedding_model_id: "stub-emb".into(),
        };
        let mut v = crate::ProductVersion::new(p.id, "current".into());
        v.status = VersionStatus::Active;
        let mut d = crate::Document::new(
            v.id,
            "ISO".into(),
            "iso.txt".into(),
            3,
            "h".into(),
            "objects/h".into(),
        );
        d.enable_status = "enabled".into();
        d.markdown = "ISO9001 certificate".into();
        let ch = crate::Chunk {
            id: Uuid::new_v4(),
            document_id: d.id,
            product_version_id: v.id,
            chunk_type: "text".into(),
            content: "ISO9001 certificate".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 19,
            parent_chunk_id: None,
            generated_questions: vec![],
        };
        let _ = crate::index::index_one(&mut s, &ch, "ISO", true, true);
        let mut p = p;
        p.current_version_id = Some(v.id);
        s.workspaces.insert(ws.id, ws);
        s.products.insert(p.id, p);
        s.versions.insert(v.id, v);
        s.documents.insert(d.id, d);
        s.chunks.insert(ch.id, ch);
        let out = matching(
            &s,
            &SearchRequest {
                mode: "matching".into(),
                query: None,
                product_id: None,
                version_id: None,
                include_library: false,
                tag_ids: vec![],
                match_count: 5,
                expand_wiki: false,
                expand_graph: false,
                requirements: vec![Requirement {
                    id: "c1".into(),
                    text: "ISO9001".into(),
                    weight: 1.0,
                    must: true,
                    tag_ids: vec![],
                    use_library: false,
                }],
                version_scope: "current".into(),
                product_ids: vec![],
                workspace_id: None,
                scope: Some("company".into()),
                group_by: "none".into(),
                tender_text: None,
            },
        )
        .unwrap();
        assert!(out.candidates.is_empty());
        assert_eq!(out.clauses.len(), 1);
        assert_eq!(out.clauses[0].id, "c1");
    }

    #[test]
    fn matching_rejects_product_id() {
        let s = Store::default();
        let req = SearchRequest {
            mode: "matching".into(),
            query: Some("x".into()),
            product_id: Some(Uuid::new_v4()),
            version_id: None,
            include_library: false,
            tag_ids: vec![],
            match_count: 5,
            expand_wiki: true,
            expand_graph: true,
            requirements: vec![],
            version_scope: "current".into(),
            product_ids: vec![],
            workspace_id: Some(Uuid::new_v4()),
            scope: None,
            group_by: "none".into(),
            tender_text: None,
        };
        let err = matching(&s, &req).unwrap_err();
        assert_eq!(err.code, "VALIDATION");
    }

    #[tokio::test]
    async fn matching_pg_returns_candidates_without_best_product() {
        let _g = {
            static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
            LOCK.lock().await
        };
        let Ok(pool) = connect_test_pool().await else {
            eprintln!("skip: isolated postgres test database not configured");
            return;
        };
        reset_fresh_schema(&pool).await;
        let owner = Uuid::new_v4();
        crate::insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = crate::create_workspace_with_library(&pool, owner, "Mt", "mt")
            .await
            .unwrap();
        let pid = Uuid::new_v4();
        let vid = Uuid::new_v4();
        crate::insert_product(
            &pool,
            pid,
            seeded.workspace_id,
            "product",
            "Switch",
            "switch",
            Some(vid),
        )
        .await
        .unwrap();
        crate::insert_version(&pool, vid, pid, "v1", "active", None)
            .await
            .unwrap();
        let did = Uuid::new_v4();
        crate::insert_document(
            &pool,
            crate::NewDocument {
                id: did,
                product_version_id: vid,
                title: "ds",
                file_name: "ds.txt",
                file_size: 20,
                file_hash: "121458e7133ee31878b0530193f304c0bf600aeff873aa89fd37250d8fa5d1ae",
                object_ref: "objects/121458e7133ee31878b0530193f304c0bf600aeff873aa89fd37250d8fa5d1ae",
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE documents SET enable_status = 'enabled' WHERE id = $1")
            .bind(did)
            .execute(&pool)
            .await
            .unwrap();
        let cid = Uuid::new_v4();
        let text = "40Gbps throughput line card";
        crate::replace_document_chunks(
            &pool,
            did,
            &[crate::Chunk {
                id: cid,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: text.into(),
                context_header: String::new(),
                start_at: 0,
                end_at: text.len() as i32,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[crate::ChunkEmbedding {
                chunk_id: cid,
                product_version_id: vid,
                document_id: did,
                content: text.into(),
                vector: crate::index::stub_embed(text),
                tsv: String::new(),
            }],
        )
        .await
        .unwrap();
        let req = SearchRequest {
            mode: "matching".into(),
            query: None,
            product_id: None,
            version_id: None,
            include_library: false,
            tag_ids: vec![],
            match_count: 5,
            expand_wiki: true,
            expand_graph: true,
            requirements: vec![Requirement {
                id: "r1".into(),
                text: "throughput".into(),
                weight: 1.0,
                must: true,
                tag_ids: vec![],
                use_library: false,
            }],
            version_scope: "current".into(),
            product_ids: vec![],
            workspace_id: Some(seeded.workspace_id),
            scope: None,
            group_by: "none".into(),
            tender_text: None,
        };
        let empty = Store::default();
        let mem = matching(&empty, &req).unwrap();
        assert!(mem.candidates.is_empty());
        let pg = matching_pg(&pool, &req).await.unwrap();
        assert_eq!(pg.candidates.len(), 1);
        assert_eq!(pg.candidates[0].product_id, pid);
        assert!(pg.candidates[0].requirements[0].hit);
        assert!(!pg.candidates[0].requirements[0].hits.is_empty());
        let v = serde_json::to_value(&pg).unwrap();
        assert!(v.get("best_product_id").is_none());
        reset_fresh_schema(&pool).await;
    }
}
