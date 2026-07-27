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

# Bare service names plus the <project>-<service> form that `ac <p> ls` prints.
_ac_bash_services() {
  local proj="$1" d f
  for d in "${XDG_CONFIG_HOME:-$HOME/.config}/ac/projects" "${AC_HOME:-$HOME/scripts/ac}/projects"; do
    f="$d/$proj.json"
    if [ -f "$f" ]; then
      jq -r ".services[].name, \"${proj}-\" + .services[].name" "$f" 2>/dev/null
      return
    fi
  done
}

_ac_complete() {
  local cur prev
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"

  local commands="ls projects status daemon images df prune config help version"
  local actions="start stop restart ls ps status logs exec sh shell stats inspect kill rm cp pull images port ip env login config"

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
    ls|projects|status|config|help|version|images|df|prune) return ;;
  esac

  if [ "$COMP_CWORD" -eq 2 ]; then
    COMPREPLY=( $(compgen -W "$actions" -- "$cur") )
    return
  fi

  local action="${COMP_WORDS[2]}"
  local svcs; svcs="$(_ac_bash_services "$first")"

  case "$action" in
    start|stop|restart|stats|inspect|rm|pull|port|ip)
      COMPREPLY=( $(compgen -W "$svcs" -- "$cur") ) ;;
    sh|shell|env)
      [ "$COMP_CWORD" -eq 3 ] && COMPREPLY=( $(compgen -W "$svcs" -- "$cur") ) ;;
    exec)
      [ "$COMP_CWORD" -eq 3 ] && COMPREPLY=( $(compgen -W "$svcs" -- "$cur") ) ;;
    logs)
      COMPREPLY=( $(compgen -W "$svcs -f -n --boot" -- "$cur") ) ;;
    kill)
      if [ "$prev" = "-s" ] || [ "$prev" = "--signal" ]; then
        COMPREPLY=( $(compgen -W "TERM KILL HUP INT QUIT USR1 USR2" -- "$cur") )
      else
        COMPREPLY=( $(compgen -W "$svcs -s" -- "$cur") )
      fi
      ;;
    cp)
      COMPREPLY=( $(compgen -W "$svcs" -- "$cur") $(compgen -f -- "$cur") ) ;;
  esac
}

complete -F _ac_complete ac
