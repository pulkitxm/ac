#!/usr/bin/env bash
# common.sh - shared configuration, paths and logging helpers for `ac`.
#
# Sourced by bin/ac and by the supervisor. Targets bash 3.2 (macOS system bash),
# so no associative arrays, no mapfile, no ${var,,}.

AC_VERSION="0.1.0"

: "${XDG_CONFIG_HOME:=$HOME/.config}"
: "${XDG_STATE_HOME:=$HOME/.local/state}"

AC_CONFIG_DIR="$XDG_CONFIG_HOME/ac"
AC_STATE_DIR="$XDG_STATE_HOME/ac"
AC_CONFIG_FILE="$AC_CONFIG_DIR/config.json"

# Presence of this file is the single source of truth for "ac started the
# daemon, so ac is allowed to stop it". If it does not exist, the daemon was
# already running and must never be touched.
AC_OWNER_FILE="$AC_STATE_DIR/daemon.owned"
AC_SUPERVISOR_PIDFILE="$AC_STATE_DIR/supervisor.pid"
AC_SUPERVISOR_LOG="$AC_STATE_DIR/supervisor.log"

mkdir -p "$AC_STATE_DIR" "$AC_CONFIG_DIR"

# ---------------------------------------------------------------- logging ---

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C_RESET=$'\033[0m'; C_DIM=$'\033[2m'; C_BOLD=$'\033[1m'
  C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'; C_BLUE=$'\033[34m'
else
  C_RESET=""; C_DIM=""; C_BOLD=""; C_RED=""; C_GREEN=""; C_YELLOW=""; C_BLUE=""
fi

log()  { printf '%s\n' "$*"; }
info() { printf '%s==>%s %s\n' "$C_BLUE" "$C_RESET" "$*"; }
ok()   { printf '%s  ok%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
warn() { printf '%swarn%s %s\n' "$C_YELLOW" "$C_RESET" "$*" >&2; }
err()  { printf '%s err%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; }
die()  { err "$*"; exit 1; }
dim()  { printf '%s%s%s\n' "$C_DIM" "$*" "$C_RESET"; }

require() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

# Run a command after echoing it verbatim. Every underlying `container`
# invocation goes through this, so any step can be copied and re-run by hand,
# and an agent reading the output can see exactly what was executed.
# Set AC_QUIET=1 to suppress the echo.
run_cmd() {
  [ -n "${AC_QUIET:-}" ] || printf '%s   $ %s%s\n' "$C_DIM" "$*" "$C_RESET" >&2
  "$@"
}

# ----------------------------------------------------------------- config ---

# Seed a config file on first run. If the daemon happens to be running we adopt
# its current appRoot so `ac` keeps using the same image store the user already
# has, rather than silently starting a second one on the internal disk.
ac_config_init() {
  [ -f "$AC_CONFIG_FILE" ] && return 0

  local app_root=""
  if container system status >/dev/null 2>&1; then
    app_root=$(container system status 2>/dev/null \
      | awk '/^appRoot/ {print substr($0, index($0,$2))}' \
      | sed 's/[[:space:]]*$//')
  fi

  jq -n --arg appRoot "$app_root" '{
    appRoot: $appRoot,
    sparseBundle: "",
    imageMount: "",
    startTimeout: 90
  }' > "$AC_CONFIG_FILE"

  dim "created $AC_CONFIG_FILE"
}

# ac_config <key> [default]
ac_config() {
  local key="$1" def="${2:-}" val
  [ -f "$AC_CONFIG_FILE" ] || { printf '%s' "$def"; return 0; }
  val=$(jq -r --arg k "$key" '.[$k] // empty' "$AC_CONFIG_FILE" 2>/dev/null)
  [ -n "$val" ] && [ "$val" != "null" ] && printf '%s' "$val" || printf '%s' "$def"
}

# ---------------------------------------------------------------- projects ---

# User projects in ~/.config/ac/projects override the ones shipped in the repo,
# so the repo stays cleanly updatable while still being customisable.
ac_project_dirs() {
  printf '%s\n' "$AC_CONFIG_DIR/projects"
  printf '%s\n' "$AC_HOME/projects"
}

ac_project_file() {
  local name="$1" d
  ac_project_dirs | while IFS= read -r d; do
    if [ -f "$d/$name.json" ]; then
      printf '%s' "$d/$name.json"
      return 0
    fi
  done
}

ac_project_names() {
  local d
  ac_project_dirs | while IFS= read -r d; do
    [ -d "$d" ] || continue
    ls -1 "$d"/*.json 2>/dev/null | while IFS= read -r f; do
      basename "$f" .json
    done
  done | sort -u
}
