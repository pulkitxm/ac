#!/usr/bin/env bash
# supervisor.sh - the background watchdog that ties daemon lifetime to
# container lifetime.
#
# It is only ever started when ac itself started the daemon. If the daemon was
# already running when ac was invoked, no supervisor is spawned at all and the
# daemon is left completely alone.
#
# The watchdog exists so the daemon is reaped even when containers go away
# without `ac <project> stop` - they crash, exit on their own, or are killed
# with plain `container stop`.

AC_POLL_INTERVAL="${AC_POLL_INTERVAL:-5}"   # seconds between checks
AC_IDLE_GRACE="${AC_IDLE_GRACE:-4}"         # consecutive idle polls before acting

supervisor_pid() {
  [ -f "$AC_SUPERVISOR_PIDFILE" ] || return 1
  local pid; pid=$(cat "$AC_SUPERVISOR_PIDFILE" 2>/dev/null)
  [ -n "$pid" ] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  printf '%s' "$pid"
}

supervisor_running() { supervisor_pid >/dev/null 2>&1; }

# Spawn the watchdog if (and only if) ac owns the daemon.
supervisor_ensure() {
  daemon_is_ours || return 0
  supervisor_running && return 0

  nohup "$AC_HOME/bin/ac" __supervise >>"$AC_SUPERVISOR_LOG" 2>&1 &
  printf '%s' "$!" > "$AC_SUPERVISOR_PIDFILE"
  dim "supervisor started (pid $!) - daemon will stop when the last container exits"
}

supervisor_stop() {
  local pid
  pid=$(supervisor_pid) || { rm -f "$AC_SUPERVISOR_PIDFILE"; return 0; }
  kill "$pid" 2>/dev/null
  rm -f "$AC_SUPERVISOR_PIDFILE"
}

# Synchronous teardown check, used by `ac <project> stop`.
# Refcounts across ALL projects: another project still running means the daemon
# stays up even though this project is done with it.
supervisor_settle() {
  local remaining
  remaining=$(ac_running_containers | wc -l | tr -d ' ')

  if [ "$remaining" -gt 0 ]; then
    dim "$remaining ac container(s) still running - leaving daemon up"
    return 0
  fi

  supervisor_stop
  daemon_release
}

# The watchdog loop itself (`ac __supervise`). Runs detached.
supervisor_loop() {
  local armed=0 idle=0 count

  while true; do
    # If the daemon went away or ownership was dropped, our job is over.
    if ! daemon_is_ours || ! daemon_running; then
      rm -f "$AC_SUPERVISOR_PIDFILE"
      exit 0
    fi

    count=$(ac_running_containers | wc -l | tr -d ' ')

    if [ "$count" -gt 0 ]; then
      armed=1
      idle=0
      sleep "$AC_POLL_INTERVAL"
      continue
    fi

    # Never act before containers have actually appeared, otherwise the
    # watchdog would race `ac start` and kill the daemon mid-startup.
    if [ "$armed" -eq 0 ]; then
      sleep "$AC_POLL_INTERVAL"
      continue
    fi

    idle=$((idle + 1))

    # TODO(human): decide the debounce policy for shutting the daemon down.
    # `idle` counts consecutive polls with zero ac containers running, and
    # AC_IDLE_GRACE is available as the threshold.

    sleep "$AC_POLL_INTERVAL"
  done
}
