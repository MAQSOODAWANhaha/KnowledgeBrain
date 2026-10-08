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

if [[ "$CI_PUSH_BRANCH" != "$DEFAULT_BRANCH" ]]; then
  echo "CI_PUSH_BRANCH=$CI_PUSH_BRANCH is not the repository default branch $DEFAULT_BRANCH" >&2
  exit 1
fi
grep -Fq "branches: [${CI_PUSH_BRANCH}]" .github/workflows/ci.yml

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
