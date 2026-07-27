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
offers "ac <tab>"        "shop"      1 ac ""
offers "ac <tab>"        "status"      1 ac ""
offers "ac <tab>"        "project"     1 ac ""
offers "ac <tab>"        "--json"      1 ac ""
offers "ac no<tab>"      "shop"      1 ac no
omits  "ac no<tab>"      "status"      1 ac no

echo "project actions, bare form"
for action in start stop down restart ls logs exec sh stats inspect kill rm \
              cp pull images port ip env build login config services builds profiles; do
    offers "ac shop <tab>" "$action" 2 ac shop ""
done

echo "project actions, explicit forms"
offers "ac project shop <tab>" "start" 3 ac project shop ""
offers "ac project shop <tab>" "build" 3 ac project shop ""

echo "service values"
for action in start stop down restart rm stats inspect kill pull port ip logs sh env; do
    offers "ac shop $action <tab>" "postgres"        3 ac shop "$action" ""
    offers "ac shop $action <tab>" "shop-postgres" 3 ac shop "$action" ""
done
offers "repeatable services" "clickhouse" 4 ac shop start postgres ""

echo "build values"
offers "ac shop build <tab>" "web"     3 ac shop build ""
offers "ac shop build <tab>" "workers" 3 ac shop build ""
omits  "ac shop build <tab>" "postgres" 3 ac shop build ""

echo "profile values"
for p in local dev pre-prod prod; do
    offers "--profile <tab>" "$p" 5 ac shop build web --profile ""
done
omits "--profile <tab>" "web" 5 ac shop build web --profile ""

echo "per action flags"
offers "start flags"   "--recreate"        3 ac shop start ""
offers "logs flags"    "--follow"          3 ac shop logs ""
offers "logs flags"    "--boot"            3 ac shop logs ""
offers "build flags"   "--platform"        3 ac shop build ""
offers "build flags"   "--builder-memory"  3 ac shop build ""
offers "build flags"   "--no-cache"        3 ac shop build ""
offers "kill flags"    "--signal"          3 ac shop kill ""

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
