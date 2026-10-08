#!/usr/bin/env bash
# Fail when CI literals drift from .github/ci.env or the repository default branch.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
: "${CI_PUSH_BRANCH:?}"
: "${DEFAULT_BRANCH:?}"
: "${CI_POSTGRES_PORT:?}"
: "${CI_REDIS_PORT:?}"
: "${CI_CONTENT_POSTGRES_TESTS:?}"
: "${CI_REQUEST_DELIVERY_TEST:?}"
: "${CI_CONTRACT_FEATURES:?}"
: "${CI_DOCREADER_FEATURE:?}"
: "${CI_DOCREADER_PYTEST_PATHS:?}"
: "${CI_SAMPLE_SOURCE_PYTEST:?}"
: "${CI_DOCREADER_PYTHONPATH:?}"
: "${CI_RUNNER:?}"
: "${CI_ACTION_CHECKOUT:?}"
: "${CI_ACTION_SETUP_NODE:?}"
: "${CI_ACTION_SETUP_PYTHON:?}"
: "${CI_ACTION_UPLOAD_ARTIFACT:?}"
: "${CI_ACTION_DOWNLOAD_ARTIFACT:?}"
: "${CI_ACTION_DOCKER_LOGIN:?}"
: "${CI_ACTION_DOCKER_BUILDX:?}"
: "${CI_ACTION_DOCKER_BUILD_PUSH:?}"
: "${CI_ACTION_SETUP_UV:?}"
: "${CI_ACTION_RUST_TOOLCHAIN:?}"
: "${CI_ACTION_RUST_CACHE:?}"

if [[ "$CI_PUSH_BRANCH" != "$DEFAULT_BRANCH" ]]; then
  echo "CI_PUSH_BRANCH=$CI_PUSH_BRANCH is not the repository default branch $DEFAULT_BRANCH" >&2
  exit 1
fi
grep -Fq "branches: [${CI_PUSH_BRANCH}]" .github/workflows/ci.yml

workflow=.github/workflows/ci.yml
anchor=0
alias=0
while IFS= read -r line; do
  case "$line" in
    *"runs-on: &runner ${CI_RUNNER}") anchor=$((anchor + 1)) ;;
    *"runs-on: *runner") alias=$((alias + 1)) ;;
    *)
      echo "runs-on is not the documented runner: $line" >&2
      exit 1
      ;;
  esac
done < <(grep -E '^[[:space:]]*runs-on:' "$workflow")
if [[ "$anchor" != 1 || "$alias" -lt 1 ]]; then
  echo "expected one &runner ${CI_RUNNER} and aliases, found anchor=$anchor alias=$alias" >&2
  exit 1
fi
if grep -Fq 'ubuntu-latest' "$workflow"; then
  echo "workflow still uses ubuntu-latest" >&2
  exit 1
fi

action_refs=(
  "$CI_ACTION_CHECKOUT"
  "$CI_ACTION_SETUP_NODE"
  "$CI_ACTION_SETUP_PYTHON"
  "$CI_ACTION_UPLOAD_ARTIFACT"
  "$CI_ACTION_DOWNLOAD_ARTIFACT"
  "$CI_ACTION_DOCKER_LOGIN"
  "$CI_ACTION_DOCKER_BUILDX"
  "$CI_ACTION_DOCKER_BUILD_PUSH"
  "$CI_ACTION_SETUP_UV"
  "$CI_ACTION_RUST_TOOLCHAIN"
  "$CI_ACTION_RUST_CACHE"
)
for ref in "${action_refs[@]}"; do
  grep -Fq "uses: ${ref}" "$workflow" || {
    echo "workflow is missing ${ref}" >&2
    exit 1
  }
done
while IFS= read -r line; do
  ref=${line##*uses: }
  ref=${ref%%[[:space:]]*}
  known=0
  for expected in "${action_refs[@]}"; do
    if [[ "$ref" == "$expected" ]]; then
      known=1
      break
    fi
  done
  if [[ "$known" != 1 ]]; then
    echo "workflow uses undocumented action ${ref}" >&2
    exit 1
  fi
done < <(grep -E '^[[:space:]]*(- )?uses:' "$workflow")

case " ${CI_CONTENT_POSTGRES_TESTS} " in
  *" ${CI_REQUEST_DELIVERY_TEST} "*) ;;
  *)
    echo "CI_REQUEST_DELIVERY_TEST is not in CI_CONTENT_POSTGRES_TESTS" >&2
    exit 1
    ;;
esac
case ",${CI_CONTRACT_FEATURES}," in
  *",${CI_DOCREADER_FEATURE},"*) ;;
  *)
    echo "CI_DOCREADER_FEATURE is not in CI_CONTRACT_FEATURES" >&2
    exit 1
    ;;
esac

grep -Fq "url.port != ${CI_POSTGRES_PORT}" scripts/fresh_schema_acceptance.sh
grep -Fq "get_port() == ${CI_POSTGRES_PORT}" crates/bidding/tests/support/mod.rs
grep -Fq "127.0.0.1:${CI_POSTGRES_PORT}" scripts/bidding_v2_phase_fixture_acceptance.sh
grep -Fq "127.0.0.1:${CI_POSTGRES_PORT}:5432" scripts/bidding_v2_content_stack_e2e.sh
grep -Fq "redis_port=\$((${CI_REDIS_PORT} + offset))" scripts/bidding_v2_content_stack_e2e.sh
grep -Fq 'Path("deploy/images.lock.json")' scripts/bidding_v2_content_stack_e2e.sh
sample_test=$(realpath -m "services/docreader/${CI_SAMPLE_SOURCE_PYTEST}")
docreader_tests=$(realpath -m "services/docreader/${CI_DOCREADER_PYTEST_PATHS}")
[[ -f "$sample_test" ]] || { echo "missing sample-source test $sample_test" >&2; exit 1; }
[[ -d "$docreader_tests" ]] || { echo "missing docreader pytest path $docreader_tests" >&2; exit 1; }
if grep -Fq 'pgvector/pgvector@sha256:' scripts/bidding_v2_content_stack_e2e.sh \
  || grep -Fq 'redis@sha256:' scripts/bidding_v2_content_stack_e2e.sh; then
  echo "content e2e repeats an image digest that belongs in deploy/images.lock.json" >&2
  exit 1
fi
