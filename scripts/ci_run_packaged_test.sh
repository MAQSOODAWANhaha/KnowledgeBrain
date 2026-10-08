#!/usr/bin/env bash
# Run one packaged integration-test binary and require a passing libtest summary.
# Usage: scripts/ci_run_packaged_test.sh <test-name> <log-path> [--unfiltered] -- [harness args]
set -euo pipefail
name=${1:?test name}
log=${2:?log path}
shift 2
unfiltered=0
if [[ "${1:-}" == "--unfiltered" ]]; then
  unfiltered=1
  shift
fi
if [[ "${1:-}" == "--" ]]; then
  shift
fi
root=$(cd "$(dirname "$0")/.." && pwd)
base=${CI_BIN_ROOT:-$root/${CI_BIN_DIR:?}}
bin=$base/tests/$name
[[ -f "$bin" ]] || { echo "missing packaged test $bin" >&2; exit 1; }
chmod +x "$bin"
"$bin" "$@" 2>&1 | tee "$log"
if [[ "$unfiltered" == 1 ]]; then
  pattern='^test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;'
else
  pattern='^test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored;'
fi
grep -Eq "$pattern" "$log"
