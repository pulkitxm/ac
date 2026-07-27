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
              cp pull images port ip env build login config services builds profiles \
              run create top wait push export; do
    offers "ac noveum <tab>" "$action" 2 ac noveum ""
done
offers "up parses even though completion offers the canonical start" "--recreate" 3 ac noveum up ""

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

echo "images noun group"
offers "ac noveum images <tab>"    "ls"      3 ac noveum images ""
offers "ac noveum images <tab>"    "rm"      3 ac noveum images ""
offers "ac noveum images <tab>"    "prune"   3 ac noveum images ""
offers "ac noveum images rm <tab>" "postgres" 4 ac noveum images rm ""
offers "ac noveum images rm <tab>" "web"      4 ac noveum images rm ""
omits  "ac noveum images ls <tab>" "postgres" 4 ac noveum images ls ""

echo "volumes noun group"
offers "ac noveum volumes <tab>"        "ls"            3 ac noveum volumes ""
offers "ac noveum volumes <tab>"        "rm"            3 ac noveum volumes ""
offers "ac noveum volumes <tab>"        "inspect"       3 ac noveum volumes ""
offers "ac noveum volumes <tab>"        "prune"         3 ac noveum volumes ""
offers "ac noveum volumes rm <tab>"     "postgres-data" 4 ac noveum volumes rm ""
omits  "ac noveum volumes rm <tab>"     "postgres"      4 ac noveum volumes rm ""
omits  "ac noveum volumes rm <tab>"     "web"           4 ac noveum volumes rm ""
offers "ac noveum volumes inspect <tab>" "redis-data"   4 ac noveum volumes inspect ""

echo "namespaces do not leak into each other"
omits  "images rm has no volume names"  "postgres-data" 4 ac noveum images rm ""
omits  "build has no service names"     "postgres"      3 ac noveum build ""

echo "compose verbs take service and build values"
offers "ac noveum run <tab>"    "postgres" 3 ac noveum run ""
offers "ac noveum wait <tab>"   "redis"    3 ac noveum wait ""
offers "ac noveum top <tab>"    "postgres" 3 ac noveum top ""
offers "ac noveum create <tab>" "redis"    3 ac noveum create ""
offers "ac noveum export <tab>" "postgres" 3 ac noveum export ""
offers "ac noveum push <tab>"   "web"      3 ac noveum push ""
omits  "ac noveum push <tab>"   "postgres" 3 ac noveum push ""
offers "push takes a profile"   "--profile" 3 ac noveum push ""

echo "global noun groups"
offers "ac <tab>"          "ps"       1 ac ""
offers "ac <tab>"          "image"    1 ac ""
offers "ac <tab>"          "guide"    1 ac ""
offers "ac image <tab>"    "pull"     2 ac image ""
offers "ac image <tab>"    "save"     2 ac image ""
offers "ac image <tab>"    "tag"      2 ac image ""
offers "ac volume <tab>"   "prune"    2 ac volume ""
offers "ac network <tab>"  "create"   2 ac network ""
offers "ac system <tab>"   "df"       2 ac system ""
offers "ac system <tab>"   "stop"     2 ac system ""
offers "ac registry <tab>" "login"    2 ac registry ""
offers "ac guide <tab>"    "claude"   2 ac guide ""

echo "escape hatch project names"
offers "ac project <tab>"     "noveum"   2 ac project ""
offers "ac project no<tab>"   "noveum"   2 ac project no

echo "new flags"
offers "stop flags"  "--time"      3 ac noveum stop ""
offers "down flags"  "--volumes"   3 ac noveum down ""
offers "stats flags" "--no-stream" 3 ac noveum stats ""

echo "nested subcommands"
offers "ac daemon <tab>"      "status"   2 ac daemon ""
offers "ac daemon <tab>"      "stop"     2 ac daemon ""
offers "ac completions <tab>" "zsh"      2 ac completions ""
offers "ac completions <tab>" "fish"     2 ac completions ""

echo "build safety flags"
offers "build flags" "--dry-run" 3 ac noveum build ""

echo "edge cases"
omits "unknown project has no actions" "start" 2 ac nosuchproject ""
offers "global flags still complete"   "--help" 1 ac --

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
