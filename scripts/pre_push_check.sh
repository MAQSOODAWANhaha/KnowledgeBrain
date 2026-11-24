#!/usr/bin/env bash
# Fast local pre-push gate: runs repo guards plus stack checks for changed areas.
# Usage:
#   bash scripts/pre_push_check.sh                 # check files changed vs origin/main
#   bash scripts/pre_push_check.sh --all           # check all stacks (rust, web, python)
# Optional git hook:
#   ln -sf ../../scripts/pre_push_check.sh .git/hooks/pre-push
set -euo pipefail

root=$(git rev-parse --show-toplevel)
cd "$root"

run_all=0
if [[ "${1:-}" == "--all" ]]; then
  run_all=1
fi

if [[ "$run_all" == 1 ]]; then
  changed=$'crates/\nweb/\nservices/docreader/parser/\nservices/docreader/'
else
  base_ref="${PRE_PUSH_BASE:-origin/main}"
  if git rev-parse --verify --quiet "$base_ref" >/dev/null; then
    changed=$(git diff --name-only "${base_ref}...HEAD"; git status --porcelain | awk '{print $NF}')
  else
    changed=$(git diff --name-only HEAD~1..HEAD 2>/dev/null || true)
  fi
fi

echo "==> [1/4] CI sources & documentation guard"
(
  set -a
  # shellcheck disable=SC1091
  source .github/ci.env
  set +a
  DEFAULT_BRANCH="${DEFAULT_BRANCH:-$CI_PUSH_BRANCH}" bash scripts/ci_check_sources.sh
)
python3 -m unittest scripts/tests/test_ci_check_docs.py

if printf '%s\n' "$changed" | grep -Eq '(^crates/|^Cargo\.(toml|lock)$)'; then
  echo "==> [2/4] Rust format & clippy"
  cargo fmt --all -- --check
  cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
else
  echo "==> [2/4] Rust unchanged, skipping"
fi

if printf '%s\n' "$changed" | grep -Eq '^web/'; then
  echo "==> [3/4] Web lockfile, lint, unit tests & build"
  npm ci --dry-run --prefix web >/dev/null
  npm run lint --prefix web
  npm test --prefix web
  npm run build --prefix web
else
  echo "==> [3/4] Web unchanged, skipping"
fi

if printf '%s\n' "$changed" | grep -Eq '^services/docreader/'; then
  echo "==> [4/4] Python ruff & parser fixture"
  (
    cd services/docreader
    uvx ruff check .
    if printf '%s\n' "$changed" | grep -Eq '^services/docreader/parser/'; then
      PYTHONPATH="${root}/services" uv run --frozen --no-sync pytest tests/test_excel_wire_fixture.py -q
    fi
  )
else
  echo "==> [4/4] Python unchanged, skipping"
fi

echo "pre_push_check: OK"
