#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
: "${DATABASE_URL:?DATABASE_URL is required}"

psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 <<'SQL'
DROP SCHEMA public CASCADE;
CREATE SCHEMA public;
GRANT ALL ON SCHEMA public TO CURRENT_USER;
SQL
cargo run --locked -p platform --bin migrator
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -At <<'SQL'
SELECT CASE WHEN
  to_regclass('public.workspaces') IS NOT NULL AND
  to_regclass('public.object_registry') IS NOT NULL AND
  to_regclass('public.bid_projects') IS NOT NULL AND
  to_regclass('public.bid_submission_workspaces') IS NOT NULL AND
  to_regclass('public.bid_part_content_artifacts') IS NULL AND
  to_regclass('public.production_launch_state') IS NULL AND
  to_regclass('public.production_first_launch_catalog_verifications') IS NULL
THEN 'fresh-v2-ok' ELSE 'fresh-v2-invalid' END;
SQL
cargo run --locked -p platform --bin migrator
