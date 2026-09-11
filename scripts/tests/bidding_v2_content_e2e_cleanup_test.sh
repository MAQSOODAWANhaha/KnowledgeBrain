#!/usr/bin/env bash
set -euo pipefail

# This test injects a fake docker command; it never contacts the host daemon.
source scripts/lib/content_e2e_cleanup.sh

tmp=$(mktemp -d /tmp/kb-content-cleanup-test.XXXXXX)
trap 'rm -rf "$tmp"' EXIT
cat >"$tmp/docker" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
state=${FAKE_DOCKER_STATE:?}
mode=${FAKE_DOCKER_MODE:-ok}
case "$1" in
  ps)
    if [[ "$2" == -aq ]]; then
      [[ "$mode" == refs-fail ]] && exit 75
      volume=${5#volume=}
      printf '%s\n' "${volume%-volume}"
      [[ "$mode" == shared-volume ]] && printf '%s\n' unrelated-container
      exit 0
    fi
    count_file="$state.ps-count"
    count=0
    [[ -f "$count_file" ]] && count=$(cat "$count_file")
    count=$((count + 1)); printf '%s' "$count" >"$count_file"
    [[ "$mode" == query-fail ]] && exit 71
    [[ "$mode" == verify-fail && "$count" -ge 2 ]] && exit 72
    cat "$state"
    ;;
  inspect)
    [[ "$mode" == inspect-fail ]] && exit 76
    name=$4
    if [[ "$3" == *Mounts* ]]; then
      printf '%s-volume\n' "$name"
    else
      owner=content-e2e
      [[ "$mode" == unowned ]] && owner=runtime
      printf '%s %s\n' "$name" "$owner"
    fi
    ;;
  volume)
    if [[ "$2" == inspect ]]; then
      [[ "$mode" == volume-query-fail ]] && exit 77
      if [[ "$mode" == named-volume ]]; then printf '{}\n'; else printf '{"com.docker.volume.anonymous":""}\n'; fi
    elif [[ "$2" == ls ]]; then
      [[ "$mode" == volume-verify-fail ]] && exit 78
      volume=${5#name=^}; volume=${volume%\$}
      grep -Fx "$volume" "$state.volumes" || true
    else exit 79; fi
    ;;
  rm)
    [[ "$2" == -fv ]] || exit 80
    name=$3
    printf '%s\n' "$name" >>"$state.rm-attempts"
    [[ "$mode" == rm-fail ]] && exit 73
    grep -Fxv "$name" "$state" >"$state.next" || true
    mv "$state.next" "$state"
    if [[ "$mode" != volume-residue ]]; then
      grep -Fxv "$name-volume" "$state.volumes" >"$state.volumes.next" || true
      mv "$state.volumes.next" "$state.volumes"
    fi
    [[ "$mode" == residue ]] && printf '%s\n' "$name" >>"$state"
    true
    ;;
  *) exit 74 ;;
esac
SH
chmod +x "$tmp/docker"

run_case() {
  local mode=$1 expected=$2
  local state="$tmp/$mode"
  printf '%s\n%s\n' e2e-postgres e2e-redis >"$state"
  printf '%s\n%s\n' e2e-postgres-volume e2e-redis-volume >"$state.volumes"
  rm -f "$state.ps-count" "$state.rm-attempts"
  set +e
  FAKE_DOCKER_STATE="$state" FAKE_DOCKER_MODE="$mode" \
    content_e2e_remove_named_containers "$tmp/docker" e2e-postgres e2e-redis >/dev/null 2>&1
  actual=$?
  set -e
  if [[ "$expected" == success ]]; then
    [[ $actual -eq 0 && ! -s "$state" && ! -s "$state.volumes" ]]
  else
    [[ $actual -ne 0 ]]
  fi
}

run_case ok success
run_case query-fail failure
run_case verify-fail failure
run_case rm-fail failure
[[ $(wc -l <"$tmp/rm-fail.rm-attempts") -eq 2 ]]
run_case residue failure
for mode in inspect-fail unowned volume-query-fail named-volume shared-volume refs-fail volume-verify-fail volume-residue; do
  run_case "$mode" failure
done
[[ ! -e "$tmp/unowned.rm-attempts" && ! -e "$tmp/shared-volume.rm-attempts" && ! -e "$tmp/named-volume.rm-attempts" ]]
[[ $(content_e2e_final_status 42 1) == 42 ]]
[[ $(content_e2e_final_status 0 1) == 1 ]]
[[ $(content_e2e_final_status 0 0) == 0 ]]
printf '%s\n' content-e2e-cleanup-test-ok
