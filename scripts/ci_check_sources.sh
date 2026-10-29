#!/usr/bin/env bash
# Fail when CI literals drift from .github/ci.env or the repository default branch.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
: "${CI_PUSH_BRANCH:?}"
: "${DEFAULT_BRANCH:?}"
: "${CI_POSTGRES_PORT:?}"
: "${CI_REDIS_PORT:?}"
: "${CI_DOCREADER_PYTEST_PATHS:?}"
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

docreader_tests=$(realpath -m "services/docreader/${CI_DOCREADER_PYTEST_PATHS}")
[[ -d "$docreader_tests" ]] || { echo "missing docreader pytest path $docreader_tests" >&2; exit 1; }
if rg -n 'crates/bidding|bidding_v2_baseline|CI_TENDER_PREPARE_BIN|bid-authoring-v2' Cargo.toml deploy/queue-registry.toml .github/workflows/ci.yml; then
  echo "retired Rust tender workflow remains in active build configuration" >&2
  exit 1
fi
