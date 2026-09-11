//! Optional Neo4j projection. Extract always writes Postgres; this is extra.

use crate::Store;
use platform::DeploymentNamespaceV1;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn configured() -> bool {
    !http_url().is_empty()
}

pub fn sync_document(store: &Store, document_id: Uuid) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    sync_document_in_namespace(
        store,
        document_id,
        DeploymentNamespaceV1::from_environment().map_err(|error| error.to_string())?,
    )
}

fn sync_document_in_namespace(
    store: &Store,
    document_id: Uuid,
    namespace: DeploymentNamespaceV1,
) -> Result<(), String> {
    let Some(doc) = store.documents.get(&document_id) else {
        return Ok(());
    };
    delete_document_in_namespace(doc.product_version_id, document_id, namespace)?;
    let mut statements = Vec::new();
    for n in store
        .graph
        .values()
        .filter(|n| n.document_id == document_id)
    {
        let ids: Vec<String> = n.chunk_ids.iter().map(|id| id.to_string()).collect();
        statements.push(json!({
            "statement":
                "MERGE (e:KbEntity {deployment_namespace_id: $namespace, key: $key}) \
                 SET e.name = $name, e.version_id = $vid, e.document_id = $did, \
                     e.chunk_ids = $ids",
            "parameters": {
                "namespace": namespace,
                "key": entity_key(namespace, n.version_id, n.document_id, &n.name),
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
                "MATCH (a:KbEntity {deployment_namespace_id: $namespace, key: $a}), \
                       (b:KbEntity {deployment_namespace_id: $namespace, key: $b}) \
                 MERGE (a)-[rel:KB_REL {deployment_namespace_id: $namespace, rel_type: $rel}]->(b) \
                 SET rel.version_id = $vid, rel.document_id = $did",
            "parameters": {
                "namespace": namespace,
                "a": entity_key(namespace, r.version_id, r.document_id, &r.node1),
                "b": entity_key(namespace, r.version_id, r.document_id, &r.node2),
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
    delete_document_in_namespace(
        version_id,
        document_id,
        DeploymentNamespaceV1::from_environment().map_err(|error| error.to_string())?,
    )
}

fn delete_document_in_namespace(
    version_id: Uuid,
    document_id: Uuid,
    namespace: DeploymentNamespaceV1,
) -> Result<(), String> {
    cypher(&[json!({
        "statement":
            "MATCH (e:KbEntity {deployment_namespace_id: $namespace, version_id: $vid, document_id: $did}) DETACH DELETE e",
        "parameters": {
            "namespace": namespace,
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
    search_names_in_namespace(
        version_id,
        query,
        DeploymentNamespaceV1::from_environment().map_err(|error| error.to_string())?,
    )
}

fn search_names_in_namespace(
    version_id: Uuid,
    query: &str,
    namespace: DeploymentNamespaceV1,
) -> Result<Vec<NeoNode>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let body = cypher(&[json!({
        "statement":
            "MATCH (e:KbEntity {deployment_namespace_id: $namespace, version_id: $vid}) \
             WHERE toLower(e.name) CONTAINS toLower($q) \
                OR toLower($q) CONTAINS toLower(e.name) \
             RETURN e.name AS name, e.document_id AS document_id, e.chunk_ids AS chunk_ids \
             LIMIT 50",
        "parameters": {
            "namespace": namespace,
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

fn entity_key(
    namespace: DeploymentNamespaceV1,
    version_id: Uuid,
    document_id: Uuid,
    name: &str,
) -> String {
    format!("{namespace}:{version_id}:{document_id}:{name}")
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
            if std::env::var("KNOWLEDGEBRAIN_REQUIRE_NEO4J_TESTS").as_deref() == Ok("1") {
                panic!("KNOWLEDGEBRAIN_REQUIRE_NEO4J_TESTS=1 requires Neo4j configuration");
            }
            eprintln!("skip: neo4j not configured");
            return;
        }
        let mut store = Store::default();
        let vid = Uuid::new_v4();
        let did = Uuid::new_v4();
        let cid = Uuid::new_v4();
        store.documents.insert(
            did,
            crate::Document::new(vid, "t".into(), "t.txt".into(), 1, "h".into(), "k".into()),
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

    #[test]
    #[ignore = "requires explicitly owned Neo4j service and KNOWLEDGEBRAIN_REQUIRE_NEO4J_TESTS=1"]
    fn live_deployment_namespaces_isolate_identical_document_ids() {
        assert_eq!(
            std::env::var("KNOWLEDGEBRAIN_REQUIRE_NEO4J_TESTS").as_deref(),
            Ok("1")
        );
        assert!(
            configured(),
            "required graph isolation test needs an isolated Neo4j service"
        );
        let first: DeploymentNamespaceV1 = Uuid::new_v4().to_string().parse().unwrap();
        let second: DeploymentNamespaceV1 = Uuid::new_v4().to_string().parse().unwrap();
        let version = Uuid::new_v4();
        let document = Uuid::new_v4();
        let mut store = Store::default();
        store.documents.insert(
            document,
            crate::Document::new(
                version,
                "fixture".into(),
                "fixture.txt".into(),
                1,
                "h".into(),
                "k".into(),
            ),
        );
        store.upsert_node(version, document, "Common", Uuid::new_v4());
        store.upsert_node(version, document, "First", Uuid::new_v4());
        store.upsert_rel(version, document, "Common", "First", "links");
        sync_document_in_namespace(&store, document, first).unwrap();
        assert!(
            search_names_in_namespace(version, "Common", second)
                .unwrap()
                .is_empty()
        );
        let first_chunk = search_names_in_namespace(version, "Common", first).unwrap()[0]
            .chunk_ids
            .clone();
        store.graph.clear();
        store.relations.clear();
        store.upsert_node(version, document, "Common", Uuid::new_v4());
        store.upsert_node(version, document, "Second", Uuid::new_v4());
        store.upsert_rel(version, document, "Common", "Second", "links");
        sync_document_in_namespace(&store, document, second).unwrap();
        assert_eq!(
            search_names_in_namespace(version, "Common", first).unwrap()[0].chunk_ids,
            first_chunk
        );
        assert!(
            search_names_in_namespace(version, "Second", first)
                .unwrap()
                .is_empty()
        );
        let graph = cypher(&[json!({
            "statement":"MATCH (a:KbEntity)-[r:KB_REL]->(b:KbEntity) WHERE a.document_id=$did RETURN a.deployment_namespace_id, r.deployment_namespace_id, b.deployment_namespace_id",
            "parameters":{"did":document.to_string()}
        })]).unwrap();
        let rows = graph["results"][0]["data"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let mut namespaces = std::collections::BTreeSet::new();
        for row in rows {
            let fields = row["row"].as_array().unwrap();
            assert_eq!(fields[0], fields[1]);
            assert_eq!(fields[1], fields[2]);
            namespaces.insert(fields[0].as_str().unwrap().to_owned());
        }
        assert_eq!(
            namespaces,
            std::collections::BTreeSet::from([first.to_string(), second.to_string()])
        );
        // An old unnamespaced projection is never adopted or removed implicitly.
        cypher(&[json!({"statement":"CREATE (:KbEntity {name:'Legacy',version_id:$vid,document_id:$did,key:$key})",
            "parameters":{"vid":version.to_string(),"did":document.to_string(),"key":Uuid::new_v4().to_string()}})]).unwrap();
        delete_document_in_namespace(version, document, first).unwrap();
        assert!(
            search_names_in_namespace(version, "Common", first)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            search_names_in_namespace(version, "Common", second)
                .unwrap()
                .len(),
            1
        );
        delete_document_in_namespace(version, document, second).unwrap();
        let remaining=cypher(&[json!({"statement":"MATCH (e:KbEntity {document_id:$did}) RETURN e.name,e.deployment_namespace_id",
            "parameters":{"did":document.to_string()}})]).unwrap();
        let remaining = remaining["results"][0]["data"].as_array().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0]["row"], json!(["Legacy", null]));
        // Remove only this test's explicitly created legacy fixture.
        cypher(&[json!({"statement":"MATCH (e:KbEntity {document_id:$did,version_id:$vid,name:'Legacy'}) WHERE e.deployment_namespace_id IS NULL DELETE e",
            "parameters":{"did":document.to_string(),"vid":version.to_string()}})]).unwrap();
    }
}
