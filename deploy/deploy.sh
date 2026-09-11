#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

COMPOSE=(
  docker compose
  -f deploy/docker-compose.yml
  -f deploy/docker-compose.local.yml
  --env-file deploy/.env
  --profile runtime
)

case "${1:-}" in
  delete)
    # Permanently removes this Compose project's containers, networks, and data volumes.
    "${COMPOSE[@]}" down -v --remove-orphans
    ;;
  create)
    # Self-signed certs for local HTTPS (127.0.0.1 + host IPv4). Skip if already present.
    bash deploy/certs/generate.sh
    # Build with Docker/BuildKit cache, then create or update only changed services.
    "${COMPOSE[@]}" up -d --build --remove-orphans
    "${COMPOSE[@]}" ps
    ;;
  up)
    bash deploy/certs/generate.sh
    # Apply current compose config without rebuilding images or deleting volumes.
    shift
    "${COMPOSE[@]}" up -d --remove-orphans "$@"
    "${COMPOSE[@]}" ps
    ;;
  restart)
    bash deploy/certs/generate.sh
    # Recreate runtime containers from deploy/.env without rebuilding images or deleting volumes.
    "${COMPOSE[@]}" up -d --no-build --force-recreate --remove-orphans
    "${COMPOSE[@]}" ps
    ;;
  logs)
    exec "${COMPOSE[@]}" logs --follow --tail=500 --timestamps
    ;;
  *)
    echo "Usage: $0 {create|up|restart|logs|delete}" >&2
    exit 64
    ;;
esac
