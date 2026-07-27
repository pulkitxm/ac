#!/usr/bin/env bash
# project.sh - turn a declarative project manifest into running containers.
#
# A project is a JSON file (see projects/noveum.json). Adding a new project
# is just dropping another JSON file into projects/ or ~/.config/ac/projects/;
# no code changes are needed.

# Container naming convention: <project>-<service>. This is also how ac
# recognises its own containers later, so it stays stable and predictable.
# Note the trailing newline: these are used both in $(...) substitution, which
# strips it, and in list-building loops, which need it as a separator.
svc_container_name() { printf '%s-%s\n' "$1" "$2"; }
svc_volume_name()    { printf '%s-%s\n' "$1" "$2"; }

# --------------------------------------------------------------- manifest ---

proj_json() {
  local file
  file=$(ac_project_file "$1")
  [ -n "$file" ] || die "unknown project: $1 (try: ac ls)"
  cat "$file"
}

proj_services() {
  proj_json "$1" | jq -r '.services[].name'
}

proj_service_json() {
  proj_json "$1" | jq -c --arg s "$2" '.services[] | select(.name==$s)'
}

proj_description() {
  proj_json "$1" | jq -r '.description // ""'
}

# ------------------------------------------------------------ state query ---

# All containers currently known to the daemon, as "<id> <state>" lines.
_all_containers() {
  container ls -a --format json 2>/dev/null \
    | jq -r '.[] | "\(.id) \(.status.state)"' 2>/dev/null
}

container_state() {
  _all_containers | awk -v n="$1" '$1==n {print $2; found=1} END {if(!found) print "absent"}'
}

container_ip() {
  container ls --format json 2>/dev/null \
    | jq -r --arg n "$1" '.[] | select(.id==$n) | .status.networks[0].ipv4Address // ""' 2>/dev/null
}

# Every container belonging to any known project that is currently running.
# This is what the supervisor counts to decide when the daemon can go away.
ac_running_containers() {
  local names p s
  names=$(
    ac_project_names | while IFS= read -r p; do
      [ -n "$p" ] || continue
      proj_services "$p" 2>/dev/null | while IFS= read -r s; do
        [ -n "$s" ] && svc_container_name "$p" "$s"
      done
    done
  )
  [ -n "$names" ] || return 0

  _all_containers | awk '$2=="running" {print $1}' | while IFS= read -r c; do
    printf '%s\n' "$names" | grep -qx "$c" && printf '%s\n' "$c"
  done
}

# ------------------------------------------------------------------ start ---

_ensure_volumes() {
  local proj="$1" svc_json="$2" vol full
  printf '%s' "$svc_json" | jq -r '(.volumes // [])[].name' | while IFS= read -r vol; do
    [ -n "$vol" ] || continue
    full=$(svc_volume_name "$proj" "$vol")
    if ! container volume ls 2>/dev/null | awk 'NR>1 {print $1}' | grep -qx "$full"; then
      container volume create "$full" >/dev/null 2>&1 && dim "  volume $full created"
    fi
  done
}

# Poll a service's readyCmd until it succeeds. Apple `container` has no
# healthcheck primitive, so readiness is implemented here.
_wait_ready() {
  local cname="$1" svc_json="$2" timeout waited=0
  local has_cmd
  has_cmd=$(printf '%s' "$svc_json" | jq -r 'if (.readyCmd // []) | length > 0 then "yes" else "no" end')
  [ "$has_cmd" = "yes" ] || return 0

  timeout=$(printf '%s' "$svc_json" | jq -r '.readyTimeout // 90')

  # Build the exec argv without arrays-of-arrays gymnastics.
  local tmp; tmp=$(mktemp)
  printf '%s' "$svc_json" | jq -r '.readyCmd[]' > "$tmp"

  printf '  waiting for %s ' "$cname"
  while [ "$waited" -lt "$timeout" ]; do
    local argv=()
    while IFS= read -r a; do argv+=("$a"); done < "$tmp"
    if container exec "$cname" "${argv[@]}" >/dev/null 2>&1; then
      rm -f "$tmp"; printf ' %sready%s\n' "$C_GREEN" "$C_RESET"; return 0
    fi
    printf '.'
    sleep 2
    waited=$((waited + 2))
  done
  rm -f "$tmp"
  printf ' %stimeout%s\n' "$C_YELLOW" "$C_RESET"
  warn "$cname did not become ready within ${timeout}s (continuing)"
  return 0
}

start_service() {
  local proj="$1" svc="$2"
  local svc_json cname state
  svc_json=$(proj_service_json "$proj" "$svc")
  [ -n "$svc_json" ] || die "no such service '$svc' in project '$proj'"
  cname=$(svc_container_name "$proj" "$svc")

  state=$(container_state "$cname")
  if [ "$state" = "running" ]; then
    ok "$cname already running"
    return 0
  fi
  if [ "$state" = "stopped" ] || [ "$state" = "exited" ]; then
    dim "  removing stale $cname"
    container rm "$cname" >/dev/null 2>&1
  fi

  _ensure_volumes "$proj" "$svc_json"

  local args=(run -d --progress none --name "$cname" --label "ac.project=$proj")

  local image; image=$(printf '%s' "$svc_json" | jq -r '.image')
  local cpus;  cpus=$(printf '%s' "$svc_json" | jq -r '.cpus // empty')
  local mem;   mem=$(printf '%s' "$svc_json" | jq -r '.memory // empty')
  [ -n "$cpus" ] && args+=(--cpus "$cpus")
  [ -n "$mem" ]  && args+=(--memory "$mem")

  local line
  while IFS= read -r line; do
    [ -n "$line" ] && args+=(--env "$line")
  done < <(printf '%s' "$svc_json" | jq -r '(.env // {}) | to_entries[] | "\(.key)=\(.value)"')

  while IFS= read -r line; do
    [ -n "$line" ] && args+=(--publish "$line")
  done < <(printf '%s' "$svc_json" | jq -r '(.ports // [])[]')

  while IFS= read -r line; do
    [ -n "$line" ] && args+=(--volume "$(svc_volume_name "$proj" "${line%%:*}"):${line#*:}")
  done < <(printf '%s' "$svc_json" | jq -r '(.volumes // [])[] | "\(.name):\(.target)"')

  args+=("$image")

  while IFS= read -r line; do
    [ -n "$line" ] && args+=("$line")
  done < <(printf '%s' "$svc_json" | jq -r '(.args // [])[]')

  info "starting $cname"
  if ! container "${args[@]}" >/dev/null; then
    # `container run` sometimes reports a spurious "not found" while the
    # container is in fact created and running, so trust observed state over
    # the exit code before giving up.
    sleep 2
    if [ "$(container_state "$cname")" != "running" ]; then
      die "failed to start $cname"
    fi
    dim "  $cname reported an error but is running; continuing"
  fi

  _wait_ready "$cname" "$svc_json"
  ok "$cname up  $(dim "$(container_ip "$cname")")"
}

project_start() {
  local proj="$1" svc
  daemon_ensure
  proj_services "$proj" | while IFS= read -r svc; do
    [ -n "$svc" ] && start_service "$proj" "$svc"
  done
  supervisor_ensure
}

# ------------------------------------------------------------------- stop ---

project_stop() {
  local proj="$1" svc cname state any=0
  proj_services "$proj" | while IFS= read -r svc; do
    [ -n "$svc" ] || continue
    cname=$(svc_container_name "$proj" "$svc")
    state=$(container_state "$cname")
    [ "$state" = "absent" ] && continue
    if [ "$state" = "running" ]; then
      info "stopping $cname"
      container stop "$cname" >/dev/null 2>&1
    fi
    container rm "$cname" >/dev/null 2>&1
    ok "$cname removed"
  done

  # Hand the daemon question to the shared policy: it is only stopped when
  # nothing else ac manages is still alive, and only if ac owns it.
  supervisor_settle
}

# ----------------------------------------------------------------- status ---

project_status() {
  local proj="$1" svc cname state ip ports
  printf '%s%-22s %-10s %-18s %s%s\n' "$C_BOLD" "CONTAINER" "STATE" "IP" "PORTS" "$C_RESET"
  proj_services "$proj" | while IFS= read -r svc; do
    [ -n "$svc" ] || continue
    cname=$(svc_container_name "$proj" "$svc")
    state=$(container_state "$cname")
    ip=$(container_ip "$cname")
    ports=$(proj_service_json "$proj" "$svc" | jq -r '(.ports // []) | join(",")')
    printf '%-22s %-10s %-18s %s\n' "$cname" "$state" "${ip:--}" "${ports:--}"
  done
}
