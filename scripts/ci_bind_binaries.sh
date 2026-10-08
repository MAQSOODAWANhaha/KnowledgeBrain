#!/usr/bin/env bash
# Point later steps at the downloaded dev binaries, whatever directory the artifact used.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
base=$root/${CI_BIN_DIR:?}
if [[ ! -f "$base/manifest.txt" ]]; then
  alt=$(find "$base" -name manifest.txt -print -quit || true)
  if [[ -n "$alt" ]]; then
    base=$(dirname "$alt")
  fi
fi
[[ -f "$base/manifest.txt" ]] || {
  echo "packaged Rust binaries are missing under $root/${CI_BIN_DIR}" >&2
  find "$root/${CI_BIN_DIR}" -maxdepth 4 -type f >&2 || true
  exit 1
}
{
  printf 'CI_BIN_ROOT=%s\n' "$base"
  printf 'KB_MIGRATOR_BIN=%s\n' "$base/bins/migrator"
  printf 'KB_SCHEMA_VERIFIER_BIN=%s\n' "$base/bins/schema-verifier"
  printf 'KB_API_BIN=%s\n' "$base/bins/api"
  printf 'KB_WORKER_BIN=%s\n' "$base/bins/worker"
  printf 'KB_DOCREADER_REPLAY_BIN=%s\n' "$base/tests/tender_document_process_real_parse_counts"
} >> "${GITHUB_ENV:?}"
chmod +x "$base/bins/"* "$base/tests/"*
