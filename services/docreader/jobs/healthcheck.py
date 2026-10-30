"""Bounded, read-only readiness checks for the two long-running Python roles.

This verifies a role process, its storage schema and Redis reachability. It does
not prove queue consumption, model availability or successful document output.
"""
import argparse
import json
from pathlib import Path
import re
import signal
import sys

import psycopg
from redis import Redis

from docreader.jobs.settings import Settings

PROBE_TIMEOUT_SECONDS = 8
IO_TIMEOUT_SECONDS = 2
_CORE_QUERIES = (
    "SELECT id,owner_user_id,status FROM tender_projects LIMIT 0",
    "SELECT id,project_id,file_name,media_type,object_ref,content_sha256,byte_length FROM tender_documents LIMIT 0",
    "SELECT run_id,source_version,domain_revision,status,cancel_requested FROM tender_analysis_runs LIMIT 0",
    "SELECT run_id,source_version,manifest_ref FROM tender_analysis_source_versions LIMIT 0",
    "SELECT run_id,source_version,domain_revision,execution_spec,scope_decisions_ref FROM tender_analysis_revisions LIMIT 0",
    "SELECT command_id,run_id,status,claim_token,claim_until FROM tender_analysis_commands LIMIT 0",
    "SELECT run_id,source_version,domain_revision,stage,scope,artifact_ref FROM tender_analysis_artifacts LIMIT 0",
    "SELECT run_id,source_version,domain_revision,checklist_digest,issue_resolutions FROM tender_analysis_confirmations LIMIT 0",
    "SELECT request_id,run_id,status,metadata,response_ref FROM tender_analysis_model_requests LIMIT 0",
    "SELECT execution_id,status,lease_until,cancel_requested,result_ref,receipts FROM tender_authoring_executions LIMIT 0",
    "SELECT request_id,execution_id,status,metadata,usage FROM tender_authoring_model_requests LIMIT 0",
    "SELECT document_id,project_id,original_ref FROM tender_document_sources LIMIT 0",
    "SELECT run_id,project_id,request_key,source_only FROM tender_project_analyses LIMIT 0",
    "SELECT execution_id,project_id,run_id,payload,status,claim_until,resume_keys,created_at FROM tender_authoring_jobs LIMIT 0",
    "SELECT revision_id,project_id,revision,parent_id,execution_id,outline_ref,request_key,request_digest FROM tender_outline_revisions LIMIT 0",
    "SELECT id,project_id,execution_id,revision,parent_version_id,source,object_ref,content_sha256,outline FROM tender_draft_versions LIMIT 0",
    "SELECT id,project_id,base_version_id,owner_user_id,mode,request_key,request_sha256,closed_at FROM tender_editor_sessions LIMIT 0",
    "SELECT project_id,version_id,session_id,pending_save_id,editor_error FROM tender_draft_state LIMIT 0",
    "SELECT id,project_id,session_id,parent_version_id,parent_sha256,state,result_version_id,callback_sha256 FROM tender_draft_saves LIMIT 0",
    "SELECT session_id,callback_sha256,version_id FROM tender_draft_recoveries LIMIT 0",
    "SELECT thread_id,checkpoint_ns,checkpoint_id,checkpoint FROM checkpoints LIMIT 0",
    "SELECT thread_id,checkpoint_ns,channel,version,blob FROM checkpoint_blobs LIMIT 0",
    "SELECT thread_id,checkpoint_ns,checkpoint_id,task_id,idx,channel,blob FROM checkpoint_writes LIMIT 0",
)


def _check_process(role, proc_root=Path("/proc")):
    for directory in proc_root.glob("[0-9]*"):
        try:
            args = directory.joinpath("cmdline").read_bytes().decode(errors="replace").split("\0")
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
        if not re.fullmatch(r"python(?:\d+(?:\.\d+)?)?", Path(args[0]).name):
            continue
        if role == "worker" and "docreader.jobs.tasks" in args and any(Path(arg).name == "dramatiq" for arg in args):
            return
        if role == "dispatcher" and any(args[index:index + 2] == ["-m", "docreader.jobs.dispatcher"] for index in range(len(args) - 1)):
            return
    raise RuntimeError("expected Python role process is not running")


def _check_database(dsn):
    with psycopg.connect(dsn, connect_timeout=IO_TIMEOUT_SECONDS) as conn:
        conn.read_only = True
        conn.execute("SET LOCAL statement_timeout = '2000ms'")
        for query in _CORE_QUERIES:
            conn.execute(query)


def _check_redis(url):
    with Redis.from_url(url, socket_connect_timeout=IO_TIMEOUT_SECONDS, socket_timeout=IO_TIMEOUT_SECONDS) as client:
        if client.ping() is not True:
            raise RuntimeError("Redis did not answer PING")


def probe(role):
    _check_process(role)
    settings = Settings.from_env()
    _check_database(settings.dsn)
    _check_redis(settings.redis_url)


def _deadline(_signum, _frame):
    raise TimeoutError("health probe deadline exceeded")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("role", choices=("worker", "dispatcher"))
    args = parser.parse_args(argv)
    previous = signal.signal(signal.SIGALRM, _deadline)
    signal.alarm(PROBE_TIMEOUT_SECONDS)
    try:
        probe(args.role)
    except Exception as error:
        print(json.dumps({"role": args.role, "healthy": False, "error": type(error).__name__}), file=sys.stderr)
        return 1
    finally:
        signal.alarm(0)
        signal.signal(signal.SIGALRM, previous)
    print(json.dumps({"role": args.role, "healthy": True}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
