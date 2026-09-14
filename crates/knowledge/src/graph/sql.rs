//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn persist_graph_maps(
    pool: &PgPool,
    graph: &std::collections::HashMap<(Uuid, Uuid, String), crate::GraphNode>,
    relations: &std::collections::HashMap<
        (Uuid, Uuid, String, String, String),
        crate::GraphRelation,
    >,
    document_id: Uuid,
) -> Result<(), sqlx::Error> {
    for n in graph.values().filter(|n| n.document_id == document_id) {
        sqlx::query(
            "INSERT INTO graph_nodes (product_version_id, document_id, name, chunk_ids)
             VALUES ($1,$2,$3,$4)
             ON CONFLICT (product_version_id, document_id, name)
             DO UPDATE SET chunk_ids = (
                 SELECT ARRAY(SELECT DISTINCT x FROM unnest(graph_nodes.chunk_ids || EXCLUDED.chunk_ids) AS x)
             )",
        )
        .bind(n.version_id)
        .bind(n.document_id)
        .bind(&n.name)
        .bind(&n.chunk_ids)
        .execute(pool)
        .await?;
    }
    for r in relations.values().filter(|r| r.document_id == document_id) {
        sqlx::query(
            "INSERT INTO graph_relations
                (product_version_id, document_id, node1, node2, rel_type)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT DO NOTHING",
        )
        .bind(r.version_id)
        .bind(r.document_id)
        .bind(&r.node1)
        .bind(&r.node2)
        .bind(&r.rel_type)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn delete_graph_for_document(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM graph_relations WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM graph_nodes WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PgGraphHit {
    pub name: String,
    pub document_id: Uuid,
    pub document_title: String,
    pub chunk_id: Uuid,
    pub content: String,
    pub start_at: i32,
    pub end_at: i32,
    pub product_id: Uuid,
    pub product_kind: String,
    pub version_id: Uuid,
    pub version_label: String,
    pub is_current: bool,
    pub tag_ids: Vec<Uuid>,
    pub tag_slugs: Vec<String>,
}

pub async fn graph_hits_pg(
    pool: &PgPool,
    version_id: Uuid,
    query: &str,
    limit: i64,
    tag_ids: &[Uuid],
) -> Result<Vec<PgGraphHit>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT n.name, n.document_id, d.title,
                COALESCE(c.id, '00000000-0000-0000-0000-000000000000'::uuid) AS chunk_id,
                COALESCE(c.content, '') AS content,
                COALESCE(c.start_at, 0) AS start_at,
                COALESCE(c.end_at, 0) AS end_at,
                p.id AS product_id, p.kind AS product_kind,
                pv.id AS version_id, pv.label AS version_label,
                (p.current_version_id = pv.id) AS is_current,
                COALESCE((
                    SELECT array_agg(dt.tag_id)
                    FROM document_tags dt WHERE dt.document_id = d.id
                ), ARRAY[]::uuid[]) AS tag_ids,
                COALESCE((
                    SELECT array_agg(t.slug)
                    FROM document_tags dt
                    JOIN tags t ON t.id = dt.tag_id
                    WHERE dt.document_id = d.id
                ), ARRAY[]::text[]) AS tag_slugs
         FROM graph_nodes n
         JOIN documents d ON d.id = n.document_id
         JOIN product_versions pv ON pv.id = n.product_version_id
         JOIN products p ON p.id = pv.product_id
         LEFT JOIN LATERAL (
            SELECT id, content, start_at, end_at
            FROM chunks
            WHERE id = ANY(n.chunk_ids)
            LIMIT 1
         ) c ON true
         WHERE n.product_version_id = $1
           AND d.deleted_at IS NULL
           AND (
                n.name ILIKE '%' || $2 || '%'
                OR $2 ILIKE '%' || n.name || '%'
           )
           AND (
                cardinality($4::uuid[]) = 0
                OR EXISTS (
                    SELECT 1 FROM document_tags dt
                    WHERE dt.document_id = d.id AND dt.tag_id = ANY($4)
                )
           )
         LIMIT $3",
    )
    .bind(version_id)
    .bind(query)
    .bind(limit)
    .bind(tag_ids)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(PgGraphHit {
            name: r.try_get("name")?,
            document_id: r.try_get("document_id")?,
            document_title: r.try_get("title")?,
            chunk_id: r.try_get("chunk_id")?,
            content: r.try_get("content")?,
            start_at: r.try_get("start_at")?,
            end_at: r.try_get("end_at")?,
            product_id: r.try_get("product_id")?,
            product_kind: r.try_get("product_kind")?,
            version_id: r.try_get("version_id")?,
            version_label: r.try_get("version_label")?,
            is_current: r.try_get("is_current")?,
            tag_ids: r.try_get("tag_ids").unwrap_or_default(),
            tag_slugs: r.try_get("tag_slugs").unwrap_or_default(),
        });
    }
    Ok(out)
}

pub async fn load_graph_for_document(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<
    (
        std::collections::HashMap<(Uuid, Uuid, String), crate::GraphNode>,
        std::collections::HashMap<(Uuid, Uuid, String, String, String), crate::GraphRelation>,
    ),
    sqlx::Error,
> {
    let mut graph = std::collections::HashMap::new();
    let nodes = sqlx::query(
        "SELECT product_version_id, document_id, name, chunk_ids
         FROM graph_nodes WHERE document_id = $1",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    for row in nodes {
        let version_id: Uuid = row.try_get("product_version_id")?;
        let document_id: Uuid = row.try_get("document_id")?;
        let name: String = row.try_get("name")?;
        let chunk_ids: Vec<Uuid> = row.try_get("chunk_ids").unwrap_or_default();
        graph.insert(
            (version_id, document_id, name.clone()),
            crate::GraphNode {
                version_id,
                document_id,
                name,
                chunk_ids,
            },
        );
    }
    let mut relations = std::collections::HashMap::new();
    let rows = sqlx::query(
        "SELECT product_version_id, document_id, node1, node2, rel_type
         FROM graph_relations WHERE document_id = $1",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    for row in rows {
        let version_id: Uuid = row.try_get("product_version_id")?;
        let document_id: Uuid = row.try_get("document_id")?;
        let node1: String = row.try_get("node1")?;
        let node2: String = row.try_get("node2")?;
        let rel_type: String = row.try_get("rel_type")?;
        relations.insert(
            (
                version_id,
                document_id,
                node1.clone(),
                node2.clone(),
                rel_type.clone(),
            ),
            crate::GraphRelation {
                version_id,
                document_id,
                node1,
                node2,
                rel_type,
            },
        );
    }
    Ok((graph, relations))
}
