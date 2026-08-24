#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

# The checked-in completion registry is an independent review gate. Parse it
# before any Docker command; environment variables cannot replace or override it.
python3 - <<'PY'
import pathlib, re, sys, tomllib
path = pathlib.Path("deploy/first-launch/runtime-completion.toml")
try:
    raw = path.read_bytes()
    value = tomllib.loads(raw.decode("utf-8"))
except Exception as error:
    print(f"refusing production first launch: malformed runtime completion registry: {error}", file=sys.stderr)
    sys.exit(66)
required = {
    "format_version", "schema_manifest_sha256", "contract", "phase_1d_runtime_complete",
    "registry_sha256", "evaluation_sha256", "readiness_sha256", "images_sha256",
    "topology_sha256",
}
if set(value) != required:
    print("refusing production first launch: runtime completion registry fields are not exact", file=sys.stderr)
    sys.exit(66)
manifest_sha256 = __import__("hashlib").sha256(
    pathlib.Path("deploy/first-launch/migration-manifest.toml").read_bytes()
).hexdigest()
if (value["format_version"] != 1 or value["contract"] != "bidding-v1"
        or value["schema_manifest_sha256"] != manifest_sha256):
    print("refusing production first launch: runtime completion registry identity mismatch", file=sys.stderr)
    sys.exit(66)
if value["phase_1d_runtime_complete"] is not True:
    print("refusing production first launch: Phase 1D runtime is not complete", file=sys.stderr)
    sys.exit(66)
import hashlib
bindings = {
    "registry_sha256": "deploy/queue-registry.toml",
    "evaluation_sha256": "deploy/eval/first-launch-evaluation.toml",
    "readiness_sha256": "deploy/health/mode-aware-probe.sh",
    "images_sha256": "deploy/images.lock.json",
    "topology_sha256": "deploy/first-launch/topology-inputs.toml",
}
for field, filename in bindings.items():
    digest = value[field]
    actual = hashlib.sha256(pathlib.Path(filename).read_bytes()).hexdigest()
    if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest) or digest != actual:
        print(f"refusing production first launch: {field} does not match {filename}", file=sys.stderr)
        sys.exit(66)
legacy_surfaces = {
    "crates/storage/src/bid.rs": ["persist_extraction_report", "persist_section_retry"],
    "crates/storage/src/bid_extract_publication.rs": ["ExtractionPublicationStore"],
    "crates/storage/src/bid_matching.rs": ["pub async fn commit_route(", "CommitRouteV1"],
    "crates/storage/src/lib.rs": ["drop_blob", "release_object_ref", "bump_object_ref"],
    "crates/bid/src/export.rs": ["regenerate_stale"],
    "crates/bid/src/booklet.rs": ["bid_booklet_parts"],
    "web/src/api.ts": ["downloadExport", "regenerateStale", "owner_name"],
}
for filename, forbidden in legacy_surfaces.items():
    path = pathlib.Path(filename)
    if not path.exists():
        continue
    source = path.read_text(encoding="utf-8")
    present = [token for token in forbidden if token in source]
    if present:
        print(f"refusing production first launch: legacy matching surface remains in {filename}: {present}", file=sys.stderr)
        sys.exit(66)
PY

if [ "${KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH:-}" != "required" ]; then
  echo "refusing destructive fresh launch: set KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH=required" >&2
  exit 64
fi

manifest_versions=$(awk '$1 == "version" && $2 == "=" { if (seen++) printf " "; printf "%s", $3 }' \
  deploy/first-launch/migration-manifest.toml)
expected_versions="1 2 3"
if [ "$manifest_versions" != "$expected_versions" ]; then
  echo "refusing production first launch: migration manifest mismatch (expected $expected_versions; found $manifest_versions)" >&2
  exit 65
fi

# This command is intentionally fresh-volume only. Compose receives secrets
# through its normal environment interpolation; this script never echoes them.
docker compose down --volumes --remove-orphans
docker compose up -d --wait

docker compose --profile first-launch run --rm --no-deps migrate

marker_count=$(docker compose exec -T postgres psql \
  --username "${POSTGRES_USER:-knowledgebrain}" \
  --dbname "${POSTGRES_DB:-knowledgebrain}" --tuples-only --no-align \
  --command 'SELECT count(*) FROM public.production_first_launch_catalog_verifications')
if [ "$marker_count" != "0" ]; then
  echo "migrate/handoff did not leave an empty verification marker" >&2
  exit 1
fi
if docker compose --profile runtime ps --status running --services | grep -Eq '^(api|worker|docreader)$'; then
  echo "runtime started before first-launch verification" >&2
  exit 1
fi

docker compose --profile first-launch run --rm --no-deps first-launch-verifier

docker compose --profile runtime up -d api worker retention docreader
