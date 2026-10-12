#!/usr/bin/env python3
"""Run local tests with a minimal child environment, without inherited service credentials.

Example: python scripts/run_local_tests.py -- cargo test -p knowledge --lib -- --test-threads=1
This isolates inherited configuration; it is not a network sandbox. Tests may
start their own loopback services. Opt-in service tests remain opt-in.
"""
import os
import subprocess
import sys

# Deliberately allowlist process/tool settings rather than matching credential names.
LOCAL_PROCESS_KEYS = frozenset({
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "TMP", "TEMP",
    "LANG", "LC_ALL", "LC_CTYPE", "TERM", "NO_COLOR", "TZ",
    "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "CARGO_TARGET_DIR",
    "UV_CACHE_DIR", "UV_PROJECT_ENVIRONMENT", "VIRTUAL_ENV",
})


def isolated_environment(source):
    return {key: value for key, value in source.items() if key in LOCAL_PROCESS_KEYS}


def main():
    command = sys.argv[1:]
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        raise SystemExit("usage: run_local_tests.py -- COMMAND [ARG ...]")
    return subprocess.run(command, env=isolated_environment(os.environ)).returncode


if __name__ == "__main__":
    raise SystemExit(main())
