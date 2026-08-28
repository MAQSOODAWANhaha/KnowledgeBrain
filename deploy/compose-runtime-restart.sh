#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
docker compose -f deploy/docker-compose.yml --profile runtime up -d --remove-orphans "$@"
