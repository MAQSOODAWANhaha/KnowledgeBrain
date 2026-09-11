#!/usr/bin/env bash

# Only remove explicitly named, test-labeled containers and their unshared
# anonymous volumes. A failed ownership/daemon check is never proof of absence.
content_e2e_remove_named_containers() {
  if (( $# < 2 )); then
    echo "content E2E cleanup requires docker command and container names" >&2
    return 1
  fi
  local docker_command=$1
  shift
  local before after name identity id owner volumes volume labels references remaining
  local failed=0 safe
  local -a anonymous
  if ! before=$("$docker_command" ps -a --format '{{.Names}}'); then
    echo "Content E2E cleanup could not query Docker" >&2
    return 1
  fi
  for name in "$@"; do
    if ! grep -Fxq "$name" <<<"$before"; then continue; fi
    if ! identity=$("$docker_command" inspect --format '{{.Id}} {{index .Config.Labels "kb.acceptance"}}' "$name"); then
      failed=1; continue
    fi
    read -r id owner <<<"$identity"
    if [[ -z "$id" || "$owner" != content-e2e ]]; then
      echo "Content E2E cleanup refused unowned container: $name" >&2
      failed=1; continue
    fi
    if ! volumes=$("$docker_command" inspect --format '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' "$id"); then
      failed=1; continue
    fi
    anonymous=(); safe=1
    while IFS= read -r volume; do
      [[ -n "$volume" ]] || continue
      if ! labels=$("$docker_command" volume inspect --format '{{json .Labels}}' "$volume"); then
        safe=0; break
      fi
      if ! python3 -c 'import json,sys; sys.exit(0 if "com.docker.volume.anonymous" in (json.loads(sys.argv[1]) or {}) else 1)' "$labels"; then
        echo "Content E2E cleanup refused named or unidentifiable volume: $volume" >&2
        safe=0; break
      fi
      if ! references=$("$docker_command" ps -aq --no-trunc --filter "volume=$volume"); then
        safe=0; break
      fi
      if [[ "$references" != "$id" ]]; then
        echo "Content E2E cleanup refused shared volume: $volume" >&2
        safe=0; break
      fi
      anonymous+=("$volume")
      printf 'Content E2E cleanup owned: container=%s id=%s label=%s anonymous_volume=%s\n' "$name" "$id" "$owner" "$volume"
    done <<<"$volumes"
    if (( ! safe )); then failed=1; continue; fi
    if ! "$docker_command" rm -fv "$id" >/dev/null; then
      echo "Content E2E cleanup could not remove container: $name" >&2
      failed=1; continue
    fi
    for volume in "${anonymous[@]}"; do
      if ! remaining=$("$docker_command" volume ls -q --filter "name=^${volume}$"); then
        failed=1
      elif [[ -n "$remaining" ]]; then
        echo "Content E2E anonymous volume residue: $volume" >&2
        failed=1
      fi
    done
  done
  if ! after=$("$docker_command" ps -a --format '{{.Names}}'); then
    echo "Content E2E cleanup could not verify Docker residue" >&2
    return 1
  fi
  for name in "$@"; do
    if grep -Fxq "$name" <<<"$after"; then
      echo "Content E2E container residue detected: $name" >&2
      failed=1
    fi
  done
  return "$failed"
}

# Preserve an existing test failure. Cleanup failures only replace success.
content_e2e_final_status() {
  local original_status=$1 cleanup_status=$2
  if (( original_status != 0 )); then
    printf '%s\n' "$original_status"
  elif (( cleanup_status != 0 )); then
    printf '1\n'
  else
    printf '0\n'
  fi
}
