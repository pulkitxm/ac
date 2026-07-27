#!/usr/bin/env bash
# daemon.sh - Apple `container` daemon lifecycle, with strict ownership rules.
#
# THE CONTRACT:
#   * Daemon already running when ac needs it -> ac NEVER touches it. Not on
#     start, not on stop, not from the supervisor. It is someone else's.
#   * Daemon not running -> ac starts it, records ownership, and is then
#     responsible for stopping it once the last ac-managed container is gone.
#
# Ownership is recorded on disk ($AC_OWNER_FILE) rather than inferred, so a new
# `ac` invocation in a different shell still knows what the first one did.

daemon_running() {
  container system status >/dev/null 2>&1
}

daemon_is_ours() {
  [ -f "$AC_OWNER_FILE" ]
}

daemon_app_root() {
  container system status 2>/dev/null \
    | awk '/^appRoot/ {print substr($0, index($0,$2))}' \
    | sed 's/[[:space:]]*$//'
}

# Attach the APFS sparse bundle backing the app root, when one is configured.
# Needed because volumes mounted `noowners` make container-apiserver abort with
# "XPC connection error: Connection invalid".
daemon_mount_backing_store() {
  local bundle mount
  bundle=$(ac_config sparseBundle)
  mount=$(ac_config imageMount)

  [ -n "$bundle" ] || return 0
  [ -n "$mount" ] || return 0
  [ -d "$mount" ] && return 0

  [ -e "$bundle" ] || die "configured sparseBundle not found: $bundle"

  info "attaching backing store $(basename "$bundle")"
  hdiutil attach -owners on "$bundle" >/dev/null \
    || die "failed to attach $bundle"
}

# Start the daemon only if it is not already up. Echoes the resulting ownership
# so callers can report it. Safe to call repeatedly.
daemon_ensure() {
  if daemon_running; then
    if daemon_is_ours; then
      dim "daemon already running (started by ac)"
    else
      dim "daemon already running (external) - ac will not manage it"
    fi
    return 0
  fi

  # Nothing running: this invocation becomes the owner.
  daemon_mount_backing_store

  local app_root timeout
  app_root=$(ac_config appRoot)
  timeout=$(ac_config startTimeout 90)

  info "starting container daemon"
  if [ -n "$app_root" ]; then
    dim "  app root: $app_root"
    run_cmd container system start --app-root "$app_root" --timeout "$timeout" >/dev/null \
      || die "failed to start container daemon"
  else
    container system start --timeout "$timeout" >/dev/null \
      || die "failed to start container daemon"
  fi

  date +%s > "$AC_OWNER_FILE"
  ok "daemon started (owned by ac)"
}

# Stop the daemon, but ONLY if ac started it.
daemon_release() {
  if ! daemon_is_ours; then
    dim "daemon was not started by ac - leaving it running"
    return 0
  fi

  if ! daemon_running; then
    rm -f "$AC_OWNER_FILE"
    return 0
  fi

  info "stopping container daemon (ac owned it)"
  run_cmd container system stop >/dev/null 2>&1
  rm -f "$AC_OWNER_FILE"
  ok "daemon stopped"
}

daemon_status_line() {
  if daemon_running; then
    if daemon_is_ours; then
      printf 'running %s(owned by ac)%s  appRoot=%s\n' "$C_DIM" "$C_RESET" "$(daemon_app_root)"
    else
      printf 'running %s(external, untouched)%s  appRoot=%s\n' "$C_DIM" "$C_RESET" "$(daemon_app_root)"
    fi
  else
    printf 'stopped\n'
  fi
}
