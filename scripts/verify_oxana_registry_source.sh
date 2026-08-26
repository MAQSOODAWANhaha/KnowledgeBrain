#!/bin/sh
set -eu

usage() {
  echo "usage: $0 [--self-test]" >&2
  exit 2
}

case $# in
  0) mode=verify ;;
  1)
    [ "$1" = "--self-test" ] || usage
    mode=self-test
    ;;
  *) usage ;;
esac

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
metadata_file=$(mktemp "${TMPDIR:-/tmp}/knowledgebrain-oxana-metadata.XXXXXX")
cleanup() {
  rm -f -- "$metadata_file"
}
trap cleanup EXIT HUP INT TERM

cd "$repo_root"
if ! cargo metadata --locked --format-version 1 >"$metadata_file"; then
  echo "oxana registry verification failed: cargo metadata --locked failed" >&2
  exit 1
fi

python3 - "$repo_root" "$metadata_file" "$mode" <<'PY'
from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import sys
import tomllib
from typing import Any, Callable, Iterable


EXPECTED_VERSION = "2.1.3"
EXPECTED_REQUIREMENT = "=2.1.3"
EXPECTED_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
EXPECTED_PACKAGES = {
    "oxana": "bf94eae5bcc69eb7d6950252afa3f316cfa7d769fecc184735a760861eeb01a1",
    "oxana-macros": "4451fc018cae2fdd5fe86041b3807f0c80401ba87a3fa2e04335e28fa3f20cd1",
    "oxana-web": "e9b57c0781b889c6dcab3e3e47ad5aef395d5f95443295c3d3b5a2f7819bebda",
}
REQUIRED_WORKSPACE_DIRECT = {"oxana", "oxana-web"}
DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")
SKIPPED_TREE_PARTS = {".git", "target", "node_modules"}


class ContractError(Exception):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def parse_json(text: str, label: str) -> Any:
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        raise ContractError(f"cannot parse {label} as JSON: {error}") from error


def parse_toml(text: str, label: str) -> dict[str, Any]:
    try:
        value = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise ContractError(f"cannot parse {label} as TOML: {error}") from error
    require(isinstance(value, dict), f"{label} did not parse to a TOML table")
    return value


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise ContractError(f"cannot read {path}: {error}") from error


def validate_package_records(records: Any, label: str, require_checksum: bool) -> None:
    require(isinstance(records, list), f"{label} package collection is not a list")
    for name, checksum in EXPECTED_PACKAGES.items():
        matches = [record for record in records if isinstance(record, dict) and record.get("name") == name]
        require(len(matches) == 1, f"{label} must contain exactly one {name} package, found {len(matches)}")
        package = matches[0]
        require(package.get("version") == EXPECTED_VERSION, f"{label} {name} version is not {EXPECTED_VERSION}")
        require(package.get("source") == EXPECTED_SOURCE, f"{label} {name} source is not crates.io registry")
        observed_checksum = package.get("checksum")
        if require_checksum:
            require(observed_checksum == checksum, f"{label} {name} checksum does not match the frozen receipt")
        elif observed_checksum is not None:
            require(observed_checksum == checksum, f"{label} {name} checksum does not match the frozen receipt")


def validate_dependency_spec(label: str, key: str, spec: Any, allow_workspace: bool) -> str | None:
    if isinstance(spec, str):
        package_name = key
        if package_name in EXPECTED_PACKAGES:
            require(spec == EXPECTED_REQUIREMENT, f"{label} {key} requirement must be {EXPECTED_REQUIREMENT}")
        return package_name if package_name in EXPECTED_PACKAGES else None

    if not isinstance(spec, dict):
        raise ContractError(f"{label} dependency {key} has an unsupported TOML shape")

    package_name = spec.get("package", key)
    if package_name not in EXPECTED_PACKAGES:
        return None

    require("path" not in spec, f"{label} {key} must not use a path dependency")
    require("git" not in spec, f"{label} {key} must not use a git dependency")
    require(spec.get("registry") in (None, "crates-io"), f"{label} {key} must use the crates.io registry")

    if spec.get("workspace") is True:
        require(allow_workspace, f"{label} {key} cannot inherit workspace dependencies here")
        require("version" not in spec, f"{label} {key} mixes workspace inheritance with a version")
    else:
        require(spec.get("version") == EXPECTED_REQUIREMENT, f"{label} {key} requirement must be {EXPECTED_REQUIREMENT}")
    return package_name


def iter_dependency_tables(document: dict[str, Any]) -> Iterable[tuple[str, dict[str, Any], bool]]:
    for table_name in DEPENDENCY_TABLES:
        table = document.get(table_name)
        if table is not None:
            require(isinstance(table, dict), f"{table_name} is not a TOML table")
            yield table_name, table, True

    workspace = document.get("workspace")
    if workspace is not None:
        require(isinstance(workspace, dict), "workspace is not a TOML table")
        table = workspace.get("dependencies")
        if table is not None:
            require(isinstance(table, dict), "workspace.dependencies is not a TOML table")
            yield "workspace.dependencies", table, False

    targets = document.get("target")
    if targets is not None:
        require(isinstance(targets, dict), "target is not a TOML table")
        for target_name, target in targets.items():
            require(isinstance(target, dict), f"target.{target_name} is not a TOML table")
            for table_name in DEPENDENCY_TABLES:
                table = target.get(table_name)
                if table is not None:
                    require(isinstance(table, dict), f"target.{target_name}.{table_name} is not a TOML table")
                    yield f"target.{target_name}.{table_name}", table, True


def validate_manifest_document(path: Path, document: dict[str, Any]) -> set[str]:
    package = document.get("package")
    if package is not None:
        require(isinstance(package, dict), f"{path}: package is not a TOML table")
        require(package.get("name") not in EXPECTED_PACKAGES, f"{path}: repository-local Oxana package is forbidden")

    patch = document.get("patch")
    if patch is not None:
        require(isinstance(patch, dict), f"{path}: patch is not a TOML table")
        require("crates-io" not in patch, f"{path}: [patch.crates-io] is forbidden")
        for source_name, entries in patch.items():
            require(isinstance(entries, dict), f"{path}: patch.{source_name} is not a TOML table")
            for key, spec in entries.items():
                package_name = spec.get("package", key) if isinstance(spec, dict) else key
                require(package_name not in EXPECTED_PACKAGES, f"{path}: patch for {package_name} is forbidden")

    require("replace" not in document, f"{path}: [replace] is forbidden")

    declared: set[str] = set()
    for table_name, table, allow_workspace in iter_dependency_tables(document):
        for key, spec in table.items():
            package_name = validate_dependency_spec(f"{path}:{table_name}", key, spec, allow_workspace)
            if package_name is not None:
                declared.add(package_name)
    return declared


def validate_cargo_config(path: Path, document: dict[str, Any]) -> None:
    require(not document.get("paths"), f"{path}: Cargo path override is forbidden")
    sources = document.get("source")
    if sources is None:
        return
    require(isinstance(sources, dict), f"{path}: source is not a TOML table")
    crates_io = sources.get("crates-io")
    if crates_io is not None:
        require(isinstance(crates_io, dict), f"{path}: source.crates-io is not a TOML table")
        require("replace-with" not in crates_io, f"{path}: crates.io source replacement is forbidden")
    for source_name, source in sources.items():
        require(isinstance(source, dict), f"{path}: source.{source_name} is not a TOML table")
        require("directory" not in source, f"{path}: vendored Cargo source is forbidden")


def validate_metadata_direct_dependencies(metadata: dict[str, Any]) -> None:
    packages = metadata.get("packages")
    members = metadata.get("workspace_members")
    require(isinstance(packages, list), "cargo metadata packages is not a list")
    require(isinstance(members, list), "cargo metadata workspace_members is not a list")
    by_id = {package.get("id"): package for package in packages if isinstance(package, dict)}
    require(len(by_id) == len(packages), "cargo metadata package IDs are missing or duplicated")

    direct: set[str] = set()
    for member_id in members:
        package = by_id.get(member_id)
        require(isinstance(package, dict), f"cargo metadata workspace member {member_id!r} has no package record")
        dependencies = package.get("dependencies")
        require(isinstance(dependencies, list), f"cargo metadata dependencies for {member_id!r} are not a list")
        for dependency in dependencies:
            require(isinstance(dependency, dict), f"cargo metadata dependency for {member_id!r} is not an object")
            name = dependency.get("name")
            if name not in EXPECTED_PACKAGES:
                continue
            direct.add(name)
            require(dependency.get("req") == EXPECTED_REQUIREMENT, f"cargo metadata direct {name} requirement must be {EXPECTED_REQUIREMENT}")
            require(dependency.get("source") == EXPECTED_SOURCE, f"cargo metadata direct {name} source is not crates.io registry")
            require(dependency.get("path") is None, f"cargo metadata direct {name} resolved to a path")
    require(REQUIRED_WORKSPACE_DIRECT <= direct, "cargo metadata is missing required direct oxana/oxana-web dependencies")


def repository_manifests(root: Path) -> list[Path]:
    manifests = []
    for directory, directory_names, file_names in os.walk(root, followlinks=False):
        directory_names[:] = [name for name in directory_names if name not in SKIPPED_TREE_PARTS]
        if "Cargo.toml" in file_names:
            path = Path(directory) / "Cargo.toml"
            require(path.resolve().is_relative_to(root), f"repository manifest escapes the workspace: {path}")
            manifests.append(path)
    require(manifests, "repository contains no Cargo.toml manifests")
    return sorted(manifests)


def repository_cargo_configs(root: Path) -> list[Path]:
    configs = []
    for directory, directory_names, file_names in os.walk(root, followlinks=False):
        directory_names[:] = [name for name in directory_names if name not in SKIPPED_TREE_PARTS]
        if Path(directory).name != ".cargo":
            continue
        for file_name in ("config", "config.toml"):
            if file_name in file_names:
                path = Path(directory) / file_name
                require(path.resolve().is_relative_to(root), f"Cargo config escapes the workspace: {path}")
                configs.append(path)
    return sorted(configs)


def validate_repository(root: Path, metadata_path: Path) -> None:
    metadata = parse_json(read_text(metadata_path), "cargo metadata output")
    require(isinstance(metadata, dict), "cargo metadata output is not an object")
    metadata_root = metadata.get("workspace_root")
    require(isinstance(metadata_root, str), "cargo metadata workspace_root is missing")
    require(Path(metadata_root).resolve() == root, "cargo metadata workspace_root does not match this repository")

    metadata_packages = metadata.get("packages")
    validate_package_records(metadata_packages, "cargo metadata", False)
    validate_metadata_direct_dependencies(metadata)

    lock = parse_toml(read_text(root / "Cargo.lock"), "Cargo.lock")
    validate_package_records(lock.get("package"), "Cargo.lock", True)

    root_manifest = parse_toml(read_text(root / "Cargo.toml"), "Cargo.toml")
    workspace = root_manifest.get("workspace")
    require(isinstance(workspace, dict), "Cargo.toml is missing [workspace]")
    workspace_dependencies = workspace.get("dependencies")
    require(isinstance(workspace_dependencies, dict), "Cargo.toml is missing [workspace.dependencies]")
    for package_name in REQUIRED_WORKSPACE_DIRECT:
        require(package_name in workspace_dependencies, f"Cargo.toml workspace.dependencies is missing {package_name}")
        validate_dependency_spec("Cargo.toml:workspace.dependencies", package_name, workspace_dependencies[package_name], False)

    declared: set[str] = set()
    for manifest_path in repository_manifests(root):
        document = parse_toml(read_text(manifest_path), str(manifest_path.relative_to(root)))
        declared.update(validate_manifest_document(manifest_path.relative_to(root), document))
    require(REQUIRED_WORKSPACE_DIRECT <= declared, "repository manifests are missing required Oxana direct declarations")

    for config_path in repository_cargo_configs(root):
        document = parse_toml(read_text(config_path), str(config_path.relative_to(root)))
        validate_cargo_config(config_path.relative_to(root), document)


def expect_failure(label: str, action: Callable[[], None]) -> None:
    try:
        action()
    except ContractError:
        return
    raise ContractError(f"self-test did not reject {label}")


def run_self_tests() -> None:
    records = [
        {"name": name, "version": EXPECTED_VERSION, "source": EXPECTED_SOURCE, "checksum": checksum}
        for name, checksum in EXPECTED_PACKAGES.items()
    ]
    validate_package_records(records, "self-test", True)

    duplicate = copy.deepcopy(records)
    duplicate.append(copy.deepcopy(records[0]))
    expect_failure("duplicate package", lambda: validate_package_records(duplicate, "self-test", True))

    wrong_version = copy.deepcopy(records)
    wrong_version[0]["version"] = "2.1.2"
    expect_failure("version drift", lambda: validate_package_records(wrong_version, "self-test", True))

    wrong_source = copy.deepcopy(records)
    wrong_source[0]["source"] = None
    expect_failure("source drift", lambda: validate_package_records(wrong_source, "self-test", True))

    wrong_checksum = copy.deepcopy(records)
    wrong_checksum[0]["checksum"] = "0" * 64
    expect_failure("checksum drift", lambda: validate_package_records(wrong_checksum, "self-test", True))

    expect_failure(
        "non-exact direct requirement",
        lambda: validate_dependency_spec("self-test", "oxana", {"version": "2.1"}, False),
    )
    expect_failure(
        "path dependency",
        lambda: validate_dependency_spec("self-test", "oxana", {"version": EXPECTED_REQUIREMENT, "path": "vendor/oxana"}, False),
    )
    expect_failure(
        "patch.crates-io",
        lambda: validate_manifest_document(Path("Cargo.toml"), {"patch": {"crates-io": {}}}),
    )
    expect_failure(
        "vendored Cargo source",
        lambda: validate_cargo_config(Path(".cargo/config.toml"), {"source": {"vendor": {"directory": "vendor"}}}),
    )
    expect_failure("malformed metadata JSON", lambda: parse_json("{", "self-test metadata"))
    expect_failure("malformed Cargo.lock TOML", lambda: parse_toml("[[package]", "self-test Cargo.lock"))


def main() -> None:
    if len(sys.argv) != 4:
        raise ContractError("internal verifier invocation is invalid")
    root = Path(sys.argv[1]).resolve()
    metadata_path = Path(sys.argv[2]).resolve()
    mode = sys.argv[3]
    require(mode in ("verify", "self-test"), "unknown verifier mode")
    validate_repository(root, metadata_path)
    if mode == "self-test":
        run_self_tests()
        print("oxana registry verifier self-test passed")
    print("oxana registry source verified: oxana/oxana-macros/oxana-web 2.1.3")


try:
    main()
except ContractError as error:
    print(f"oxana registry verification failed: {error}", file=sys.stderr)
    raise SystemExit(1)
PY
