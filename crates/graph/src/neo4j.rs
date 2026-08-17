//! Optional Neo4j projection. Extract always writes Postgres; this is extra.

use domain::Store;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn configured() -> bool {
    !http_url().is_empty()
}

pub fn sync_document(store: &Store, document_id: Uuid) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    let Some(doc) = store.documents.get(&document_id) else {
        return Ok(());
    };
    delete_document(doc.product_version_id, document_id)?;
    let mut statements = Vec::new();
    for n in store
        .graph
        .values()
        .filter(|n| n.document_id == document_id)
    {
        let ids: Vec<String> = n.chunk_ids.iter().map(|id| id.to_string()).collect();
        statements.push(json!({
            "statement":
                "MERGE (e:KbEntity {key: $key}) \
                 SET e.name = $name, e.version_id = $vid, e.document_id = $did, \
                     e.chunk_ids = $ids",
            "parameters": {
                "key": entity_key(n.version_id, n.document_id, &n.name),
                "name": n.name,
                "vid": n.version_id.to_string(),
                "did": n.document_id.to_string(),
                "ids": ids,
            }
        }));
    }
    for r in store
        .relations
        .values()
        .filter(|r| r.document_id == document_id)
    {
        statements.push(json!({
            "statement":
                "MATCH (a:KbEntity {key: $a}), (b:KbEntity {key: $b}) \
                 MERGE (a)-[rel:KB_REL {rel_type: $rel}]->(b) \
                 SET rel.version_id = $vid, rel.document_id = $did",
            "parameters": {
                "a": entity_key(r.version_id, r.document_id, &r.node1),
                "b": entity_key(r.version_id, r.document_id, &r.node2),
                "rel": r.rel_type,
                "vid": r.version_id.to_string(),
                "did": r.document_id.to_string(),
            }
        }));
    }
    if statements.is_empty() {
        return Ok(());
    }
    cypher(&statements)?;
    Ok(())
}

pub fn delete_document(version_id: Uuid, document_id: Uuid) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    cypher(&[json!({
        "statement":
            "MATCH (e:KbEntity {version_id: $vid, document_id: $did}) DETACH DELETE e",
        "parameters": {
            "vid": version_id.to_string(),
            "did": document_id.to_string(),
        }
    })])?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct NeoNode {
    pub name: String,
    pub document_id: Uuid,
    pub chunk_ids: Vec<Uuid>,
}

pub fn search_names(version_id: Uuid, query: &str) -> Result<Vec<NeoNode>, String> {
    if !configured() {
        return Ok(Vec::new());
    }
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let body = cypher(&[json!({
        "statement":
            "MATCH (e:KbEntity {version_id: $vid}) \
             WHERE toLower(e.name) CONTAINS toLower($q) \
                OR toLower($q) CONTAINS toLower(e.name) \
             RETURN e.name AS name, e.document_id AS document_id, e.chunk_ids AS chunk_ids \
             LIMIT 50",
        "parameters": {
            "vid": version_id.to_string(),
            "q": q,
        }
    })])?;
    let mut out = Vec::new();
    let rows = body["results"][0]["data"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for row in rows {
        let cols = row["row"].as_array().cloned().unwrap_or_default();
        if cols.len() < 3 {
            continue;
        }
        let name = cols[0].as_str().unwrap_or("").to_string();
        let did = cols[1]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or(Uuid::nil());
        let chunk_ids = cols[2]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                    .collect()
            })
            .unwrap_or_default();
        if !name.is_empty() {
            out.push(NeoNode {
                name,
                document_id: did,
                chunk_ids,
            });
        }
    }
    Ok(out)
}

fn entity_key(version_id: Uuid, document_id: Uuid, name: &str) -> String {
    format!("{version_id}:{document_id}:{name}")
}

fn http_url() -> String {
    let v = std::env::var("KNOWLEDGEBRAIN_NEO4J_HTTP_URL").unwrap_or_default();
    if !v.is_empty() {
        return v;
    }
    std::env::var("NEO4J_HTTP_URL").unwrap_or_default()
}

fn username() -> String {
    std::env::var("KNOWLEDGEBRAIN_NEO4J_USERNAME")
        .or_else(|_| std::env::var("NEO4J_USERNAME"))
        .unwrap_or_else(|_| "neo4j".into())
}

fn password() -> String {
    std::env::var("KNOWLEDGEBRAIN_NEO4J_PASSWORD")
        .or_else(|_| std::env::var("NEO4J_PASSWORD"))
        .unwrap_or_default()
}

fn cypher(statements: &[Value]) -> Result<Value, String> {
    let base = http_url().trim_end_matches('/').to_string();
    let url = if base.ends_with("/db/neo4j/tx/commit") {
        base
    } else {
        format!("{base}/db/neo4j/tx/commit")
    };
    let user = username();
    let pass = password();
    let body = json!({ "statements": statements });
    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .post(url)
        .basic_auth(user, Some(pass))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("neo4j http {}", resp.status()));
    }
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    if let Some(errs) = v["errors"].as_array()
        && let Some(first) = errs.first()
    {
        let msg = first["message"].as_str().unwrap_or("neo4j error");
        return Err(msg.to_string());
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_is_noop() {
        if configured() {
            return;
        }
        assert!(sync_document(&Store::default(), Uuid::new_v4()).is_ok());
        assert!(delete_document(Uuid::new_v4(), Uuid::new_v4()).is_ok());
        assert!(search_names(Uuid::new_v4(), "Widget").unwrap().is_empty());
    }

    #[test]
    fn live_upsert_and_search() {
        if !configured() {
            eprintln!("skip: neo4j not configured");
            return;
        }
        let mut store = Store::default();
        let vid = Uuid::new_v4();
        let did = Uuid::new_v4();
        let cid = Uuid::new_v4();
        store.documents.insert(
            did,
            domain::Document::new(vid, "t".into(), "t.txt".into(), 1, "h".into(), "k".into()),
        );
        store.upsert_node(vid, did, "Widget", cid);
        store.upsert_rel(vid, did, "Widget", "Spec", "mentions");
        sync_document(&store, did).expect("neo4j sync");
        let found = search_names(vid, "widget").expect("neo4j search");
        assert!(
            found
                .iter()
                .any(|n| n.name == "Widget" && n.document_id == did),
            "{found:?}"
        );
        delete_document(vid, did).expect("neo4j delete");
        let gone = search_names(vid, "widget").expect("neo4j search after delete");
        assert!(gone.iter().all(|n| n.document_id != did), "{gone:?}");
    }
}
