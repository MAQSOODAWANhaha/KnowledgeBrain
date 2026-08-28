//! chunk:extract — no NEO4J_ENABLE gate. Namespace is (version, document).
//! Runtime follows brain `extract.go` + Formater, not token-split.

mod neo4j;
mod parse;
mod prompt;

pub use neo4j::{
    NeoNode, configured as neo4j_configured, delete_document, search_names, sync_document,
};

pub use parse::{parse_graph, stub_extract_json};
pub use prompt::{
    DEFAULT_RELATION_TAGS, ENTITY_PROMPT, EXTRACT_GRAPH_PROMPT, RELATION_PROMPT,
    render_extract_messages, render_system_prompt,
};

use crate::Store;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractOutcome {
    Done,
    Superseded,
}

pub fn extract_chunk(store: &mut Store, chunk_id: Uuid, document_id: Uuid) -> Result<(), String> {
    extract_chunk_for_attempt(store, chunk_id, document_id, 0).map(|_| ())
}

pub fn extract_chunk_for_attempt(
    store: &mut Store,
    chunk_id: Uuid,
    document_id: Uuid,
    job_attempt: i32,
) -> Result<ExtractOutcome, String> {
    let Some(doc) = store.documents.get(&document_id).cloned() else {
        return Ok(ExtractOutcome::Done);
    };
    if crate::enrichment::attempt_superseded(doc.attempt, job_attempt) {
        return Ok(ExtractOutcome::Superseded);
    }
    if doc.parse_status.is_aborted() {
        store.finalize_subtask(document_id);
        return Ok(ExtractOutcome::Done);
    }
    let Some(version) = store.effective_version(document_id) else {
        store.finalize_subtask(document_id);
        return Ok(ExtractOutcome::Done);
    };
    if !version.extract_enabled {
        store.finalize_subtask(document_id);
        return Ok(ExtractOutcome::Done);
    }
    let Some(ch) = store.chunks.get(&chunk_id).cloned() else {
        store.finalize_subtask(document_id);
        return Ok(ExtractOutcome::Done);
    };

    let tags = prompt::DEFAULT_RELATION_TAGS;
    let (system, user) = prompt::render_extract_messages(
        &ch.content,
        tags,
        &version.extract_custom_instructions,
        prompt::DEFAULT_EXAMPLE_TEXT,
        &prompt::default_example_nodes(),
        &prompt::default_example_rels(),
    );
    let raw = crate::enrichment::chat_complete(&system, &user, &version.summary_model_id)?;
    let graph = match parse::parse_graph(&raw) {
        Ok(g) => g,
        Err(_) if is_stub_chat(&version.summary_model_id) => {
            parse::parse_graph(&parse::stub_extract_json(&ch.content)).unwrap_or_default()
        }
        Err(e) => return Err(e),
    };

    if !store.chunks.contains_key(&chunk_id) {
        store.finalize_subtask(document_id);
        return Ok(ExtractOutcome::Done);
    }

    for n in &graph.nodes {
        if n.name.trim().is_empty() {
            continue;
        }
        store.upsert_node(ch.product_version_id, ch.document_id, &n.name, ch.id);
    }
    for r in &graph.relations {
        if r.node1.trim().is_empty() || r.node2.trim().is_empty() {
            continue;
        }
        store.upsert_rel(
            ch.product_version_id,
            ch.document_id,
            &r.node1,
            &r.node2,
            &r.rel_type,
        );
    }
    store.finalize_subtask(document_id);
    Ok(ExtractOutcome::Done)
}

fn is_stub_chat(model_id: &str) -> bool {
    crate::chat_base_url().is_empty() && (model_id == "stub-chat" || model_id.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Chunk, Document, ProductVersion};

    fn text_chunk(did: Uuid, vid: Uuid, content: &str) -> Chunk {
        Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: content.into(),
            context_header: String::new(),
            start_at: 0,
            end_at: content.chars().count() as i32,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        }
    }

    #[test]
    fn upsert_unions_chunk_ids() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let doc = Document::new(vid, "t".into(), "a.txt".into(), 1, "h".into(), "h".into());
        let did = doc.id;
        s.documents.insert(did, doc);
        let c1 = text_chunk(did, vid, "Alpha device throughput");
        let c2 = text_chunk(did, vid, "Alpha switch fabric");
        s.chunks.insert(c1.id, c1.clone());
        s.chunks.insert(c2.id, c2.clone());
        extract_chunk(&mut s, c1.id, did).unwrap();
        extract_chunk(&mut s, c2.id, did).unwrap();
        let node = s
            .graph
            .values()
            .find(|n| n.name == "Alpha")
            .expect("Alpha node");
        assert!(node.chunk_ids.len() >= 2);
        assert!(
            s.relations.values().any(|r| r.rel_type == "RELATES_TO"),
            "stub path should emit a relation"
        );
    }

    #[test]
    fn superseded_extract_does_not_finalize() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "t".into(), "a.txt".into(), 1, "h".into(), "h".into());
        doc.attempt = 2;
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        let c = text_chunk(did, vid, "Alpha device throughput");
        s.chunks.insert(c.id, c.clone());
        let out = extract_chunk_for_attempt(&mut s, c.id, did, 1).unwrap();
        assert_eq!(out, ExtractOutcome::Superseded);
        assert_eq!(s.documents[&did].pending_subtasks_count, 1);
        assert!(s.graph.is_empty());
    }

    #[test]
    fn extract_disabled_finalizes_without_nodes() {
        let mut s = Store::default();
        let mut v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        v.extract_enabled = false;
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "t".into(), "a.txt".into(), 1, "h".into(), "h".into());
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        let c = text_chunk(did, vid, "Alpha device throughput");
        s.chunks.insert(c.id, c.clone());
        extract_chunk(&mut s, c.id, did).unwrap();
        assert_eq!(s.documents[&did].pending_subtasks_count, 0);
        assert!(s.graph.is_empty());
    }

    #[test]
    fn extract_uses_effective_version_overrides() {
        let mut s = Store::default();
        let mut v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        v.extract_enabled = true;
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "t".into(), "a.txt".into(), 1, "h".into(), "h".into());
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        doc.process_overrides = Some(crate::ProcessOverrides {
            extract_config: Some(crate::ExtractOverride {
                enabled: false,
                text: String::new(),
            }),
            ..Default::default()
        });
        let did = doc.id;
        s.documents.insert(did, doc);
        let c = text_chunk(did, vid, "Alpha device throughput");
        s.chunks.insert(c.id, c.clone());
        extract_chunk(&mut s, c.id, did).unwrap();
        assert_eq!(s.documents[&did].pending_subtasks_count, 0);
        assert!(s.graph.is_empty());
    }

    #[test]
    fn extract_prompt_is_brain_extract_graph() {
        let (system, user) = render_extract_messages(
            "chunk body",
            DEFAULT_RELATION_TAGS,
            "",
            prompt::DEFAULT_EXAMPLE_TEXT,
            &prompt::default_example_nodes(),
            &prompt::default_example_rels(),
        );
        assert!(system.contains("Allowed relationship types are:"));
        assert!(system.contains("Author"));
        assert!(system.contains("William Shakespeare"));
        assert!(system.contains("# Examples"));
        assert!(user.contains("# Question"));
        assert!(user.contains("chunk body"));
        assert!(ENTITY_PROMPT.contains("EntityTypes"));
        assert!(RELATION_PROMPT.contains("relationship network"));
    }
}
