-- One-shot backfill. apply_0001 re-runs every connect(); must not
-- keep moving newly created product-line libraries.

CREATE TABLE IF NOT EXISTS schema_flags (
    name text PRIMARY KEY,
    applied_at timestamptz NOT NULL DEFAULT now()
);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM schema_flags WHERE name = '0013_bid_backfill') THEN
        RETURN;
    END IF;

    UPDATE product_versions
    SET image_processing_config = jsonb_set(
            COALESCE(image_processing_config, '{}'::jsonb),
            '{enable_multimodel}',
            'true'::jsonb,
            true
        )
    WHERE deleted_at IS NULL;

    UPDATE products p
    SET workspace_id = c.id,
        slug = 'legacy-' || w.slug
    FROM workspaces w
    JOIN workspaces c ON c.kind = 'company'
    WHERE p.workspace_id = w.id
      AND w.kind = 'product_line'
      AND p.kind = 'library'
      AND p.slug = 'library'
      AND NOT EXISTS (
          SELECT 1 FROM products x
          WHERE x.workspace_id = c.id AND x.slug = 'legacy-' || w.slug
      );

    INSERT INTO schema_flags (name) VALUES ('0013_bid_backfill');
END $$;
