# bash completion for `ac`.
# Install: source this file from ~/.bashrc

_ac_bash_projects() {
  local d f
  for d in "${XDG_CONFIG_HOME:-$HOME/.config}/ac/projects" "${AC_HOME:-$HOME/scripts/ac}/projects"; do
    [ -d "$d" ] || continue
    for f in "$d"/*.json; do
      [ -e "$f" ] || continue
      basename "$f" .json
    done
  done | sort -u
}

_ac_bash_services() {
  local proj="$1" d f
  for d in "${XDG_CONFIG_HOME:-$HOME/.config}/ac/projects" "${AC_HOME:-$HOME/scripts/ac}/projects"; do
    f="$d/$proj.json"
    if [ -f "$f" ]; then
      jq -r '.services[].name' "$f" 2>/dev/null
      return
    fi
  done
}

_ac_complete() {
  local cur prev words cword
  cur="${COMP_WORDS[COMP_CWORD]}"
  local commands="ls projects status daemon config help version"
  local actions="start stop restart status ps logs exec ip"

  if [ "$COMP_CWORD" -eq 1 ]; then
    COMPREPLY=( $(compgen -W "$(_ac_bash_projects) $commands" -- "$cur") )
    return
  fi

  local first="${COMP_WORDS[1]}"

  if [ "$first" = "daemon" ]; then
    [ "$COMP_CWORD" -eq 2 ] && COMPREPLY=( $(compgen -W "status stop" -- "$cur") )
    return
  fi

  case "$first" in
    ls|projects|status|config|help|version) return ;;
  esac

  if [ "$COMP_CWORD" -eq 2 ]; then
    COMPREPLY=( $(compgen -W "$actions" -- "$cur") )
    return
  fi

  if [ "$COMP_CWORD" -eq 3 ]; then
    case "${COMP_WORDS[2]}" in
      logs|exec|ip)
        COMPREPLY=( $(compgen -W "$(_ac_bash_services "$first")" -- "$cur") )
        ;;
    esac
  fi
}

complete -F _ac_complete ac
