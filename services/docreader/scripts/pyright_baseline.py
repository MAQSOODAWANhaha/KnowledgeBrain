"""Fail only on pyright errors that are not in the recorded baseline.

Usage (from services/docreader):
  uv run --with pyright python scripts/pyright_baseline.py           # check
  uv run --with pyright python scripts/pyright_baseline.py --update  # rewrite baseline after a cleanup

Errors are keyed by file, rule and first message line, without line numbers, so
unrelated edits do not move the baseline. A key may only keep or lower its count.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
BASELINE = ROOT / "pyright-baseline.json"


def current():
    run = subprocess.run(["pyright", ".", "--outputjson"], cwd=ROOT, capture_output=True, text=True)
    report = json.loads(run.stdout)
    errors = Counter()
    for item in report["generalDiagnostics"]:
        if item["severity"] != "error":
            continue
        path = Path(item["file"]).resolve().relative_to(ROOT).as_posix()
        errors["\t".join((path, item.get("rule", ""), item["message"].splitlines()[0]))] += 1
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--update", action="store_true", help="record the current errors as the baseline")
    args = parser.parse_args()
    errors = current()
    if args.update:
        BASELINE.write_text(json.dumps(dict(sorted(errors.items())), ensure_ascii=False, indent=0) + "\n")
        print(f"baseline: {sum(errors.values())} errors")
        return 0
    baseline = Counter(json.loads(BASELINE.read_text()))
    new = errors - baseline
    for key, count in sorted(new.items()):
        print(f"+{count}\t{key}")
    print(f"pyright: {sum(errors.values())} errors, baseline {sum(baseline.values())}, new {sum(new.values())}")
    return 1 if new else 0


if __name__ == "__main__":
    sys.exit(main())
