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
offers "ac sho<tab>"     "shop"      1 ac sho
omits  "ac sho<tab>"     "status"      1 ac sho

echo "project actions, bare form"
for action in start stop down restart ls logs exec sh stats inspect kill rm \
              cp pull images port ip env build login config services builds profiles \
              run create top wait push export; do
    offers "ac shop <tab>" "$action" 2 ac shop ""
done
offers "up parses even though completion offers the canonical start" "--recreate" 3 ac shop up ""

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

echo "images noun group"
offers "ac shop images <tab>"    "ls"      3 ac shop images ""
offers "ac shop images <tab>"    "rm"      3 ac shop images ""
offers "ac shop images <tab>"    "prune"   3 ac shop images ""
offers "ac shop images rm <tab>" "postgres" 4 ac shop images rm ""
offers "ac shop images rm <tab>" "web"      4 ac shop images rm ""
omits  "ac shop images ls <tab>" "postgres" 4 ac shop images ls ""

echo "volumes noun group"
offers "ac shop volumes <tab>"        "ls"            3 ac shop volumes ""
offers "ac shop volumes <tab>"        "rm"            3 ac shop volumes ""
offers "ac shop volumes <tab>"        "inspect"       3 ac shop volumes ""
offers "ac shop volumes <tab>"        "prune"         3 ac shop volumes ""
offers "ac shop volumes rm <tab>"     "postgres-data" 4 ac shop volumes rm ""
omits  "ac shop volumes rm <tab>"     "postgres"      4 ac shop volumes rm ""
omits  "ac shop volumes rm <tab>"     "web"           4 ac shop volumes rm ""
offers "ac shop volumes inspect <tab>" "redis-data"   4 ac shop volumes inspect ""

echo "namespaces do not leak into each other"
omits  "images rm has no volume names"  "postgres-data" 4 ac shop images rm ""
omits  "build has no service names"     "postgres"      3 ac shop build ""

echo "compose verbs take service and build values"
offers "ac shop run <tab>"    "postgres" 3 ac shop run ""
offers "ac shop wait <tab>"   "redis"    3 ac shop wait ""
offers "ac shop top <tab>"    "postgres" 3 ac shop top ""
offers "ac shop create <tab>" "redis"    3 ac shop create ""
offers "ac shop export <tab>" "postgres" 3 ac shop export ""
offers "ac shop push <tab>"   "web"      3 ac shop push ""
omits  "ac shop push <tab>"   "postgres" 3 ac shop push ""
offers "push takes a profile"   "--profile" 3 ac shop push ""

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
offers "ac project <tab>"     "shop"   2 ac project ""
offers "ac project sho<tab>"  "shop"   2 ac project sho

echo "new flags"
offers "stop flags"  "--time"      3 ac shop stop ""
offers "down flags"  "--volumes"   3 ac shop down ""
offers "stats flags" "--no-stream" 3 ac shop stats ""

echo "nested subcommands"
offers "ac daemon <tab>"      "status"   2 ac daemon ""
offers "ac daemon <tab>"      "stop"     2 ac daemon ""
offers "ac completions <tab>" "zsh"      2 ac completions ""
offers "ac completions <tab>" "fish"     2 ac completions ""

echo "build safety flags"
offers "build flags" "--dry-run" 3 ac shop build ""

echo "global docker verbs"
offers "ac <tab> -> offers run"      "run"    1 ac ""
offers "ac <tab> -> offers build"    "build"  1 ac ""
offers "ac <tab> -> offers logs"     "logs"   1 ac ""
offers "run flags -> --publish"      "--publish"  2 ac run ""
offers "run flags -> --detach"       "--detach"   2 ac run ""
offers "build flags -> --tag"        "--tag"      2 ac build ""
offers "stop flags -> --all"         "--all"      2 ac stop ""
offers "kill signals -> KILL"        "KILL"       3 ac kill -s ""
offers "kill signals -> TERM"        "TERM"       3 ac kill -s ""
offers "builder <tab> -> status"     "status"     2 ac builder ""

echo "edge cases"
omits "unknown project has no project actions" "down" 2 ac nosuchproject ""
offers "known project still has them"          "down" 2 ac shop ""
offers "global flags still complete"   "--help" 1 ac --

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
