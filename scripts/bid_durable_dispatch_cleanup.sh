#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
RECEIPT_DIR=${BID_DURABLE_DISPATCH_CLEANUP_RECEIPT_DIR:-$ROOT/.artifacts/bid-durable-dispatch/cleanup}
REQUIRED_MODES="success failure timeout cancel SIGINT SIGTERM"
BASE_IMAGE=${BID_DURABLE_DISPATCH_CLEANUP_BASE_IMAGE:-redis:7-alpine}

fail() {
  printf '%s\n' "bid durable-dispatch cleanup: $*" >&2
  exit 1
}

sanitize_token() {
  printf '%s' "$1" \
    | tr '[:upper:]_ .' '[:lower:]---' \
    | tr -cd 'a-z0-9-' \
    | cut -c1-28
}

run_token_file() {
  printf '%s/run-token\n' "$RECEIPT_DIR"
}

project_for() {
  mode_token=$(sanitize_token "$1")
  printf 'kb-dd-%s-%s\n' "$RUN_TOKEN" "$mode_token"
}

image_for() {
  mode_token=$(sanitize_token "$1")
  printf 'kb-bid-dd-cleanup:%s-%s\n' "$RUN_TOKEN" "$mode_token"
}

compose_file_for() {
  mode_token=$(sanitize_token "$1")
  printf '%s/%s.compose.yml\n' "$RECEIPT_DIR" "$mode_token"
}

receipt_file_for() {
  mode_token=$(sanitize_token "$1")
  printf '%s/%s.json\n' "$RECEIPT_DIR" "$mode_token"
}

expected_status_for() {
  case "$1" in
    success) printf '0\n' ;;
    failure) printf '42\n' ;;
    timeout) printf '124\n' ;;
    cancel) printf '125\n' ;;
    SIGINT) printf '130\n' ;;
    SIGTERM) printf '143\n' ;;
    *) fail "unknown cleanup mode: $1" ;;
  esac
}

resource_counts() {
  project=$1
  image=$2

  CONTAINERS_REMAINING=$(docker ps -aq \
    --filter "label=com.docker.compose.project=$project" | wc -l | tr -d ' ')
  VOLUMES_REMAINING=$(docker volume ls -q \
    --filter "label=com.docker.compose.project=$project" | wc -l | tr -d ' ')
  NETWORKS_REMAINING=$(docker network ls -q \
    --filter "label=com.docker.compose.project=$project" | wc -l | tr -d ' ')
  if docker image inspect "$image" >/dev/null 2>&1; then
    TEMPORARY_IMAGES_REMAINING=1
  else
    TEMPORARY_IMAGES_REMAINING=0
  fi
}

remove_labeled_resources() {
  project=$1
  fallback_ok=1

  for resource_id in $(docker ps -aq \
    --filter "label=com.docker.compose.project=$project"); do
    docker rm -f -v "$resource_id" >/dev/null 2>&1 || fallback_ok=0
  done
  for resource_id in $(docker volume ls -q \
    --filter "label=com.docker.compose.project=$project"); do
    docker volume rm -f "$resource_id" >/dev/null 2>&1 || fallback_ok=0
  done
  for resource_id in $(docker network ls -q \
    --filter "label=com.docker.compose.project=$project"); do
    docker network rm "$resource_id" >/dev/null 2>&1 || fallback_ok=0
  done

  [ "$fallback_ok" -eq 1 ]
}

write_receipt() {
  mode=$1
  project=$2
  scenario_status=$3
  trap_result=$4
  ci_always_result=$5
  receipt=$(receipt_file_for "$mode")
  temporary="$receipt.tmp.$$"

  {
    printf '{\n'
    printf '  "format_version": 1,\n'
    printf '  "mode": "%s",\n' "$mode"
    printf '  "project_name": "%s",\n' "$project"
    printf '  "scenario_status": %s,\n' "$scenario_status"
    printf '  "containers_remaining": %s,\n' "$CONTAINERS_REMAINING"
    printf '  "volumes_remaining": %s,\n' "$VOLUMES_REMAINING"
    printf '  "networks_remaining": %s,\n' "$NETWORKS_REMAINING"
    printf '  "temporary_images_remaining": %s,\n' "$TEMPORARY_IMAGES_REMAINING"
    printf '  "trap_result": "%s",\n' "$trap_result"
    printf '  "ci_always_result": "%s"\n' "$ci_always_result"
    printf '}\n'
  } >"$temporary"
  mv "$temporary" "$receipt"
}

cleanup_resources() {
  project=$1
  image=$2
  compose_file=$3
  cleanup_ok=1

  if ! docker compose -p "$project" -f "$compose_file" \
    down --volumes --remove-orphans >/dev/null 2>&1; then
    cleanup_ok=0
  fi
  if ! remove_labeled_resources "$project"; then
    cleanup_ok=0
  fi
  if docker image inspect "$image" >/dev/null 2>&1; then
    if ! docker image rm -f "$image" >/dev/null 2>&1; then
      cleanup_ok=0
    fi
  fi

  resource_counts "$project" "$image"
  if [ "$CONTAINERS_REMAINING" -ne 0 ] \
    || [ "$VOLUMES_REMAINING" -ne 0 ] \
    || [ "$NETWORKS_REMAINING" -ne 0 ] \
    || [ "$TEMPORARY_IMAGES_REMAINING" -ne 0 ]; then
    cleanup_ok=0
  fi

  [ "$cleanup_ok" -eq 1 ]
}

write_compose_file() {
  mode=$1
  image=$2
  compose_file=$(compose_file_for "$mode")

  {
    printf 'services:\n'
    printf '  probe:\n'
    printf '    image: %s\n' "$image"
    printf '    command: ["sh", "-c", "while :; do sleep 30; done"]\n'
    printf '    volumes:\n'
    printf '      - scratch:/scratch\n'
    printf 'volumes:\n'
    printf '  scratch: {}\n'
  } >"$compose_file"
}

verify_receipt() {
  mode=$1
  phase=$2
  project=$(project_for "$mode")
  receipt=$(receipt_file_for "$mode")
  expected_status=$(expected_status_for "$mode")

  [ -f "$receipt" ] || fail "missing $phase receipt for $mode"
  grep -Fq "\"mode\": \"$mode\"" "$receipt" \
    || fail "receipt mode mismatch for $mode"
  grep -Fq "\"project_name\": \"$project\"" "$receipt" \
    || fail "receipt project mismatch for $mode"
  grep -Fq "\"scenario_status\": $expected_status" "$receipt" \
    || fail "receipt scenario status mismatch for $mode"
  grep -Fq '"containers_remaining": 0' "$receipt" \
    || fail "container residue recorded for $mode"
  grep -Fq '"volumes_remaining": 0' "$receipt" \
    || fail "volume residue recorded for $mode"
  grep -Fq '"networks_remaining": 0' "$receipt" \
    || fail "network residue recorded for $mode"
  grep -Fq '"temporary_images_remaining": 0' "$receipt" \
    || fail "temporary image residue recorded for $mode"
  grep -Fq '"trap_result": "passed"' "$receipt" \
    || fail "trap cleanup did not pass for $mode"
  if [ "$phase" = ci ]; then
    grep -Fq '"ci_always_result": "passed"' "$receipt" \
      || fail "CI always cleanup did not pass for $mode"
  fi
}

verify_all_receipts() {
  phase=$1
  seen=0
  for mode in $REQUIRED_MODES; do
    verify_receipt "$mode" "$phase"
    seen=$((seen + 1))
  done
  [ "$seen" -eq 6 ] || fail "cleanup must produce exactly six mode receipts"
  receipt_count=$(find "$RECEIPT_DIR" -maxdepth 1 -type f -name '*.json' \
    | wc -l | tr -d ' ')
  [ "$receipt_count" -eq 6 ] \
    || fail "cleanup receipt directory must contain exactly six JSON receipts"
  project_count=$(wc -l <"$RECEIPT_DIR/projects.tsv" | tr -d ' ')
  [ "$project_count" -eq 6 ] \
    || fail "cleanup project manifest must contain exactly six entries"
}

case_cleanup() {
  original_status=$?
  trap - EXIT HUP INT TERM
  if cleanup_resources "$CASE_PROJECT" "$CASE_IMAGE" "$CASE_COMPOSE_FILE"; then
    trap_result=passed
    cleanup_status=$original_status
  else
    trap_result=failed
    cleanup_status=97
  fi
  write_receipt "$CASE_MODE" "$CASE_PROJECT" "$original_status" \
    "$trap_result" pending
  exit "$cleanup_status"
}

run_case_process() {
  CASE_MODE=$1
  CASE_PROJECT=$2
  CASE_IMAGE=$3
  CASE_COMPOSE_FILE=$4

  trap case_cleanup EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM

  docker image inspect "$BASE_IMAGE" >/dev/null 2>&1 \
    || docker pull "$BASE_IMAGE" >/dev/null
  docker image tag "$BASE_IMAGE" "$CASE_IMAGE"
  docker compose -p "$CASE_PROJECT" -f "$CASE_COMPOSE_FILE" up -d >/dev/null

  case "$CASE_MODE" in
    success)
      exit 0
      ;;
    failure)
      exit 42
      ;;
    timeout)
      set +e
      timeout 1s sh -c 'sleep 30'
      timeout_status=$?
      set -e
      [ "$timeout_status" -eq 124 ] || exit 98
      exit 124
      ;;
    cancel)
      cancel_ready="$RECEIPT_DIR/.cancel-ready.$$"
      rm -f "$cancel_ready"
      sh -c '
        trap "exit 77" TERM
        : >"$1"
        while :; do sleep 1; done
      ' sh "$cancel_ready" &
      child=$!
      attempts=0
      while [ ! -f "$cancel_ready" ] && [ "$attempts" -lt 50 ]; do
        sleep 0.02
        attempts=$((attempts + 1))
      done
      [ -f "$cancel_ready" ] || exit 98
      kill -TERM "$child"
      set +e
      wait "$child"
      child_status=$?
      set -e
      rm -f "$cancel_ready"
      [ "$child_status" -eq 77 ] || exit 98
      exit 125
      ;;
    SIGINT)
      kill -INT "$$"
      exit 98
      ;;
    SIGTERM)
      kill -TERM "$$"
      exit 98
      ;;
    *)
      exit 98
      ;;
  esac
}

prepare_exercise() {
  command -v docker >/dev/null 2>&1 || fail "docker is required"
  command -v timeout >/dev/null 2>&1 || fail "timeout is required"
  docker compose version >/dev/null 2>&1 || fail "docker compose is required"

  mkdir -p "$RECEIPT_DIR"
  raw_token=${BID_DURABLE_DISPATCH_CLEANUP_RUN_ID:-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$}
  RUN_TOKEN=$(sanitize_token "$raw_token")
  [ -n "$RUN_TOKEN" ] || fail "cleanup run token is empty after sanitization"
  printf '%s\n' "$RUN_TOKEN" >"$(run_token_file)"
  : >"$RECEIPT_DIR/projects.tsv"

  for mode in $REQUIRED_MODES; do
    receipt=$(receipt_file_for "$mode")
    compose_file=$(compose_file_for "$mode")
    project=$(project_for "$mode")
    image=$(image_for "$mode")
    rm -f "$receipt" "$compose_file"
    write_compose_file "$mode" "$image"
    printf '%s\t%s\t%s\n' "$mode" "$project" "$image" \
      >>"$RECEIPT_DIR/projects.tsv"
  done
}

exercise() {
  prepare_exercise

  for mode in $REQUIRED_MODES; do
    project=$(project_for "$mode")
    image=$(image_for "$mode")
    compose_file=$(compose_file_for "$mode")
    expected_status=$(expected_status_for "$mode")

    set +e
    "$0" __case "$mode" "$project" "$image" "$compose_file"
    actual_status=$?
    set -e
    [ "$actual_status" -eq "$expected_status" ] \
      || fail "$mode exited $actual_status, expected $expected_status"
    verify_receipt "$mode" trap
  done

  verify_all_receipts trap
}

ci_cleanup() {
  command -v docker >/dev/null 2>&1 || fail "docker is required"
  docker compose version >/dev/null 2>&1 || fail "docker compose is required"
  token_file=$(run_token_file)
  [ -f "$token_file" ] || fail "cleanup run token is missing"
  RUN_TOKEN=$(sed -n '1p' "$token_file")
  [ -n "$RUN_TOKEN" ] || fail "cleanup run token is empty"

  ci_ok=1
  for mode in $REQUIRED_MODES; do
    project=$(project_for "$mode")
    image=$(image_for "$mode")
    compose_file=$(compose_file_for "$mode")
    receipt=$(receipt_file_for "$mode")
    expected_status=$(expected_status_for "$mode")

    if [ ! -f "$compose_file" ]; then
      remove_labeled_resources "$project" || true
      if docker image inspect "$image" >/dev/null 2>&1; then
        docker image rm -f "$image" >/dev/null 2>&1 || true
      fi
      ci_ok=0
      continue
    fi
    if cleanup_resources "$project" "$image" "$compose_file"; then
      ci_result=passed
    else
      ci_result=failed
      ci_ok=0
    fi
    if [ -f "$receipt" ]; then
      scenario_status=$(sed -n \
        's/.*"scenario_status": \([0-9][0-9]*\).*/\1/p' "$receipt" \
        | sed -n '1p')
      if grep -Fq '"trap_result": "passed"' "$receipt"; then
        trap_result=passed
      else
        trap_result=failed
        ci_ok=0
      fi
    else
      scenario_status=98
      trap_result=failed
      ci_ok=0
    fi
    if [ "$scenario_status" != "$expected_status" ]; then
      ci_ok=0
    fi
    scenario_status=${scenario_status:-98}
    write_receipt "$mode" "$project" "$scenario_status" \
      "$trap_result" "$ci_result"
  done

  [ "$ci_ok" -eq 1 ] || fail "CI always cleanup found an incomplete or failed trap receipt"
  verify_all_receipts ci
}

case "${1:-}" in
  exercise)
    exercise
    ;;
  ci-cleanup)
    ci_cleanup
    ;;
  __case)
    [ "$#" -eq 5 ] || fail "internal cleanup case requires mode/project/image/compose"
    if [ -f "$(run_token_file)" ]; then
      RUN_TOKEN=$(sed -n '1p' "$(run_token_file)")
    else
      fail "cleanup run token is missing"
    fi
    run_case_process "$2" "$3" "$4" "$5"
    ;;
  *)
    fail "usage: $0 exercise|ci-cleanup"
    ;;
esac
