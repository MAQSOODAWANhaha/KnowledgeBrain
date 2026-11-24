#!/usr/bin/env python3
"""Fail closed when repository documentation drifts from standards, naming, structure, or source files."""
from __future__ import annotations

import argparse
import pathlib
import re
import sys

IGNORED_DIRS = frozenset({
    ".git",
    ".pytest_cache",
    ".venv",
    "dist",
    "node_modules",
    "target",
    "testdata",
})

ALLOWED_DOCS_TOP_LEVEL_DIRS = frozenset({
    "platform",
    "knowledge-base",
    "tender",
})

ALLOWED_DOCS_TOP_LEVEL_FILES = frozenset({
    "README.md",
    "code-standards.md",
})

RUNTIME_ONLY_PATHS = frozenset({
    "deploy/.env",
    "deploy/release-descriptor-v1.json",
    "web/dist",
})

# Docs that quote forbidden patch phrases as negative examples in rule definitions.
PATCH_PHRASE_EXEMPT_FILES = frozenset({
    "AGENTS.md",
    "LESSONS.md",
    "docs/code-standards.md",
    "docs/README.md",
    "docs/tender/bug-fix.md",
})

LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
BACKTICK_REPO_PATH_RE = re.compile(
    r"`((?:crates|services|web|scripts|migrations|deploy|docs|plans|\.github)/[A-Za-z0-9._/\-*<>]+)`"
)
TMP_PATH_RE = re.compile(r"/tmp/[A-Za-z0-9._/\-]+")
PATCH_PHRASE_RE = re.compile(r"若旧句|本轮尚未|架构草案")
LESSONS_CURSOR_RE = re.compile(r"<!--\s*reviewed-through:\s*\S+.*?-->")
RETIRED_LINE_RE = re.compile(r"已删除|已废止|retired|removed", re.IGNORECASE)
KEBAB_DIR_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
KEBAB_FILE_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*\.[a-z0-9]+$")
FORBIDDEN_DOC_NAME_RE = re.compile(r"(?:^python-|-phase\d+|\d{8})", re.IGNORECASE)
LESSONS_MAX_LINES = 180


def iter_markdown_files(root: pathlib.Path) -> list[pathlib.Path]:
    return sorted(
        p
        for p in root.rglob("*.md")
        if not any(part in IGNORED_DIRS for part in p.relative_to(root).parts)
    )


def strip_fenced_code(text: str) -> str:
    """Replace fenced code blocks with blank lines to preserve line numbers."""
    out: list[str] = []
    in_fence = False
    for line in text.splitlines():
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            out.append("")
            continue
        out.append("" if in_fence else line)
    return "\n".join(out)


def check_layout_and_naming(root: pathlib.Path) -> list[str]:
    errors: list[str] = []
    docs_dir = root / "docs"
    if docs_dir.is_dir():
        for entry in sorted(docs_dir.iterdir()):
            name = entry.name
            if entry.is_dir():
                if name not in ALLOWED_DOCS_TOP_LEVEL_DIRS:
                    errors.append(
                        f"docs/{name}/: disallowed top-level docs directory "
                        f"(allowed: {', '.join(sorted(ALLOWED_DOCS_TOP_LEVEL_DIRS))})"
                    )
            elif entry.is_file():
                if name not in ALLOWED_DOCS_TOP_LEVEL_FILES:
                    errors.append(
                        f"docs/{name}: disallowed top-level file under docs/ "
                        "(move it into platform/, knowledge-base/, or tender/)"
                    )

    for base_name in ("docs", "plans"):
        base_dir = root / base_name
        if not base_dir.is_dir():
            continue
        for entry in sorted(base_dir.rglob("*")):
            rel = entry.relative_to(root).as_posix()
            name = entry.name
            if entry.is_dir():
                if not KEBAB_DIR_RE.match(name) or FORBIDDEN_DOC_NAME_RE.search(name):
                    errors.append(
                        f"{rel}/: directory name must be lowercase kebab-case without language/phase/date tokens"
                    )
            elif entry.is_file():
                if name == "README.md":
                    continue
                if not KEBAB_FILE_RE.match(name) or FORBIDDEN_DOC_NAME_RE.search(name):
                    errors.append(
                        f"{rel}: file name must be lowercase kebab-case without language/phase/date tokens"
                    )
    return errors


def check_repo(root: pathlib.Path) -> list[str]:
    errors: list[str] = []

    # 1. Required workflow files and structure
    claude_md = root / "CLAUDE.md"
    agents_md = root / "AGENTS.md"
    lessons_md = root / "LESSONS.md"
    docs_readme = root / "docs" / "README.md"
    code_standards = root / "docs" / "code-standards.md"

    for req in (claude_md, agents_md, lessons_md, docs_readme, code_standards):
        if not req.is_file():
            errors.append(f"missing required workflow file: {req.relative_to(root)}")

    if claude_md.is_file():
        claude_text = claude_md.read_text(encoding="utf-8")
        for needle in ("@AGENTS.md", "@LESSONS.md"):
            if needle not in claude_text:
                errors.append(f"CLAUDE.md must import {needle}")

    if lessons_md.is_file():
        lessons_text = lessons_md.read_text(encoding="utf-8")
        lessons_lines = lessons_text.splitlines()
        if not LESSONS_CURSOR_RE.search(lessons_text):
            errors.append("LESSONS.md is missing a '<!-- reviewed-through: ... -->' cursor")
        if len(lessons_lines) > LESSONS_MAX_LINES:
            errors.append(
                f"LESSONS.md has {len(lessons_lines)} lines (limit {LESSONS_MAX_LINES}); "
                "promote recurring lessons to docs/code-standards.md or guard tests"
            )

    if docs_readme.is_file():
        docs_readme_text = docs_readme.read_text(encoding="utf-8")
        for needle in ("code-standards.md", "../LESSONS.md"):
            if needle not in docs_readme_text:
                errors.append(f"docs/README.md must link to {needle}")

    # 2. Enforce normalized docs/ and plans/ directory & kebab-case naming rules
    errors.extend(check_layout_and_naming(root))

    md_files = iter_markdown_files(root)
    resolved_links_by_src: dict[pathlib.Path, set[pathlib.Path]] = {}

    for path in md_files:
        rel = path.relative_to(root).as_posix()
        raw_text = path.read_text(encoding="utf-8", errors="ignore")
        prose_text = strip_fenced_code(raw_text)

        # 3. Zero broken relative Markdown links across the entire repository
        targets: set[pathlib.Path] = set()
        for lineno, line in enumerate(prose_text.splitlines(), 1):
            for match in LINK_RE.finditer(line):
                href = match.group(1).strip()
                if not href or href.startswith(("http://", "https://", "mailto:", "#", "file://")):
                    continue
                path_part = href.split("#", 1)[0].split("?", 1)[0]
                if not path_part:
                    continue
                target = (path.parent / path_part).resolve()
                if not target.exists():
                    errors.append(f"{rel}:{lineno}: broken relative link -> {href}")
                else:
                    targets.add(target)
        resolved_links_by_src[path.resolve()] = targets

        # 4. Forbid /tmp/ evidence paths outside historical bug-fix archive
        if rel != "docs/tender/bug-fix.md":
            for lineno, line in enumerate(raw_text.splitlines(), 1):
                m = TMP_PATH_RE.search(line)
                if m:
                    errors.append(
                        f"{rel}:{lineno}: forbidden /tmp path '{m.group(0)}' "
                        "(use /opt/workspace/kb-tender-evidence/ or mktemp without hardcoded /tmp)"
                    )

        # 5. Active doc hygiene: referenced repo paths, obsolete env vars, and patch phrases
        if path.name != "bug-fix.md":
            for lineno, line in enumerate(prose_text.splitlines(), 1):
                if "DOCREADER_PORT" in line:
                    errors.append(
                        f"{rel}:{lineno}: obsolete env var DOCREADER_PORT "
                        "(code reads DOCREADER_GRPC_PORT)"
                    )
                if rel not in PATCH_PHRASE_EXEMPT_FILES:
                    patch_match = PATCH_PHRASE_RE.search(line)
                    if patch_match:
                        errors.append(
                            f"{rel}:{lineno}: forbidden patch/draft phrase '{patch_match.group(0)}' "
                            "(rewrite in present tense instead of patching over stale text)"
                        )
                if RETIRED_LINE_RE.search(line):
                    continue
                for match in BACKTICK_REPO_PATH_RE.finditer(line):
                    ref = match.group(1).rstrip(".,:;/")
                    if any(ch in ref for ch in ("*", "<", ">", "{", "}")):
                        continue
                    if ref in RUNTIME_ONLY_PATHS:
                        continue
                    if not (root / ref).exists():
                        errors.append(f"{rel}:{lineno}: referenced repo path does not exist -> `{ref}`")

    # 6. Every doc under docs/ must be linked from docs/README.md or its directory README.md
    if docs_readme.is_file():
        linked_from_docs_readme = resolved_links_by_src.get(docs_readme.resolve(), set())
        for path in md_files:
            rel_parts = path.relative_to(root).parts
            rel = path.relative_to(root).as_posix()
            if not rel_parts or rel_parts[0] != "docs" or rel == "docs/README.md":
                continue
            resolved = path.resolve()
            dir_readme = (path.parent / "README.md").resolve()
            linked_from_dir = resolved_links_by_src.get(dir_readme, set())
            if resolved not in linked_from_docs_readme and resolved not in linked_from_dir:
                errors.append(
                    f"{rel}: orphan doc under docs/ (link it from docs/README.md or {path.relative_to(root).parent}/README.md)"
                )

    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=pathlib.Path,
        default=pathlib.Path(__file__).resolve().parent.parent,
        help="Repository root directory",
    )
    args = parser.parse_args(argv)
    errors = check_repo(args.root.resolve())
    if errors:
        for err in errors:
            print(f"DOC CHECK FAILED: {err}", file=sys.stderr)
        print(f"Total documentation guard errors: {len(errors)}", file=sys.stderr)
        return 1
    print(f"ci_check_docs: ok ({args.root.resolve()})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
