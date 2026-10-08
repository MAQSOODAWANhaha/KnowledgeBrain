#!/usr/bin/env bash
# Copy the dev-profile binaries and integration-test executables built in this job.
# Later jobs run these files and do not invoke cargo.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
: "${CI_BIN_DIR:?}"
: "${CI_CONTRACT_FEATURES:?}"
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

copy_bin api
copy_bin worker
copy_bin migrator
copy_bin schema-verifier

copy_tests() {
  local package=$1
  local feature_mode=$2
  shift 2
  local -a tests=("$@")
  local -a cargo_args=(--locked --no-run --message-format=json -p "$package")
  local -a selectors=()
  local test_name json exe
  if [[ "$feature_mode" == features ]]; then
    cargo_args+=(--features "$CI_CONTRACT_FEATURES")
  fi
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

read -r -a bidding_tests <<< "${CI_DEFAULT_BIDDING_TESTS:?}"
read -r -a platform_tests <<< "${CI_DEFAULT_PLATFORM_TESTS:?}"
read -r -a feature_tests <<< "${CI_FEATURE_BIDDING_TESTS:?}"
copy_tests bidding default "${bidding_tests[@]}"
copy_tests platform default "${platform_tests[@]}"
copy_tests bidding features "${feature_tests[@]}"

{
  printf '%s\n' "bins:"
  find "$out/bins" -type f -printf '  %f\n' | sort
  printf '%s\n' "tests:"
  find "$out/tests" -type f -printf '  %f\n' | sort
} | tee "$out/manifest.txt"
