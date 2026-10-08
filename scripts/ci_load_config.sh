#!/usr/bin/env bash
# Load .github/ci.env into the current shell and, on Actions, into GITHUB_ENV.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
file=$root/.github/ci.env
set -a
# shellcheck disable=SC1090
source "$file"
set +a
if [[ -n "${GITHUB_ENV:-}" ]]; then
  while IFS= read -r line || [[ -n "$line" ]]; do
    case "$line" in
      ''|\#*) continue ;;
    esac
    key=${line%%=*}
    printf '%s=%s\n' "$key" "${!key}" >> "$GITHUB_ENV"
  done < "$file"
fi
