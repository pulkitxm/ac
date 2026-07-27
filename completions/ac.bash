_ac() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="ac"
                ;;
            ac,__supervise)
                cmd="ac__subcmd____supervise"
                ;;
            ac,completions)
                cmd="ac__subcmd__completions"
                ;;
            ac,config)
                cmd="ac__subcmd__config"
                ;;
            ac,daemon)
                cmd="ac__subcmd__daemon"
                ;;
            ac,df)
                cmd="ac__subcmd__df"
                ;;
            ac,images)
                cmd="ac__subcmd__images"
                ;;
            ac,ls)
                cmd="ac__subcmd__ls"
                ;;
            ac,project)
                cmd="ac__subcmd__project"
                ;;
            ac,prune)
                cmd="ac__subcmd__prune"
                ;;
            ac,schema)
                cmd="ac__subcmd__schema"
                ;;
            ac,status)
                cmd="ac__subcmd__status"
                ;;
            ac,version)
                cmd="ac__subcmd__version"
                ;;
            ac__subcmd__daemon,status)
                cmd="ac__subcmd__daemon__subcmd__status"
                ;;
            ac__subcmd__daemon,stop)
                cmd="ac__subcmd__daemon__subcmd__stop"
                ;;
            ac__subcmd__project,build)
                cmd="ac__subcmd__project__subcmd__build"
                ;;
            ac__subcmd__project,config)
                cmd="ac__subcmd__project__subcmd__config"
                ;;
            ac__subcmd__project,cp)
                cmd="ac__subcmd__project__subcmd__cp"
                ;;
            ac__subcmd__project,down)
                cmd="ac__subcmd__project__subcmd__down"
                ;;
            ac__subcmd__project,env)
                cmd="ac__subcmd__project__subcmd__env"
                ;;
            ac__subcmd__project,exec)
                cmd="ac__subcmd__project__subcmd__exec"
                ;;
            ac__subcmd__project,images)
                cmd="ac__subcmd__project__subcmd__images"
                ;;
            ac__subcmd__project,inspect)
                cmd="ac__subcmd__project__subcmd__inspect"
                ;;
            ac__subcmd__project,ip)
                cmd="ac__subcmd__project__subcmd__ip"
                ;;
            ac__subcmd__project,kill)
                cmd="ac__subcmd__project__subcmd__kill"
                ;;
            ac__subcmd__project,login)
                cmd="ac__subcmd__project__subcmd__login"
                ;;
            ac__subcmd__project,logs)
                cmd="ac__subcmd__project__subcmd__logs"
                ;;
            ac__subcmd__project,ls)
                cmd="ac__subcmd__project__subcmd__ls"
                ;;
            ac__subcmd__project,port)
                cmd="ac__subcmd__project__subcmd__port"
                ;;
            ac__subcmd__project,pull)
                cmd="ac__subcmd__project__subcmd__pull"
                ;;
            ac__subcmd__project,restart)
                cmd="ac__subcmd__project__subcmd__restart"
                ;;
            ac__subcmd__project,rm)
                cmd="ac__subcmd__project__subcmd__rm"
                ;;
            ac__subcmd__project,sh)
                cmd="ac__subcmd__project__subcmd__sh"
                ;;
            ac__subcmd__project,start)
                cmd="ac__subcmd__project__subcmd__start"
                ;;
            ac__subcmd__project,stats)
                cmd="ac__subcmd__project__subcmd__stats"
                ;;
            ac__subcmd__project,stop)
                cmd="ac__subcmd__project__subcmd__stop"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        ac)
            opts="-q -h -V --json --quiet --no-color --help --version ls status daemon images df prune config schema completions version project __supervise"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd____supervise)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__completions)
            opts="-q -h -V --json --quiet --no-color --help --version bash zsh fish elvish power-shell"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__config)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__daemon)
            opts="-q -h -V --json --quiet --no-color --help --version status stop"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__daemon__subcmd__status)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__daemon__subcmd__stop)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__df)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__images)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__ls)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project)
            opts="-q -h -V --json --quiet --no-color --help --version start stop down restart ls logs exec sh stats inspect kill rm cp pull images port ip env build login config"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__build)
            opts="-P -q -h -V --profile --root --platform --push --no-push --no-cache --progress --target --builder-cpus --builder-memory --sequential --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --profile)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -P)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --root)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --platform)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --progress)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --target)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --builder-cpus)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --builder-memory)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__config)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__cp)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__down)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__env)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__exec)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__images)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__inspect)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__ip)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__kill)
            opts="-s -q -h -V --signal --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --signal)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -s)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__login)
            opts="-P -q -h -V --profile --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --profile)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -P)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__logs)
            opts="-f -n -q -h -V --follow --tail --boot --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --tail)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -n)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__ls)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__port)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__pull)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__restart)
            opts="-q -h -V --recreate --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__rm)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__sh)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__start)
            opts="-q -h -V --recreate --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__stats)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__project__subcmd__stop)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__prune)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__schema)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__status)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        ac__subcmd__version)
            opts="-q -h -V --json --quiet --no-color --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _ac_wrapper -o nosort -o bashdefault -o default ac
else
    complete -F _ac_wrapper -o bashdefault -o default ac
fi

_ac_project_names() {
    local d f
    for d in "${XDG_CONFIG_HOME:-$HOME/.config}/ac/projects" "${AC_HOME:-$HOME/scripts/ac}/projects"; do
        [ -d "$d" ] || continue
        for f in "$d"/*.json; do
            [ -e "$f" ] || continue
            basename "$f" .json
        done
    done | sort -u
}

_ac_wrapper() {
    local cur projects
    cur="${COMP_WORDS[COMP_CWORD]}"
    projects="$(_ac_project_names)"

    if [ "$COMP_CWORD" -eq 1 ]; then
        _ac_clap "$@"
        COMPREPLY+=( $(compgen -W "$projects" -- "$cur") )
        return
    fi

    if printf '%s\n' "$projects" | grep -qxF -- "${COMP_WORDS[1]}"; then
        COMP_WORDS=("${COMP_WORDS[0]}" project "${COMP_WORDS[@]:1}")
        COMP_CWORD=$((COMP_CWORD + 1))
    fi

    _ac_clap "$@"
}
