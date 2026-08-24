#!/bin/sh
set -eu

: "${KNOWLEDGEBRAIN_MIGRATOR_PASSWORD:?set KNOWLEDGEBRAIN_MIGRATOR_PASSWORD}"
: "${KNOWLEDGEBRAIN_FIRST_LAUNCH_VERIFIER_PASSWORD:?set KNOWLEDGEBRAIN_FIRST_LAUNCH_VERIFIER_PASSWORD}"
: "${KNOWLEDGEBRAIN_API_DB_PASSWORD:?set KNOWLEDGEBRAIN_API_DB_PASSWORD}"
: "${KNOWLEDGEBRAIN_WORKER_DB_PASSWORD:?set KNOWLEDGEBRAIN_WORKER_DB_PASSWORD}"
: "${KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD:?set KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD}"

psql --set ON_ERROR_STOP=1 \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --variable migrator_password="$KNOWLEDGEBRAIN_MIGRATOR_PASSWORD" \
  --variable verifier_password="$KNOWLEDGEBRAIN_FIRST_LAUNCH_VERIFIER_PASSWORD" \
  --variable api_password="$KNOWLEDGEBRAIN_API_DB_PASSWORD" \
  --variable worker_password="$KNOWLEDGEBRAIN_WORKER_DB_PASSWORD" \
  --variable retention_password="$KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD" <<'SQL'
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pgcrypto;

SELECT format(
  'CREATE ROLE kb_migrator LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'migrator_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_migrator') \gexec
CREATE ROLE kb_app_owner NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE
 NOREPLICATION NOBYPASSRLS PASSWORD NULL;
GRANT EXECUTE ON FUNCTION public.digest(bytea,text), public.digest(text,text)
 TO kb_app_owner;
SELECT format(
  'CREATE ROLE kb_first_launch_verifier LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'verifier_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_first_launch_verifier') \gexec
SELECT format(
  'CREATE ROLE kb_runtime_api LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'api_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_runtime_api') \gexec
SELECT format(
  'CREATE ROLE kb_runtime_worker LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'worker_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_runtime_worker') \gexec
SELECT format(
  'CREATE ROLE kb_runtime_retention LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'retention_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='kb_runtime_retention') \gexec

-- pgvector is bootstrap-owned. Knowledge indexing/search binds text vectors,
-- applies the declared typmod, reads vectors for replay, and ranks by cosine;
-- keep those four extension routines role-scoped after PUBLIC is revoked.
GRANT EXECUTE ON FUNCTION
 public.vector_in(cstring,oid,integer),
 public.vector_out(public.vector),
 public.vector(public.vector,integer,boolean),
 public.cosine_distance(public.vector,public.vector)
TO kb_runtime_api,kb_runtime_worker;

DO $launch_roles$
DECLARE role_name text; expected_inherit boolean;
BEGIN
 FOREACH role_name IN ARRAY ARRAY[
  'kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
  'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'
 ] LOOP
  expected_inherit := role_name <> 'kb_launch_owner';
  IF NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname=role_name) THEN
   EXECUTE format(
    'CREATE ROLE %I NOLOGIN %s NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD NULL',
    role_name, CASE WHEN expected_inherit THEN 'INHERIT' ELSE 'NOINHERIT' END);
  ELSIF EXISTS(
   SELECT 1 FROM pg_catalog.pg_roles
   WHERE rolname=role_name AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole
    OR rolreplication OR rolbypassrls OR rolconnlimit<>-1 OR rolvaliduntil IS NOT NULL
    OR rolinherit IS DISTINCT FROM expected_inherit)
  ) THEN
   RAISE EXCEPTION 'existing launch role % has incompatible attributes',role_name
    USING ERRCODE='42501';
  END IF;
 END LOOP;
END $launch_roles$;

DO $bootstrap_topology$
BEGIN
 IF EXISTS(
  SELECT 1 FROM pg_catalog.pg_roles role_value
  WHERE role_value.rolname=ANY(ARRAY[
   'kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention'])
    AND (NOT role_value.rolcanlogin OR NOT role_value.rolinherit OR role_value.rolsuper
      OR role_value.rolcreatedb OR role_value.rolcreaterole OR role_value.rolreplication
      OR role_value.rolbypassrls OR role_value.rolconnlimit<>-1
      OR role_value.rolvaliduntil IS NOT NULL)
 ) OR EXISTS(
  SELECT 1 FROM pg_catalog.pg_auth_members membership
  JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
  JOIN pg_catalog.pg_roles member ON member.oid=membership.member
  WHERE granted.rolname=ANY(ARRAY[
    'kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention',
    'kb_app_owner','kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
    'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'])
     OR member.rolname=ANY(ARRAY[
    'kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention',
    'kb_app_owner','kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
    'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'])
 ) THEN
  RAISE EXCEPTION 'existing runtime/migrator/verifier membership or role attributes violate bootstrap topology'
   USING ERRCODE='42501';
 END IF;
END $bootstrap_topology$;

CREATE OR REPLACE FUNCTION pg_catalog.kb_launch_role_password_absent(role_name name)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path=pg_catalog
AS $password_contract$
BEGIN
 IF role_name::text <> ALL(ARRAY[
  'kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention',
  'kb_app_owner','kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
  'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'
 ]) THEN
  RAISE EXCEPTION 'role is outside the fixed launch-role password contract'
   USING ERRCODE='42501';
 END IF;
 RETURN (SELECT role_value.rolpassword IS NULL
         FROM pg_catalog.pg_authid role_value WHERE role_value.rolname=role_name::text);
END $password_contract$;
REVOKE ALL ON FUNCTION pg_catalog.kb_launch_role_password_absent(name) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION pg_catalog.kb_launch_role_password_absent(name)
 TO kb_migrator,kb_first_launch_verifier,kb_runtime_api,kb_runtime_worker,kb_runtime_retention;

-- PostgreSQL/bootstrap-owned, fixed-search-path, one-shot trust handoff. The
-- ACCESS EXCLUSIVE marker lock is the narrow mechanism which may bypass the
-- append-only triggers solely to erase evidence forged during migration-owner
-- SET ROLE reachability. This is phase 1: it commits NOLOGIN and removes all
-- migrator authority, but deliberately does not terminate another backend in
-- its transaction.
CREATE OR REPLACE FUNCTION pg_catalog.kb_handoff_first_launch_to_verifier()
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path=pg_catalog,public
AS $handoff$
DECLARE edge record;
DECLARE launch_state text;
DECLARE governed constant text[] := ARRAY[
 'kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention',
 'kb_app_owner','kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
 'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'];
BEGIN
 IF session_user IS DISTINCT FROM 'kb_migrator' THEN
  RAISE EXCEPTION 'first-launch handoff is restricted to kb_migrator session_user'
   USING ERRCODE='42501';
 END IF;
 -- current_user is the bootstrap owner inside this SECURITY DEFINER routine,
 -- so inspect PostgreSQL's role GUC to reject an invoker that entered through
 -- SET ROLE. NULL and every value other than the plain-session sentinel fail
 -- closed.
 IF pg_catalog.current_setting('role',true) IS DISTINCT FROM 'none' THEN
  RAISE EXCEPTION 'first-launch handoff rejects an active SET ROLE'
   USING ERRCODE='42501';
 END IF;
 PERFORM pg_catalog.pg_advisory_xact_lock(5422981878432022868);
 IF to_regclass('public.schema_migrations') IS NULL
    OR (SELECT array_agg(version ORDER BY version) FROM public.schema_migrations)
       IS DISTINCT FROM ARRAY[1,2,3]
    OR (SELECT array_agg(name ORDER BY version) FROM public.schema_migrations)
       IS DISTINCT FROM ARRAY['knowledge_base_baseline','shared_platform_baseline','bidding_v1_baseline']
 THEN
  RAISE EXCEPTION 'first-launch handoff requires the exact closed migration head'
   USING ERRCODE='55000';
 END IF;
 SELECT state INTO STRICT launch_state FROM public.production_launch_state WHERE singleton_key;
 IF launch_state IS DISTINCT FROM 'preflight'
    OR EXISTS(SELECT 1 FROM public.production_launch_state
              WHERE cutover_id IS NOT NULL OR cutover_epoch<>0 OR evidence_epoch<>0
                 OR traffic_exposure_started_at IS NOT NULL
                 OR reset_authority_revoked_at IS NOT NULL
                 OR first_production_request_at IS NOT NULL)
    OR NOT EXISTS(SELECT 1 FROM public.application_maintenance_gate
                  WHERE singleton_key AND mode='maintenance')
 THEN
  RAISE EXCEPTION 'first-launch handoff requires closed maintenance preflight state'
   USING ERRCODE='55000';
 END IF;

 -- Commit authentication closure before phase 2 can terminate residual
 -- sessions. Once this transaction commits no replacement migrator session
 -- can authenticate.
 ALTER ROLE kb_migrator NOLOGIN NOINHERIT PASSWORD NULL NOSUPERUSER NOCREATEDB
  NOCREATEROLE NOREPLICATION NOBYPASSRLS;

 LOCK TABLE public.production_first_launch_catalog_verifications IN ACCESS EXCLUSIVE MODE;
 ALTER TABLE public.production_first_launch_catalog_verifications DISABLE TRIGGER USER;
 DELETE FROM public.production_first_launch_catalog_verifications;
 ALTER TABLE public.production_first_launch_catalog_verifications ENABLE TRIGGER USER;

 FOR edge IN
  SELECT granted.rolname AS granted_role,member.rolname AS member_role
  FROM pg_catalog.pg_auth_members membership
  JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
  JOIN pg_catalog.pg_roles member ON member.oid=membership.member
  WHERE granted.rolname=ANY(governed) OR member.rolname=ANY(governed)
 LOOP
  EXECUTE format('REVOKE %I FROM %I',edge.granted_role,edge.member_role);
 END LOOP;
 -- Transfer every migrator-owned object in this database, including relations,
 -- sequences, routines, types, the database, and the public schema. Launch
 -- objects created under kb_launch_owner are intentionally unaffected.
 REASSIGN OWNED BY kb_migrator TO kb_app_owner;
 DROP OWNED BY kb_migrator;
 ALTER TABLE public.schema_migrations OWNER TO kb_launch_owner;
 EXECUTE format('ALTER DATABASE %I OWNER TO kb_app_owner',current_database());
 ALTER SCHEMA public OWNER TO kb_app_owner;

 EXECUTE format('REVOKE ALL PRIVILEGES ON DATABASE %I FROM PUBLIC,kb_migrator,kb_first_launch_verifier,kb_runtime_api,kb_runtime_worker,kb_runtime_retention,kb_app_owner',current_database());
 REVOKE ALL ON SCHEMA public FROM PUBLIC,kb_migrator,kb_first_launch_verifier,
  kb_runtime_api,kb_runtime_worker,kb_runtime_retention,kb_app_owner,kb_launch_owner,kb_launch_operator,
  kb_launch_router,kb_launch_ingress,kb_launch_attestor,kb_launch_signature_verifier,
  kb_launch_reset_dispatcher;
 REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM kb_migrator;
 REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM kb_migrator;
 REVOKE ALL PRIVILEGES ON ALL ROUTINES IN SCHEMA public FROM PUBLIC,kb_migrator;
 ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator REVOKE ALL ON TABLES FROM PUBLIC,kb_migrator;
 ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator REVOKE ALL ON SEQUENCES FROM PUBLIC,kb_migrator;
 ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator REVOKE ALL ON ROUTINES FROM PUBLIC,kb_migrator;
 ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator REVOKE ALL ON TYPES FROM PUBLIC,kb_migrator;
 ALTER DEFAULT PRIVILEGES FOR ROLE kb_app_owner IN SCHEMA public
  REVOKE EXECUTE ON ROUTINES FROM PUBLIC;
 ALTER ROLE kb_migrator NOLOGIN NOINHERIT PASSWORD NULL NOSUPERUSER NOCREATEDB
  NOCREATEROLE NOREPLICATION NOBYPASSRLS;
 ALTER ROLE kb_first_launch_verifier NOSUPERUSER NOCREATEDB NOCREATEROLE
  NOREPLICATION NOBYPASSRLS;

 EXECUTE format('GRANT CONNECT ON DATABASE %I TO kb_first_launch_verifier,kb_runtime_api,kb_runtime_worker,kb_runtime_retention',current_database());
 GRANT USAGE ON SCHEMA public TO kb_first_launch_verifier,kb_runtime_api,
  kb_runtime_worker,kb_runtime_retention,kb_app_owner,kb_launch_owner,kb_launch_operator,kb_launch_router,
  kb_launch_ingress,kb_launch_attestor,kb_launch_signature_verifier,
  kb_launch_reset_dispatcher;
 REVOKE ALL ON public.schema_migrations FROM PUBLIC,kb_migrator,kb_first_launch_verifier,
  kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
 GRANT SELECT ON public.schema_migrations
  TO kb_first_launch_verifier,kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
 REVOKE ALL ON public.production_first_launch_catalog_verifications
  FROM PUBLIC,kb_migrator,kb_first_launch_verifier,kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
 GRANT SELECT,INSERT ON public.production_first_launch_catalog_verifications TO kb_first_launch_verifier;
 GRANT SELECT ON public.production_first_launch_catalog_verifications TO kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
 REVOKE EXECUTE ON FUNCTION pg_catalog.kb_finalize_first_launch_privileges()
  FROM PUBLIC,kb_migrator,kb_first_launch_verifier;
 GRANT EXECUTE ON FUNCTION pg_catalog.kb_finalize_first_launch_privileges()
  TO kb_first_launch_verifier;
 REVOKE EXECUTE ON FUNCTION pg_catalog.kb_handoff_first_launch_to_verifier()
  FROM PUBLIC,kb_migrator,kb_first_launch_verifier;
 GRANT kb_app_owner,kb_launch_owner TO kb_first_launch_verifier
  WITH ADMIN FALSE, INHERIT FALSE, SET TRUE;

 IF pg_catalog.pg_has_role('kb_migrator','kb_app_owner','SET')
    OR pg_catalog.pg_has_role('kb_migrator','kb_launch_owner','SET')
    OR pg_catalog.has_schema_privilege('kb_migrator','public','USAGE')
    OR pg_catalog.has_database_privilege('kb_migrator',current_database(),'CONNECT')
    OR EXISTS(SELECT 1 FROM pg_catalog.pg_class relation
       JOIN pg_catalog.pg_namespace namespace ON namespace.oid=relation.relnamespace
       WHERE namespace.nspname='public'
         AND ((relation.relkind IN ('r','p','v','m','f')
               AND pg_catalog.has_table_privilege('kb_migrator',relation.oid,
                 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))
           OR (relation.relkind='S' AND pg_catalog.has_sequence_privilege(
                 'kb_migrator',relation.oid,'USAGE,SELECT,UPDATE'))))
    OR EXISTS(SELECT 1 FROM pg_catalog.pg_proc routine
       JOIN pg_catalog.pg_namespace namespace ON namespace.oid=routine.pronamespace
       WHERE namespace.nspname='public'
         AND pg_catalog.has_function_privilege('kb_migrator',routine.oid,'EXECUTE'))
    OR (SELECT count(*) FROM pg_catalog.pg_auth_members membership
        JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
        JOIN pg_catalog.pg_roles member ON member.oid=membership.member
        WHERE granted.rolname=ANY(governed) OR member.rolname=ANY(governed))<>2
    OR NOT pg_catalog.pg_has_role('kb_first_launch_verifier','kb_app_owner','SET')
    OR NOT pg_catalog.pg_has_role('kb_first_launch_verifier','kb_launch_owner','SET')
 THEN
  RAISE EXCEPTION 'first-launch handoff did not establish exact verifier-only topology'
   USING ERRCODE='42501';
 END IF;
END $handoff$;
REVOKE ALL ON FUNCTION pg_catalog.kb_handoff_first_launch_to_verifier() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION pg_catalog.kb_handoff_first_launch_to_verifier() TO kb_migrator;

-- Phase 2 runs only after phase 1 committed and the migrate-only process closed
-- its migrator pool. It is owned by and executable only as the PostgreSQL
-- bootstrap identity; kb_migrator is never granted EXECUTE.
CREATE OR REPLACE FUNCTION pg_catalog.kb_terminate_residual_migrator_backends()
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path=pg_catalog
AS $terminate_migrator$
DECLARE attempt integer := 0;
DECLARE deadline timestamptz := pg_catalog.clock_timestamp() + interval '10 seconds';
DECLARE migrator_role_oid oid;
DECLARE backend_pid integer;
DECLARE backend_pids integer[];
BEGIN
 IF session_user IS DISTINCT FROM pg_catalog.pg_get_userbyid(
      (SELECT routine.proowner FROM pg_catalog.pg_proc routine
       JOIN pg_catalog.pg_namespace namespace ON namespace.oid=routine.pronamespace
       WHERE namespace.nspname='pg_catalog'
         AND routine.proname='kb_terminate_residual_migrator_backends'
         AND routine.pronargs=0))
    OR pg_catalog.current_setting('role',true) IS DISTINCT FROM 'none'
 THEN
  RAISE EXCEPTION 'residual migrator termination requires the bootstrap admin session'
   USING ERRCODE='42501';
 END IF;
 SELECT oid INTO STRICT migrator_role_oid
 FROM pg_catalog.pg_authid
 WHERE rolname='kb_migrator' AND NOT rolcanlogin AND rolpassword IS NULL;

 LOOP
  attempt := attempt + 1;
  PERFORM pg_catalog.pg_stat_clear_snapshot();
  SELECT pg_catalog.array_agg(activity.pid ORDER BY activity.pid)
   INTO backend_pids
  FROM pg_catalog.pg_stat_activity activity
  WHERE activity.usesysid=migrator_role_oid;
  IF COALESCE(pg_catalog.array_length(backend_pids,1),0)=0 THEN
   RETURN;
  END IF;
  FOREACH backend_pid IN ARRAY backend_pids LOOP
   -- A backend may disappear after enumeration. A false return is benign; the
   -- next exact-zero scan is the success criterion.
   PERFORM pg_catalog.pg_terminate_backend(backend_pid,1000);
  END LOOP;
  PERFORM pg_catalog.pg_stat_clear_snapshot();
  IF NOT EXISTS(SELECT 1 FROM pg_catalog.pg_stat_activity activity
                WHERE activity.usesysid=migrator_role_oid) THEN
   RETURN;
  END IF;
  IF attempt>=100 OR pg_catalog.clock_timestamp()>=deadline THEN
   RAISE EXCEPTION 'residual migrator backends did not reach exact zero before deadline'
    USING ERRCODE='55000';
  END IF;
  PERFORM pg_catalog.pg_sleep(0.05);
 END LOOP;
END $terminate_migrator$;
REVOKE ALL ON FUNCTION pg_catalog.kb_terminate_residual_migrator_backends()
 FROM PUBLIC,kb_migrator,kb_first_launch_verifier,kb_runtime_api,kb_runtime_worker,kb_runtime_retention;

CREATE OR REPLACE FUNCTION pg_catalog.kb_finalize_first_launch_privileges()
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path=pg_catalog,public
AS $finalizer$
DECLARE launch_state text;
DECLARE governed constant text[] := ARRAY[
 'kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention',
 'kb_app_owner','kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
 'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'];
BEGIN
 IF session_user IS DISTINCT FROM 'kb_first_launch_verifier' THEN
  RAISE EXCEPTION 'first-launch privilege finalizer is restricted to kb_first_launch_verifier session_user'
   USING ERRCODE='42501';
 END IF;
 SELECT state INTO STRICT launch_state
 FROM public.production_launch_state WHERE singleton_key;
 IF launch_state IS DISTINCT FROM 'preflight'
    OR EXISTS(SELECT 1 FROM public.production_launch_state
              WHERE traffic_exposure_started_at IS NOT NULL
                 OR reset_authority_revoked_at IS NOT NULL
                 OR first_production_request_at IS NOT NULL)
    OR NOT EXISTS(SELECT 1 FROM public.application_maintenance_gate
                  WHERE singleton_key AND mode='maintenance')
    OR NOT EXISTS(SELECT 1 FROM public.production_first_launch_catalog_verifications
                  WHERE singleton_key)
    OR pg_catalog.pg_get_userbyid((SELECT datdba FROM pg_catalog.pg_database
                                   WHERE datname=current_database()))<>'kb_app_owner'
    OR pg_catalog.pg_get_userbyid((SELECT nspowner FROM pg_catalog.pg_namespace
                                   WHERE nspname='public'))<>'kb_app_owner'
    OR pg_catalog.pg_has_role('kb_migrator','kb_app_owner','SET')
    OR pg_catalog.pg_has_role('kb_migrator','kb_launch_owner','SET')
    OR (SELECT count(*) FROM pg_catalog.pg_auth_members membership
        JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
        JOIN pg_catalog.pg_roles member ON member.oid=membership.member
        WHERE granted.rolname=ANY(governed) OR member.rolname=ANY(governed))<>2
    OR NOT pg_catalog.pg_has_role('kb_first_launch_verifier','kb_app_owner','SET')
    OR NOT pg_catalog.pg_has_role('kb_first_launch_verifier','kb_launch_owner','SET')
 THEN
  RAISE EXCEPTION 'first-launch privilege finalization requires verified handoff-complete topology'
   USING ERRCODE='42501';
 END IF;
 REVOKE kb_app_owner,kb_launch_owner FROM kb_first_launch_verifier;
 REVOKE ALL ON public.production_first_launch_catalog_verifications FROM kb_migrator,kb_first_launch_verifier;
 REVOKE ALL ON SCHEMA public FROM kb_first_launch_verifier;
 EXECUTE format('REVOKE ALL PRIVILEGES ON DATABASE %I FROM kb_first_launch_verifier',current_database());
 REVOKE EXECUTE ON FUNCTION pg_catalog.kb_launch_role_password_absent(name)
  FROM kb_migrator,kb_first_launch_verifier;
 REVOKE EXECUTE ON FUNCTION pg_catalog.kb_finalize_first_launch_privileges()
  FROM kb_migrator,kb_first_launch_verifier;
 ALTER ROLE kb_migrator NOLOGIN NOINHERIT PASSWORD NULL NOCREATEROLE;
 ALTER ROLE kb_first_launch_verifier NOLOGIN NOINHERIT PASSWORD NULL NOCREATEROLE;
END $finalizer$;
REVOKE ALL ON FUNCTION pg_catalog.kb_finalize_first_launch_privileges() FROM PUBLIC,kb_migrator,kb_first_launch_verifier;

GRANT kb_launch_owner TO kb_migrator
 WITH ADMIN FALSE, INHERIT FALSE, SET TRUE;
GRANT kb_app_owner,kb_launch_owner TO kb_first_launch_verifier
 WITH ADMIN FALSE, INHERIT FALSE, SET TRUE;

DO $launch_topology$
BEGIN
 IF EXISTS(
  SELECT 1 FROM pg_catalog.pg_roles role_value
  WHERE role_value.rolname=ANY(ARRAY[
   'kb_app_owner','kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
   'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'])
    AND (role_value.rolcanlogin
         OR role_value.rolinherit IS DISTINCT FROM
            (role_value.rolname NOT IN ('kb_app_owner','kb_launch_owner'))
         OR NOT pg_catalog.kb_launch_role_password_absent(role_value.rolname)
         OR EXISTS(SELECT 1 FROM pg_catalog.pg_db_role_setting setting_value
                   WHERE setting_value.setrole=role_value.oid))
 ) OR (SELECT count(*) FROM pg_catalog.pg_auth_members membership
       JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
       JOIN pg_catalog.pg_roles member ON member.oid=membership.member
       WHERE granted.rolname=ANY(ARRAY[
        'kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
        'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'])
          OR member.rolname=ANY(ARRAY[
        'kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
        'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'])) <> 2
 OR (SELECT count(*) FROM pg_catalog.pg_auth_members membership
     JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
     JOIN pg_catalog.pg_roles member ON member.oid=membership.member
     WHERE (granted.rolname,member.rolname) IN (
       ('kb_launch_owner','kb_migrator'),
       ('kb_launch_owner','kb_first_launch_verifier'),
       ('kb_app_owner','kb_first_launch_verifier'))
       AND NOT membership.admin_option AND NOT membership.inherit_option
       AND membership.set_option) <> 3
 OR (SELECT count(*) FROM pg_catalog.pg_auth_members membership
     JOIN pg_catalog.pg_roles granted ON granted.oid=membership.roleid
     JOIN pg_catalog.pg_roles member ON member.oid=membership.member
     WHERE granted.rolname=ANY(ARRAY[
       'kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention',
       'kb_app_owner','kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
       'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'])
        OR member.rolname=ANY(ARRAY[
       'kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention',
       'kb_app_owner','kb_launch_owner','kb_launch_operator','kb_launch_router','kb_launch_ingress',
       'kb_launch_attestor','kb_launch_signature_verifier','kb_launch_reset_dispatcher'])) <> 3
 THEN
  RAISE EXCEPTION 'fixed launch-role membership/password topology mismatch' USING ERRCODE='42501';
 END IF;
END $launch_topology$;

SELECT format('ALTER DATABASE %I OWNER TO kb_migrator',current_database()) \gexec
ALTER SCHEMA public OWNER TO kb_migrator;
REVOKE ALL ON DATABASE :"DBNAME" FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
-- First launch opens new migrator connections and creates temporary migration
-- state before the one-shot handoff revokes all migrator database authority.
GRANT CONNECT,CREATE,TEMPORARY ON DATABASE :"DBNAME" TO kb_migrator;
GRANT CONNECT ON DATABASE :"DBNAME"
 TO kb_first_launch_verifier,kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
GRANT USAGE ON SCHEMA public
 TO kb_first_launch_verifier,kb_runtime_api,kb_runtime_worker,kb_runtime_retention;

-- Runtime authority is relation-by-relation in migration 0010. Keep the
-- migrator's default ACL empty so a future control object cannot inherit access.
ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator IN SCHEMA public
 REVOKE ALL ON TABLES FROM kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator IN SCHEMA public
 REVOKE ALL ON SEQUENCES FROM kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
ALTER DEFAULT PRIVILEGES FOR ROLE kb_migrator IN SCHEMA public
 REVOKE ALL ON FUNCTIONS FROM kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
SQL
