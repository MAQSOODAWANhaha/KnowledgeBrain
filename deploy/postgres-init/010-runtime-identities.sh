#!/bin/sh
set -eu

: "${KNOWLEDGEBRAIN_MIGRATOR_PASSWORD:?set KNOWLEDGEBRAIN_MIGRATOR_PASSWORD}"
: "${KNOWLEDGEBRAIN_API_DB_PASSWORD:?set KNOWLEDGEBRAIN_API_DB_PASSWORD}"
: "${KNOWLEDGEBRAIN_WORKER_DB_PASSWORD:?set KNOWLEDGEBRAIN_WORKER_DB_PASSWORD}"
: "${KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD:?set KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD}"

psql --set ON_ERROR_STOP=1 \
  --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  --variable migrator_password="$KNOWLEDGEBRAIN_MIGRATOR_PASSWORD" \
  --variable api_password="$KNOWLEDGEBRAIN_API_DB_PASSWORD" \
  --variable worker_password="$KNOWLEDGEBRAIN_WORKER_DB_PASSWORD" \
  --variable retention_password="$KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD" <<'SQL'
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pgcrypto;
DO $roles$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_app_owner') THEN
    CREATE ROLE kb_app_owner NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD NULL;
  END IF;
END $roles$;
SELECT format('CREATE ROLE kb_migrator LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L', :'migrator_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_migrator') \gexec
SELECT format('CREATE ROLE kb_runtime_api LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L', :'api_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_runtime_api') \gexec
SELECT format('CREATE ROLE kb_runtime_worker LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L', :'worker_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_runtime_worker') \gexec
SELECT format('CREATE ROLE kb_runtime_retention LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L', :'retention_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_runtime_retention') \gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO kb_migrator,kb_runtime_api,kb_runtime_worker,kb_runtime_retention', current_database()) \gexec
SELECT format('REVOKE TEMPORARY ON DATABASE %I FROM PUBLIC', current_database()) \gexec
GRANT USAGE ON SCHEMA public TO kb_migrator,kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
GRANT CREATE ON SCHEMA public TO kb_migrator;
GRANT EXECUTE ON FUNCTION public.digest(bytea,text), public.digest(text,text) TO kb_migrator,kb_app_owner;
GRANT EXECUTE ON FUNCTION public.vector_in(cstring,oid,integer), public.vector_out(public.vector), public.vector(public.vector,integer,boolean), public.cosine_distance(public.vector,public.vector) TO kb_runtime_api,kb_runtime_worker;
ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator IN SCHEMA public REVOKE ALL ON TABLES FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator IN SCHEMA public REVOKE ALL ON FUNCTIONS FROM PUBLIC;
SQL
