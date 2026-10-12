#!/usr/bin/env bash
# Fresh, private-network engine bootstrap. Only the app login has a password.
set -Eeuo pipefail
source /usr/local/bin/docker-entrypoint.sh

[[ "${POSTGRES_USER:-}" == postgres ]] || { echo 'internal POSTGRES_USER must be postgres' >&2; exit 1; }
: "${KB_APP_DB_USER:?set KB_APP_DB_USER}"
: "${KB_APP_DB_PASSWORD:?set KB_APP_DB_PASSWORD}"
[[ -n "${KB_APP_DB_USER//[[:space:]]/}" && -n "${KB_APP_DB_PASSWORD//[[:space:]]/}" ]] || {
  echo 'application database credentials must not be blank' >&2; exit 1;
}
[[ "$KB_APP_DB_USER" != postgres ]] || { echo 'application database user must not be postgres' >&2; exit 1; }
# No engine password is generated, accepted from outside, or copied from the app.
unset POSTGRES_PASSWORD POSTGRES_PASSWORD_FILE
export POSTGRES_HOST_AUTH_METHOD=scram-sha-256
export POSTGRES_INITDB_ARGS='--auth-local=trust --auth-host=scram-sha-256'

# Do the official root-only directory preparation before re-entering this wrapper;
# official _main's gosu path would otherwise lose the scoped overrides below.
if [[ "$(id -u)" == 0 ]]; then
  docker_setup_env
  docker_create_db_directories
  exec gosu postgres bash "${BASH_SOURCE[0]}" "$@"
fi

docker_verify_minimum_env() { :; }
docker_init_database_dir() {
  # Unix socket administration remains local to this container. TCP connections
  # always require SCRAM; postgres has no password and cannot authenticate there.
  initdb --username=postgres --auth-local=trust --auth-host=scram-sha-256
}
_main "$@"
