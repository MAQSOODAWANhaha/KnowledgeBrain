//! Processing spans. Brain-aligned five-stage DAG. No Langfuse / Prometheus.

use crate::{Span, Store};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const KIND_ROOT: &str = "root";
pub const KIND_STAGE: &str = "stage";
pub const KIND_SUBSPAN: &str = "subspan";

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_SKIPPED: &str = "skipped";
pub const STATUS_CANCELLED: &str = "cancelled";

pub const ROOT_NAME: &str = "document_processing";
pub const SPAN_DOCREADER: &str = "docreader";
pub const SPAN_CHUNKING: &str = "chunking";
pub const SPAN_EMBEDDING: &str = "embedding";
pub const SPAN_MULTIMODAL: &str = "multimodal";
pub const SPAN_POSTPROCESS: &str = "postprocess";

pub const ALL_STAGES: [&str; 5] = [
    SPAN_DOCREADER,
    SPAN_CHUNKING,
    SPAN_EMBEDDING,
    SPAN_MULTIMODAL,
    SPAN_POSTPROCESS,
];

pub fn kind_of(name: &str) -> &'static str {
    if name == ROOT_NAME {
        KIND_ROOT
    } else if ALL_STAGES.contains(&name) {
        KIND_STAGE
    } else {
        KIND_SUBSPAN
    }
}

pub fn normalize_status(status: &str) -> &'static str {
    match status {
        "ok" | STATUS_DONE => STATUS_DONE,
        "error" | STATUS_FAILED => STATUS_FAILED,
        STATUS_RUNNING => STATUS_RUNNING,
        STATUS_PENDING => STATUS_PENDING,
        STATUS_SKIPPED => STATUS_SKIPPED,
        STATUS_CANCELLED => STATUS_CANCELLED,
        _ => STATUS_DONE,
    }
}

pub fn stage_satisfied(rows: &[Span], name: &str) -> bool {
    rows.iter()
        .any(|s| s.name == name && (s.status == STATUS_DONE || s.status == STATUS_SKIPPED))
}

/// Whether `stage` may start given already-recorded rows. Embedding and
/// multimodal only need chunking; postprocess joins both siblings.
/// True when this document has no stage tracking yet (legacy / unit tests)
/// or when the DAG join for `stage` is satisfied.
pub fn can_start_stage_or_legacy(stage: &str, rows: &[Span]) -> bool {
    let tracked = rows.iter().any(|s| {
        s.kind == KIND_ROOT || s.kind == KIND_STAGE || ALL_STAGES.contains(&s.name.as_str())
    });
    if !tracked {
        return true;
    }
    can_start_stage(stage, rows)
}

pub fn can_start_stage(stage: &str, rows: &[Span]) -> bool {
    match stage {
        SPAN_DOCREADER => true,
        SPAN_CHUNKING => stage_satisfied(rows, SPAN_DOCREADER),
        SPAN_EMBEDDING | SPAN_MULTIMODAL => stage_satisfied(rows, SPAN_CHUNKING),
        SPAN_POSTPROCESS => {
            stage_satisfied(rows, SPAN_EMBEDDING) && stage_satisfied(rows, SPAN_MULTIMODAL)
        }
        _ => true,
    }
}

/// Stages that cannot run after `stage` fails (siblings, not tree children).
pub fn dependents_of(stage: &str) -> &'static [&'static str] {
    match stage {
        SPAN_DOCREADER => &[
            SPAN_CHUNKING,
            SPAN_EMBEDDING,
            SPAN_MULTIMODAL,
            SPAN_POSTPROCESS,
        ],
        SPAN_CHUNKING => &[SPAN_EMBEDDING, SPAN_MULTIMODAL, SPAN_POSTPROCESS],
        SPAN_EMBEDDING | SPAN_MULTIMODAL => &[SPAN_POSTPROCESS],
        _ => &[],
    }
}

pub fn start(store: &mut Store, document_id: Uuid, attempt: i32, name: &str, parent: Option<&str>) {
    if let Some(existing) = store
        .spans
        .iter_mut()
        .rev()
        .find(|s| s.document_id == document_id && s.attempt == attempt && s.name == name)
    {
        existing.status = STATUS_RUNNING.into();
        existing.started_at = Utc::now();
        existing.finished_at = None;
        existing.duration_ms = None;
        existing.error_message.clear();
        existing.output = None;
        return;
    }
    let parent_span_id = parent.and_then(|p| {
        store
            .spans
            .iter()
            .rev()
            .find(|s| s.document_id == document_id && s.attempt == attempt && s.name == p)
            .map(|s| s.span_id)
    });
    store.spans.push(Span {
        span_id: Uuid::new_v4(),
        document_id,
        attempt,
        name: name.to_string(),
        parent_span_id,
        kind: kind_of(name).to_string(),
        status: STATUS_RUNNING.into(),
        output: None,
        error_message: String::new(),
        started_at: Utc::now(),
        finished_at: None,
        duration_ms: None,
    });
}

pub fn finish(store: &mut Store, document_id: Uuid, name: &str, status: &str) {
    let status = normalize_status(status);
    if let Some(s) = store
        .spans
        .iter_mut()
        .rev()
        .find(|s| s.document_id == document_id && s.name == name && s.finished_at.is_none())
    {
        s.status = status.to_string();
        s.finished_at = Some(Utc::now());
        s.duration_ms = Some((s.finished_at.unwrap() - s.started_at).num_milliseconds());
    }
}

pub fn skip(store: &mut Store, document_id: Uuid, attempt: i32, name: &str, reason: &str) {
    start(store, document_id, attempt, name, Some(ROOT_NAME));
    if let Some(s) = store
        .spans
        .iter_mut()
        .rev()
        .find(|s| s.document_id == document_id && s.name == name)
    {
        s.status = STATUS_SKIPPED.into();
        s.error_message = reason.to_string();
        s.finished_at = Some(Utc::now());
        s.duration_ms = Some(0);
    }
}

pub fn cascade_cancel(store: &mut Store, document_id: Uuid, attempt: i32, failed_stage: &str) {
    let deps = dependents_of(failed_stage);
    for s in store.spans.iter_mut().filter(|s| {
        s.document_id == document_id
            && s.attempt == attempt
            && deps.contains(&s.name.as_str())
            && (s.status == STATUS_PENDING || s.status == STATUS_RUNNING)
    }) {
        s.status = STATUS_CANCELLED.into();
        s.error_message = format!("upstream {failed_stage} failed");
        s.finished_at = Some(Utc::now());
    }
}

pub fn current_step(spans: &[Span]) -> Option<&Span> {
    spans
        .iter()
        .find(|s| s.kind == KIND_STAGE && s.status == STATUS_RUNNING)
        .or_else(|| spans.iter().rev().find(|s| s.kind == KIND_STAGE))
}

pub fn timeline(store: &Store, document_id: Uuid) -> Vec<Span> {
    store
        .spans
        .iter()
        .filter(|s| s.document_id == document_id)
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceNode {
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub attempt: i32,
    pub output: Option<serde_json::Value>,
    pub error_message: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
    pub children: Vec<TraceNode>,
}

pub fn build_trace(
    attempt: i32,
    parse_status: &str,
    rows: &[Span],
) -> (TraceNode, String, Option<Span>) {
    let synthetic = match parse_status {
        "completed" => STATUS_DONE,
        "failed" => STATUS_FAILED,
        "cancelled" => STATUS_CANCELLED,
        _ => STATUS_PENDING,
    };
    let mut current_stage = String::new();
    let mut last_fail: Option<Span> = None;
    let mut root_span: Option<Span> = None;
    for r in rows {
        if r.kind == KIND_ROOT && root_span.is_none() {
            root_span = Some(r.clone());
        }
        if r.kind == KIND_STAGE && r.status == STATUS_RUNNING && current_stage.is_empty() {
            current_stage = r.name.clone();
        }
        if r.status == STATUS_FAILED {
            last_fail = Some(r.clone());
        }
    }
    let mut root = match root_span {
        Some(r) => node_from(&r),
        None => TraceNode {
            span_id: None,
            parent_span_id: None,
            name: ROOT_NAME.into(),
            kind: KIND_ROOT.into(),
            status: synthetic.into(),
            attempt,
            output: None,
            error_message: String::new(),
            started_at: None,
            finished_at: None,
            duration_ms: None,
            children: Vec::new(),
        },
    };
    let root_id = root.span_id;
    let mut seen_stages = std::collections::HashSet::new();
    for r in rows {
        if root_id.is_some() && r.span_id == root.span_id.unwrap_or(Uuid::nil()) {
            continue;
        }
        if r.kind == KIND_STAGE {
            seen_stages.insert(r.name.clone());
        }
        let child = node_from(r);
        if r.parent_span_id.is_some() && r.parent_span_id == root_id {
            root.children.push(child);
        } else if let Some(pid) = r.parent_span_id {
            if !attach_child(&mut root, pid, child) {
                root.children.push(node_from(r));
            }
        } else if r.kind != KIND_ROOT {
            root.children.push(child);
        }
    }
    for name in ALL_STAGES {
        if seen_stages.contains(name) {
            continue;
        }
        root.children.push(TraceNode {
            span_id: None,
            parent_span_id: root_id,
            name: (*name).into(),
            kind: KIND_STAGE.into(),
            status: synthetic.into(),
            attempt,
            output: None,
            error_message: String::new(),
            started_at: None,
            finished_at: None,
            duration_ms: None,
            children: Vec::new(),
        });
    }
    (root, current_stage, last_fail)
}

fn node_from(r: &Span) -> TraceNode {
    TraceNode {
        span_id: Some(r.span_id),
        parent_span_id: r.parent_span_id,
        name: r.name.clone(),
        kind: r.kind.clone(),
        status: r.status.clone(),
        attempt: r.attempt,
        output: r.output.clone(),
        error_message: r.error_message.clone(),
        started_at: Some(r.started_at),
        finished_at: r.finished_at,
        duration_ms: r.duration_ms,
        children: Vec::new(),
    }
}

fn attach_child(node: &mut TraceNode, parent_id: Uuid, child: TraceNode) -> bool {
    if node.span_id == Some(parent_id) {
        node.children.push(child);
        return true;
    }
    for c in &mut node.children {
        if attach_child(c, parent_id, child.clone()) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_step_prefers_running() {
        let did = Uuid::new_v4();
        let mut store = Store::default();
        start(&mut store, did, 1, ROOT_NAME, None);
        start(&mut store, did, 1, SPAN_DOCREADER, Some(ROOT_NAME));
        finish(&mut store, did, SPAN_DOCREADER, "ok");
        start(&mut store, did, 1, SPAN_CHUNKING, Some(ROOT_NAME));
        let steps = timeline(&store, did);
        let cur = current_step(&steps).unwrap();
        assert_eq!(cur.name, SPAN_CHUNKING);
        assert_eq!(cur.status, STATUS_RUNNING);
    }

    #[test]
    fn build_trace_always_has_five_stages() {
        let (tree, current, fail) = build_trace(1, "processing", &[]);
        assert_eq!(tree.name, ROOT_NAME);
        assert_eq!(tree.children.len(), 5);
        assert!(current.is_empty());
        assert!(fail.is_none());
        assert!(tree.children.iter().all(|c| c.status == STATUS_PENDING));
    }

    #[test]
    fn build_trace_completed_without_rows_is_done() {
        let (tree, _, _) = build_trace(1, "completed", &[]);
        assert_eq!(tree.status, STATUS_DONE);
        assert!(tree.children.iter().all(|c| c.status == STATUS_DONE));
    }

    #[test]
    fn dependents_match_brain_dag() {
        assert!(dependents_of(SPAN_CHUNKING).contains(&SPAN_EMBEDDING));
        assert!(dependents_of(SPAN_CHUNKING).contains(&SPAN_MULTIMODAL));
        assert!(!dependents_of(SPAN_EMBEDDING).contains(&SPAN_MULTIMODAL));
        assert!(dependents_of(SPAN_EMBEDDING).contains(&SPAN_POSTPROCESS));
    }

    #[test]
    fn postprocess_waits_for_both_siblings() {
        let did = Uuid::new_v4();
        let mut store = Store::default();
        start(&mut store, did, 1, ROOT_NAME, None);
        start(&mut store, did, 1, SPAN_DOCREADER, Some(ROOT_NAME));
        finish(&mut store, did, SPAN_DOCREADER, STATUS_DONE);
        start(&mut store, did, 1, SPAN_CHUNKING, Some(ROOT_NAME));
        finish(&mut store, did, SPAN_CHUNKING, STATUS_DONE);
        start(&mut store, did, 1, SPAN_EMBEDDING, Some(ROOT_NAME));
        finish(&mut store, did, SPAN_EMBEDDING, STATUS_DONE);
        start(&mut store, did, 1, SPAN_MULTIMODAL, Some(ROOT_NAME));
        let rows = timeline(&store, did);
        assert!(!can_start_stage(SPAN_POSTPROCESS, &rows));
        finish(&mut store, did, SPAN_MULTIMODAL, STATUS_DONE);
        let rows = timeline(&store, did);
        assert!(can_start_stage(SPAN_POSTPROCESS, &rows));
    }

    #[test]
    fn skipped_multimodal_unblocks_postprocess() {
        let did = Uuid::new_v4();
        let mut store = Store::default();
        start(&mut store, did, 1, ROOT_NAME, None);
        start(&mut store, did, 1, SPAN_DOCREADER, Some(ROOT_NAME));
        finish(&mut store, did, SPAN_DOCREADER, STATUS_DONE);
        start(&mut store, did, 1, SPAN_CHUNKING, Some(ROOT_NAME));
        finish(&mut store, did, SPAN_CHUNKING, STATUS_DONE);
        start(&mut store, did, 1, SPAN_EMBEDDING, Some(ROOT_NAME));
        finish(&mut store, did, SPAN_EMBEDDING, STATUS_DONE);
        skip(&mut store, did, 1, SPAN_MULTIMODAL, "no images");
        let rows = timeline(&store, did);
        assert!(can_start_stage(SPAN_POSTPROCESS, &rows));
    }
}
