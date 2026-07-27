#!/usr/bin/env bash
set -uo pipefail

AC="${AC_BIN:-$(cd "$(dirname "$0")/.." && pwd)/target/release/ac}"
[ -x "$AC" ] || { echo "no binary at $AC, run: make build"; exit 1; }

pass=0
fail=0

complete_at() {
    local idx="$1"; shift
    _CLAP_COMPLETE_INDEX="$idx" COMPLETE=zsh "$AC" -- "$@" 2>/dev/null | cut -d: -f1
}

offers() {
    local label="$1" want="$2" idx="$3"; shift 3
    local got
    got=$(complete_at "$idx" "$@")
    if printf '%s\n' "$got" | grep -qxF -- "$want"; then
        pass=$((pass + 1))
        printf '  ok   %s -> offers %s\n' "$label" "$want"
    else
        fail=$((fail + 1))
        printf '  FAIL %s -> missing %s\n       got: %s\n' \
            "$label" "$want" "$(printf '%s ' $got)"
    fi
}

omits() {
    local label="$1" unwanted="$2" idx="$3"; shift 3
    local got
    got=$(complete_at "$idx" "$@")
    if printf '%s\n' "$got" | grep -qxF -- "$unwanted"; then
        fail=$((fail + 1))
        printf '  FAIL %s -> should not offer %s\n' "$label" "$unwanted"
    else
        pass=$((pass + 1))
        printf '  ok   %s -> omits %s\n' "$label" "$unwanted"
    fi
}

echo "top level"
offers "ac <tab>"        "noveum"      1 ac ""
offers "ac <tab>"        "status"      1 ac ""
offers "ac <tab>"        "project"     1 ac ""
offers "ac <tab>"        "--json"      1 ac ""
offers "ac no<tab>"      "noveum"      1 ac no
omits  "ac no<tab>"      "status"      1 ac no

echo "project actions, bare form"
for action in start stop down restart ls logs exec sh stats inspect kill rm \
              cp pull images port ip env build login config services builds profiles; do
    offers "ac noveum <tab>" "$action" 2 ac noveum ""
done

echo "project actions, explicit forms"
offers "ac project noveum <tab>" "start" 3 ac project noveum ""
offers "ac project noveum <tab>" "build" 3 ac project noveum ""

echo "service values"
for action in start stop down restart rm stats inspect kill pull port ip logs sh env; do
    offers "ac noveum $action <tab>" "postgres"        3 ac noveum "$action" ""
    offers "ac noveum $action <tab>" "noveum-postgres" 3 ac noveum "$action" ""
done
offers "repeatable services" "clickhouse" 4 ac noveum start postgres ""

echo "build values"
offers "ac noveum build <tab>" "web"     3 ac noveum build ""
offers "ac noveum build <tab>" "workers" 3 ac noveum build ""
omits  "ac noveum build <tab>" "postgres" 3 ac noveum build ""

echo "profile values"
for p in local dev pre-prod prod; do
    offers "--profile <tab>" "$p" 5 ac noveum build web --profile ""
done
omits "--profile <tab>" "web" 5 ac noveum build web --profile ""

echo "per action flags"
offers "start flags"   "--recreate"        3 ac noveum start ""
offers "logs flags"    "--follow"          3 ac noveum logs ""
offers "logs flags"    "--boot"            3 ac noveum logs ""
offers "build flags"   "--platform"        3 ac noveum build ""
offers "build flags"   "--builder-memory"  3 ac noveum build ""
offers "build flags"   "--no-cache"        3 ac noveum build ""
offers "kill flags"    "--signal"          3 ac noveum kill ""

echo "nested subcommands"
offers "ac daemon <tab>"      "status"   2 ac daemon ""
offers "ac daemon <tab>"      "stop"     2 ac daemon ""
offers "ac completions <tab>" "zsh"      2 ac completions ""
offers "ac completions <tab>" "fish"     2 ac completions ""

echo "edge cases"
omits "unknown project has no actions" "start" 2 ac nosuchproject ""
offers "global flags still complete"   "--help" 1 ac --

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
