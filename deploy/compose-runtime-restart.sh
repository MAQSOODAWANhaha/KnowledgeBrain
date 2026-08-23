#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

# Deliberately excludes migrate and first-launch-verifier. Runtime identities
# read the durable marker and finalized topology during every startup.
docker compose --profile runtime up -d --no-deps api worker retention docreader
