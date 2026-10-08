#!/usr/bin/env bash
# Prove the one-shot outline test and the retirement module ran inside the workspace suite.
# The workspace log is the only execution; this does not start cargo again.
set -euo pipefail
log=${1:?workspace test log}
one_shot=${ONE_SHOT_TEST:?}
prefix=${RETIREMENT_TEST_PREFIX:?}
if ! grep -Eq "^test ${one_shot} \\.\\.\\. ok$" "$log"; then
  echo "required test did not pass: $one_shot" >&2
  exit 1
fi
mapfile -t lines < <(grep -E "^test ${prefix}" "$log" || true)
if ((${#lines[@]} < 1)); then
  echo "no tests matched $prefix" >&2
  exit 1
fi
for line in "${lines[@]}"; do
  if [[ "$line" != *" ... ok" ]]; then
    echo "retirement test did not pass: $line" >&2
    exit 1
  fi
done
