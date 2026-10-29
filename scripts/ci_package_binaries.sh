#!/usr/bin/env bash
# Copy the dev-profile binaries and integration-test executables built in this job.
# Later jobs run these files and do not invoke cargo.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
: "${CI_BIN_DIR:?}"
: "${CI_API_BIN:?}"
: "${CI_WORKER_BIN:?}"
: "${CI_MIGRATOR_BIN:?}"
: "${CI_KNOWLEDGE_WORKFLOW_TESTS:?}"
: "${CI_OXANA_TEST:?}"
: "${CI_FRESH_TEST:?}"
out=$root/$CI_BIN_DIR
rm -rf "$out"
mkdir -p "$out/bins" "$out/tests"

target=$(cargo metadata --locked --format-version=1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')

copy_bin() {
  local name=$1
  local src=$target/debug/$name
  [[ -f "$src" ]] || { echo "missing runtime binary $src" >&2; exit 1; }
  cp "$src" "$out/bins/$name"
  chmod +x "$out/bins/$name"
}

copy_bin "$CI_API_BIN"
copy_bin "$CI_WORKER_BIN"
copy_bin "$CI_MIGRATOR_BIN"

copy_tests() {
  local package=$1
  shift
  local -a tests=("$@")
  local -a cargo_args=(--locked --no-run --message-format=json -p "$package")
  local -a selectors=()
  local test_name json exe
  for test_name in "${tests[@]}"; do
    selectors+=(--test "$test_name")
  done
  json=$(cargo test "${cargo_args[@]}" "${selectors[@]}")
  for test_name in "${tests[@]}"; do
    exe=$(printf '%s\n' "$json" | python3 -c '
import json, sys
name = sys.argv[1]
found = None
for line in sys.stdin:
    line = line.strip()
    if not line.startswith("{"):
        continue
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        continue
    target = msg.get("target") or {}
    if msg.get("reason") == "compiler-artifact" and name == target.get("name") and "test" in (target.get("kind") or []) and msg.get("executable"):
        found = msg["executable"]
if not found:
    raise SystemExit(f"no test executable for {name}")
print(found)
' "$test_name" || true)
    if [[ -z "${exe:-}" || ! -f "$exe" ]]; then
      exe=$(find "$target/debug/deps" -maxdepth 1 -type f -name "${test_name}-*" ! -name '*.*' -print -quit)
    fi
    [[ -n "${exe:-}" && -f "$exe" ]] || { echo "no test executable for $test_name" >&2; exit 1; }
    cp "$exe" "$out/tests/$test_name"
    chmod +x "$out/tests/$test_name"
  done
}

read -r -a knowledge_tests <<< "$CI_KNOWLEDGE_WORKFLOW_TESTS"
copy_tests knowledge "${knowledge_tests[@]}" vector_index_postgres
copy_tests platform "$CI_OXANA_TEST" "$CI_FRESH_TEST"
copy_tests api owner_access_postgres

{
  printf '%s\n' "bins:"
  find "$out/bins" -type f -printf '  %f\n' | sort
  printf '%s\n' "tests:"
  find "$out/tests" -type f -printf '  %f\n' | sort
} | tee "$out/manifest.txt"
