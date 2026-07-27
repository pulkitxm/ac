#!/usr/bin/env bash
# project.sh - turn a declarative project manifest into running containers.
#
# A project is a JSON file (see projects/shop.json). Adding a new project
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

# Accept either the bare service name (postgres) or the full container name
# (shop-postgres). `ac <proj> ls` prints the latter, so that is what people
# naturally copy back into the next command.
proj_normalize_service() {
  local proj="$1" name="$2"
  case "$name" in
    "$proj"-*) printf '%s' "${name#${proj}-}" ;;
    *)         printf '%s' "$name" ;;
  esac
}

# True if the argument names a service of this project, in either form.
proj_has_service() {
  proj_services "$1" | grep -qxF -- "$(proj_normalize_service "$1" "$2")"
}

# Echo the services an action should apply to: every service when no names are
# given, otherwise just the named ones, validated so a typo fails loudly
# instead of silently doing nothing.
proj_target_services() {
  local proj="$1"; shift
  if [ $# -eq 0 ]; then
    proj_services "$proj"
    return 0
  fi
  local s n all
  all=$(proj_services "$proj")
  for s in "$@"; do
    n=$(proj_normalize_service "$proj" "$s")
    printf '%s\n' "$all" | grep -qxF -- "$n" \
      || die "no such service '$s' in project '$proj' (have: $(echo $all))"
    printf '%s\n' "$n"
  done
}

# Same, but as container names, for passing straight to `container <verb>`.
proj_container_names() {
  local proj="$1"; shift
  local svc
  proj_target_services "$proj" "$@" | while IFS= read -r svc; do
    [ -n "$svc" ] && svc_container_name "$proj" "$svc"
  done
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
    printf '%s\n' "$names" | grep -qxF -- "$c" && printf '%s\n' "$c"
  done
}

# ------------------------------------------------------------------ start ---

_ensure_volumes() {
  local proj="$1" svc_json="$2" vol full
  printf '%s' "$svc_json" | jq -r '(.volumes // [])[].name' | while IFS= read -r vol; do
    [ -n "$vol" ] || continue
    full=$(svc_volume_name "$proj" "$vol")
    if ! container volume ls 2>/dev/null | awk 'NR>1 {print $1}' | grep -qxF -- "$full"; then
      run_cmd container volume create "$full" >/dev/null 2>&1 && dim "  volume $full created"
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
  # A stopped container still has its filesystem: restart it in place rather
  # than recreating, unless --recreate was asked for or the config changed.
  if [ "$state" = "stopped" ] || [ "$state" = "exited" ]; then
    if [ -n "${AC_F_RECREATE:-}" ]; then
      dim "  recreating $cname"
      run_cmd container rm "$cname" >/dev/null 2>&1
    else
      info "restarting $cname"
      if run_cmd container start "$cname" >/dev/null 2>&1; then
        _wait_ready "$cname" "$svc_json"
        ok "$cname up  $(dim "$(container_ip "$cname")")"
        return 0
      fi
      dim "  restart failed, recreating"
      run_cmd container rm "$cname" >/dev/null 2>&1
    fi
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
  if ! run_cmd container "${args[@]}" >/dev/null; then
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
  local proj="$1"; shift
  local targets svc
  # Resolve (and validate) before touching the daemon, so a typo does not
  # leave a daemon started for nothing.
  targets=$(proj_target_services "$proj" "$@") || exit 1

  daemon_ensure
  # Only the images we are about to pull can justify a registry login.
  local svc_images
  svc_images=$(proj_json "$proj" | jq -r '.services[].image' | tr '\n' ' ')
  project_login "$proj" "" $svc_images
  printf '%s\n' "$targets" | while IFS= read -r svc; do
    [ -n "$svc" ] && start_service "$proj" "$svc"
  done
  supervisor_ensure
}

# Pre-pull every image in the manifest so a later start is fast.
project_pull() {
  local proj="$1"; shift
  local targets svc img
  targets=$(proj_target_services "$proj" "$@") || exit 1
  daemon_ensure
  local svc_images
  svc_images=$(proj_json "$proj" | jq -r '.services[].image' | tr '\n' ' ')
  project_login "$proj" "" $svc_images
  printf '%s\n' "$targets" | while IFS= read -r svc; do
    [ -n "$svc" ] || continue
    img=$(proj_service_json "$proj" "$svc" | jq -r '.image')
    info "pulling $img"
    container image pull "$img" >/dev/null && ok "$img"
  done
}

# Authenticate to any private registries the project declares, before images
# are pulled. Credentials are never stored in the manifest: `passwordCmd` is an
# argv that is executed and piped to --password-stdin, which suits tokens that
# expire (AWS ECR tokens last 12 hours, so this re-runs on every start).
# Log in to the project's private registries.
#
# Any images passed as extra arguments act as a filter: a registry is only
# contacted when one of those images actually comes from it. That keeps
# `ac <proj> start` from trying to authenticate to ECR just to pull
# postgres from docker.io. With no images given (an explicit `ac <proj>
# login`) every declared registry is used.
project_login() {
  local proj="$1" profile="${2:-}"; shift 2 2>/dev/null || shift $#
  local images="$*"
  local n; n=$(proj_json "$proj" | jq '(.registries // []) | length')
  [ "$n" -gt 0 ] || return 0

  local i=0 server user tmp a
  while [ "$i" -lt "$n" ]; do
    server=$(build_interpolate "$proj" "$profile" \
      "$(proj_json "$proj" | jq -r --argjson i "$i" '.registries[$i].server')")
    user=$(proj_json "$proj" | jq -r --argjson i "$i" '.registries[$i].username // "AWS"')

    # Skip registries nothing is being pulled from, and skip malformed ones
    # (an uninterpolated {{account}} leaves a leading dot).
    case "$server" in ""|.*|*"{{"*) i=$((i + 1)); continue ;; esac
    if [ -n "$images" ]; then
      case "$images" in
        *"$server"*) ;;
        *) i=$((i + 1)); continue ;;
      esac
    fi

    tmp=$(mktemp)
    proj_json "$proj" | jq -r --argjson i "$i" '.registries[$i].passwordCmd[]' > "$tmp"
    local argv=()
    while IFS= read -r a; do
      argv+=("$(build_interpolate "$proj" "$profile" "$a")")
    done < "$tmp"
    rm -f "$tmp"

    info "logging in to $server"
    if "${argv[@]}" 2>/dev/null \
        | container registry login --username "$user" --password-stdin "$server" >/dev/null 2>&1; then
      ok "authenticated to $server"
    else
      warn "login to $server failed; pulls of private images will fail"
    fi
    i=$((i + 1))
  done
}

# Follow (or dump) every service at once, prefixing each line with the service
# name. `container logs` only handles a single container, so the fan-out and
# the interleaving are done here, the way `docker compose logs` behaves.
project_logs_all() {
  local proj="$1"; shift
  local svc cname col i=0 pids=""
  local colors
  colors=("$C_BLUE" "$C_GREEN" "$C_YELLOW" "$C_RED")

  for svc in $(proj_services "$proj"); do
    cname=$(svc_container_name "$proj" "$svc")
    col=${colors[$((i % 4))]}
    (
      container logs "$@" "$cname" 2>&1 | while IFS= read -r line; do
        printf '%s%-11s%s | %s\n' "$col" "$svc" "$C_RESET" "$line"
      done
    ) &
    pids="$pids $!"
    i=$((i + 1))
  done

  # Ctrl-C must take the whole fan-out down, not just the foreground wait.
  trap 'kill $pids 2>/dev/null; exit 0' INT TERM
  wait
}

project_images() {
  local proj="$1" svc
  printf '%s%-14s %s%s\n' "$C_BOLD" "SERVICE" "IMAGE" "$C_RESET"
  proj_services "$proj" | while IFS= read -r svc; do
    [ -n "$svc" ] || continue
    printf '%-14s %s\n' "$svc" "$(proj_service_json "$proj" "$svc" | jq -r '.image')"
  done
}

# ------------------------------------------------------------------- stop ---

# Stop containers WITHOUT removing them. The container keeps its filesystem and
# can be restarted in place, which is both faster and non-destructive. Use
# `down` when you actually want them gone.
project_stop() {
  local proj="$1"; shift
  local targets svc cname state
  targets=$(proj_target_services "$proj" "$@") || exit 1

  printf '%s\n' "$targets" | while IFS= read -r svc; do
    [ -n "$svc" ] || continue
    cname=$(svc_container_name "$proj" "$svc")
    state=$(container_state "$cname")
    case "$state" in
      absent)  dim "  $cname not created" ;;
      running) info "stopping $cname"; run_cmd container stop "$cname" >/dev/null 2>&1
               ok "$cname stopped" ;;
      *)       dim "  $cname already $state" ;;
    esac
  done

  supervisor_settle
}

# Stop AND remove the containers. Named volumes are untouched, so data survives.
project_down() {
  local proj="$1"; shift
  local targets svc cname state
  targets=$(proj_target_services "$proj" "$@") || exit 1

  printf '%s\n' "$targets" | while IFS= read -r svc; do
    [ -n "$svc" ] || continue
    cname=$(svc_container_name "$proj" "$svc")
    state=$(container_state "$cname")
    [ "$state" = "absent" ] && continue
    if [ "$state" = "running" ]; then
      info "stopping $cname"
      run_cmd container stop "$cname" >/dev/null 2>&1
    fi
    run_cmd container rm "$cname" >/dev/null 2>&1
    ok "$cname removed"
  done

  supervisor_settle
}

# ----------------------------------------------------------------- status ---

project_status() {
  local proj="$1" svc cname state ip ports
  # Without a running daemon nothing can be queried, and every container would
  # be reported as "absent", which is a lie: they are merely unreachable.
  if ! daemon_running; then
    warn "container daemon is not running - state unknown (run: ac $proj start)"
  fi
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
