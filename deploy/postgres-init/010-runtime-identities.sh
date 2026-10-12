#!/bin/sh
# Fresh cluster bootstrap only: engine admin remains local; all application
# components share this one non-superuser identity. Never repairs existing roles.
set -eu
: "${KB_APP_DB_USER:?set POSTGRES_USER for the application}"
: "${KB_APP_DB_PASSWORD:?set POSTGRES_PASSWORD for the application}"
case "$KB_APP_DB_USER" in
  postgres|pg_*|[[:space:]]*|*[[:space:]])
    echo 'POSTGRES_USER must be a non-reserved application username without outer whitespace' >&2; exit 1 ;;
esac
if [ "$(printf '%s' "$KB_APP_DB_USER" | wc -c)" -gt 63 ]; then
  echo 'POSTGRES_USER exceeds PostgreSQL identifier length' >&2; exit 1
fi
case "$KB_APP_DB_PASSWORD" in
  *[![:space:]]*) ;;
  *) echo 'POSTGRES_PASSWORD must not be blank' >&2; exit 1 ;;
esac
psql --set ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  --variable app_user="$KB_APP_DB_USER" --variable app_password="$KB_APP_DB_PASSWORD" <<'SQL'
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pgcrypto;
SELECT format('CREATE ROLE %I LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L', :'app_user', :'app_password') \gexec
SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', current_database()) \gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO %I', current_database(), :'app_user') \gexec
SELECT format('ALTER SCHEMA public OWNER TO %I', :'app_user') \gexec
REVOKE ALL ON SCHEMA public FROM PUBLIC;
SELECT format('GRANT USAGE, CREATE ON SCHEMA public TO %I', :'app_user') \gexec
SQL
