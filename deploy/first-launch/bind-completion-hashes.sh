#!/usr/bin/env bash
# Compute first-launch bind hashes. Read-only: never writes
# runtime-completion.toml and never flips phase_1d_runtime_complete.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

REGISTRY="deploy/queue-registry.toml"
EVALUATION="deploy/eval/first-launch-evaluation.toml"
READINESS="deploy/health/mode-aware-probe.sh"
IMAGES="deploy/images.lock.json"
TOPOLOGY="deploy/first-launch/topology-inputs.toml"
ALL_ZERO="0000000000000000000000000000000000000000000000000000000000000000"

file_sha256() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "bind refuses: missing $path" >&2
    exit 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  else
    shasum -a 256 "$path" | awk '{print $1}'
  fi
}

REGISTRY_SHA="$(file_sha256 "$REGISTRY")"
EVALUATION_SHA="$(file_sha256 "$EVALUATION")"
READINESS_SHA="$(file_sha256 "$READINESS")"
IMAGES_SHA="$(file_sha256 "$IMAGES")"
TOPOLOGY_SHA="$(file_sha256 "$TOPOLOGY")"

printf 'registry_sha256=%s\n' "$REGISTRY_SHA"
printf 'evaluation_sha256=%s\n' "$EVALUATION_SHA"
printf 'readiness_sha256=%s\n' "$READINESS_SHA"
printf 'images_sha256=%s\n' "$IMAGES_SHA"
printf 'topology_sha256=%s\n' "$TOPOLOGY_SHA"

failed=0
for digest in "$REGISTRY_SHA" "$EVALUATION_SHA" "$READINESS_SHA" "$IMAGES_SHA" "$TOPOLOGY_SHA"; do
  if [[ "$digest" == "$ALL_ZERO" ]]; then
    echo "bind refuses: hash would be all-zero" >&2
    failed=1
  fi
done

python3 - "$EVALUATION" "$IMAGES" <<'PY' || failed=1
import json, sys, tomllib

evaluation_path, images_path = sys.argv[1], sys.argv[2]
try:
    with open(evaluation_path, "rb") as handle:
        evaluation = tomllib.load(handle)
except Exception as error:
    print(f"bind refuses: evaluation unreadable: {error}", file=sys.stderr)
    sys.exit(1)
if evaluation.get("status") != "passed":
    print("bind refuses: evaluation status is not passed", file=sys.stderr)
    sys.exit(1)

try:
    with open(images_path, encoding="utf-8") as handle:
        lock = json.load(handle)
except Exception as error:
    print(f"bind refuses: image lock unreadable: {error}", file=sys.stderr)
    sys.exit(1)

def is_signed(entry):
    if not isinstance(entry, dict) or entry.get("unsigned"):
        return False
    image = entry.get("image") or ""
    if "@sha256:" not in image:
        return False
    repository, digest = image.split("@sha256:", 1)
    return (
        bool(repository)
        and ":" not in repository
        and "@" not in repository
        and len(digest) == 64
        and all(ch in "0123456789abcdefABCDEF" for ch in digest)
    )

def has_signed_lock_id(entries, lock_id):
    return any(
        isinstance(entry, dict)
        and entry.get("lock_id") == lock_id
        and is_signed(entry)
        for entry in entries
    )

def is_production_complete(lock):
    platforms = lock.get("platforms") or {}
    if not platforms:
        return False
    for platform in platforms.values():
        if not isinstance(platform, dict):
            return False
        runtime = platform.get("runtime_deployable") or []
        build = platform.get("build_base") or []
        if not runtime or not build:
            return False
        if not all(is_signed(entry) for entry in list(runtime) + list(build)):
            return False
        if not has_signed_lock_id(runtime, "api") or not has_signed_lock_id(runtime, "worker"):
            return False
    return True

if not is_production_complete(lock):
    print("bind refuses: image lock is not production complete", file=sys.stderr)
    sys.exit(1)
PY

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi
