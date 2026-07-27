use crate::cli::CompletionShell;

pub fn with_dynamic_projects(shell: &CompletionShell, generated: &str) -> String {
    match shell {
        CompletionShell::Zsh => zsh(generated),
        CompletionShell::Bash => bash(generated),
        _ => generated.to_string(),
    }
}

fn zsh(generated: &str) -> String {
    let renamed = generated
        .replace("\n_ac() {", "\n_ac_clap() {")
        .replace(
            "\"$funcstack[1]\" = \"_ac\"",
            "\"$funcstack[1]\" = \"_ac_clap\"",
        )
        .replace("\n    _ac \"$@\"", "\n    _ac_clap \"$@\"")
        .replace("\n    compdef _ac ac", "\n    :");

    format!("{renamed}{ZSH_WRAPPER}")
}

fn bash(generated: &str) -> String {
    let renamed = generated
        .replace("\n_ac() {", "\n_ac_clap() {")
        .replace("complete -F _ac ", "complete -F _ac_wrapper ");

    format!("{renamed}{BASH_WRAPPER}")
}

const ZSH_WRAPPER: &str = r#"
_ac_project_names() {
    local d f
    for d in "${XDG_CONFIG_HOME:-$HOME/.config}/ac/projects" "${AC_HOME:-$HOME/scripts/ac}/projects"; do
        [[ -d $d ]] || continue
        for f in $d/*.json(N); do
            print -r -- ${${f:t}:r}
        done
    done | sort -u
}

_ac() {
    local -a projects
    projects=(${(f)"$(_ac_project_names)"})

    if (( CURRENT == 2 )); then
        _ac_clap "$@"
        _describe -t projects 'project' projects
        return
    fi

    if (( ${projects[(I)$words[2]]} )); then
        words=("$words[1]" project "${(@)words[2,-1]}")
        (( CURRENT += 1 ))
    fi

    _ac_clap "$@"
}

compdef _ac ac
"#;

const BASH_WRAPPER: &str = r#"
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
"#;
