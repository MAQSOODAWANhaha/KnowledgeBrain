-- KnowledgeBrain 0005: graph nodes and relations. Namespace is (version, document).

CREATE TABLE IF NOT EXISTS graph_nodes (
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    document_id uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    name text NOT NULL,
    chunk_ids uuid[] NOT NULL DEFAULT '{}',
    PRIMARY KEY (product_version_id, document_id, name)
);

CREATE TABLE IF NOT EXISTS graph_relations (
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    document_id uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    node1 text NOT NULL,
    node2 text NOT NULL,
    rel_type text NOT NULL,
    PRIMARY KEY (product_version_id, document_id, node1, node2, rel_type)
);
