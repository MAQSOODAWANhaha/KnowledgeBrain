#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: mode-aware-probe.sh kind=startup|liveness|readiness target=api|worker
       mode-aware-probe.sh --kind KIND --target TARGET
       mode-aware-probe.sh --help

startup/liveness: GET /live (API may use /health)
readiness: GET /ready
Exit 0 on HTTP 200 only, otherwise 1.
EOF
}

KIND=""
TARGET=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --kind)
      KIND="${2:-}"
      shift 2
      ;;
    --target)
      TARGET="${2:-}"
      shift 2
      ;;
    --kind=*)
      KIND="${1#--kind=}"
      shift
      ;;
    --target=*)
      TARGET="${1#--target=}"
      shift
      ;;
    kind=*)
      KIND="${1#kind=}"
      shift
      ;;
    target=*)
      TARGET="${1#target=}"
      shift
      ;;
    *)
      usage >&2
      exit 1
      ;;
  esac
done

case "$KIND" in
  startup|liveness|readiness) ;;
  *)
    usage >&2
    exit 1
    ;;
esac

case "$TARGET" in
  api|worker) ;;
  *)
    usage >&2
    exit 1
    ;;
esac

api_base() {
  if [[ -n "${KNOWLEDGEBRAIN_API_PROBE_BASE:-}" ]]; then
    printf '%s\n' "${KNOWLEDGEBRAIN_API_PROBE_BASE%/}"
    return
  fi
  printf 'http://127.0.0.1:%s\n' "${API_PORT:-8080}"
}

worker_base() {
  if [[ -n "${KNOWLEDGEBRAIN_WORKER_PROBE_BASE:-}" ]]; then
    printf '%s\n' "${KNOWLEDGEBRAIN_WORKER_PROBE_BASE%/}"
    return
  fi
  local addr="${KNOWLEDGEBRAIN_WORKER_PROBE_ADDR:-127.0.0.1:8081}"
  printf 'http://%s\n' "$addr"
}

hit() {
  local url="$1"
  curl -fsS -o /dev/null --max-time 3 "$url"
}

if [[ "$TARGET" == api ]]; then
  BASE="$(api_base)"
else
  BASE="$(worker_base)"
fi

if [[ "$KIND" == readiness ]]; then
  hit "$BASE/ready"
  exit 0
fi

if [[ "$TARGET" == api ]]; then
  if hit "$BASE/live"; then
    exit 0
  fi
  hit "$BASE/health"
  exit 0
fi

hit "$BASE/live"
exit 0
