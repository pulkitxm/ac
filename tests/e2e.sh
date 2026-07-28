#!/usr/bin/env bash

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AC="$REPO/target/release/ac"
PROJ_DIR="$HOME/.config/ac/projects"
STATE_DIR="$HOME/.local/state/ac"
OWNER="$STATE_DIR/daemon.owned"
SUP_PID="$STATE_DIR/supervisor.pid"
BUILD_ROOT="/tmp/ac-e2e-build"
IMAGE="docker.io/library/alpine:3.20"

[ -x "$AC" ] || { echo "build first: make build"; exit 1; }

PASS=0; FAIL=0
RESULTS=()

pass() { PASS=$((PASS+1)); RESULTS+=("PASS|$1|$2"); printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { FAIL=$((FAIL+1)); RESULTS+=("FAIL|$1|$2"); printf '  \033[31mFAIL\033[0m %s\n     %s\n' "$1" "$2"; }
scen() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
note() { printf '  \033[2m%s\033[0m\n' "$*"; }

check() {
  if [ "$2" = "$3" ]; then pass "$1" "$2"; else fail "$1" "expected [$3] got [$2]"; fi
}

check_contains() {
  case "$2" in
    *"$3"*) pass "$1" "found: $3" ;;
    *)      fail "$1" "missing [$3] in: $(printf '%s' "$2" | head -c 400)" ;;
  esac
}

check_not_contains() {
  case "$2" in
    *"$3"*) fail "$1" "unexpectedly found [$3]" ;;
    *)      pass "$1" "absent: $3" ;;
  esac
}

daemon_up()   { container system status >/dev/null 2>&1; }
owned()       { [ -f "$OWNER" ] && echo yes || echo no; }
sup_running() {
  [ -f "$SUP_PID" ] || { echo no; return; }
  local p; p=$(cat "$SUP_PID" 2>/dev/null)
  if [ -n "$p" ] && kill -0 "$p" 2>/dev/null; then echo yes; else echo no; fi
}

cstate() {
  container ls -a --format json 2>/dev/null | python3 -c "
import json,sys
try: d=json.load(sys.stdin)
except Exception: d=[]
for c in d:
    if c.get('id')=='$1': print(c.get('status',{}).get('state','unknown')); break
else: print('absent')
"
}

started_at() {
  container ls -a --format json 2>/dev/null | python3 -c "
import json,sys
try: d=json.load(sys.stdin)
except Exception: d=[]
for c in d:
    if c.get('id')=='$1': print(c.get('status',{}).get('startedDate','')); break
else: print('')
"
}

running_ids() {
  container ls --format json 2>/dev/null | python3 -c "
import json,sys
try: d=json.load(sys.stdin)
except Exception: d=[]
print(' '.join(c['id'] for c in d))
"
}

mkdir -p "$PROJ_DIR" "$BUILD_ROOT"

cat > "$PROJ_DIR/actest1.json" <<'JSON'
{
  "name": "actest1",
  "description": "throwaway ac integration test project",
  "root": "/tmp/ac-e2e-build",
  "profiles": {
    "local": { "platform": "linux/arm64", "push": false, "tag": "e2e", "registry": "" }
  },
  "builds": [
    {
      "name": "tiny",
      "dockerfile": "Dockerfile",
      "context": ".",
      "image": "{{registry}}ac-e2e-tiny",
      "tags": ["{{tag}}"],
      "preflight": [["sh", "-c", "echo preflight-ran"]]
    },
    {
      "name": "failhook",
      "dockerfile": "Dockerfile",
      "context": ".",
      "image": "{{registry}}ac-e2e-never",
      "tags": ["{{tag}}"],
      "preflight": [["sh", "-c", "exit 3"]]
    }
  ],
  "services": [
    {
      "name": "alpha",
      "image": "docker.io/library/alpine:3.20",
      "cpus": 1,
      "memory": "256m",
      "ports": ["18099:8080"],
      "env": { "AC_E2E": "alpha" },
      "volumes": [{ "name": "alpha-data", "target": "/data" }],
      "args": ["sleep", "infinity"],
      "readyCmd": ["sh", "-c", "test -d /data"],
      "readyTimeout": 30
    },
    {
      "name": "beta",
      "image": "docker.io/library/alpine:3.20",
      "cpus": 1,
      "memory": "256m",
      "args": ["sleep", "infinity"],
      "readyCmd": ["true"],
      "readyTimeout": 30
    }
  ]
}
JSON

cat > "$PROJ_DIR/actest2.json" <<'JSON'
{
  "name": "actest2",
  "description": "second throwaway project, for cross-project daemon refcounting",
  "services": [
    {
      "name": "solo",
      "image": "docker.io/library/alpine:3.20",
      "cpus": 1,
      "memory": "256m",
      "args": ["sleep", "infinity"],
      "readyCmd": ["true"],
      "readyTimeout": 30
    }
  ]
}
JSON

cat > "$BUILD_ROOT/Dockerfile" <<'DOCKER'
FROM docker.io/library/alpine:3.20 AS base
RUN echo base > /base.txt
FROM base AS final
RUN echo final > /final.txt
DOCKER

if [ ! -f "$REPO/extras/express-app/Dockerfile" ]; then
  mkdir -p "$REPO/extras/express-app"
  cat > "$REPO/extras/express-app/package.json" <<'JSON'
{
  "name": "ac-playground-express",
  "version": "1.4.2",
  "private": true,
  "main": "server.js",
  "scripts": { "start": "node server.js" },
  "dependencies": { "express": "^4.19.2" }
}
JSON
  cat > "$REPO/extras/express-app/server.js" <<'JS'
const express = require("express");
const app = express();
const port = process.env.PORT || 3000;
app.get("/", (_req, res) => {
  res.json({ app: "ac-playground-express", env: process.env.APP_MESSAGE || "no message set" });
});
app.get("/healthz", (_req, res) => res.status(200).send("ok"));
app.listen(port, () => console.log(`listening on ${port}`));
JS
  cat > "$REPO/extras/express-app/Dockerfile" <<'DOCKER'
FROM docker.io/library/node:20-alpine AS deps
WORKDIR /app
COPY package.json ./
RUN npm install --omit=dev

FROM docker.io/library/node:20-alpine AS runner
WORKDIR /app
ENV NODE_ENV=production
COPY --from=deps /app/node_modules ./node_modules
COPY package.json server.js ./
RUN addgroup -S app && adduser -S app -G app
USER app
EXPOSE 3000
CMD ["node", "server.js"]
DOCKER
  note "recreated extras/express-app playground"
fi

PRE_RUNNING=""
PRE_DAEMON="no"
daemon_up && PRE_DAEMON="yes"
if [ "$PRE_DAEMON" = "yes" ]; then
  PRE_RUNNING=$(running_ids)
fi
APP_ROOT=$(python3 -c "
import json
try: print(json.load(open('$HOME/.config/ac/config.json')).get('appRoot',''))
except Exception: print('')
")

note "daemon was: $PRE_DAEMON"
note "running was: ${PRE_RUNNING:-<none>}"
note "appRoot: ${APP_ROOT:-<unset>}"

restore() {
  printf '\n\033[1m== restoring the environment as found\033[0m\n'
  if [ "${KEEP:-}" != "1" ]; then
    daemon_up || start_daemon_raw
    "$AC" actest1 down >/dev/null 2>&1
    "$AC" actest2 down >/dev/null 2>&1
    container volume rm actest1-alpha-data >/dev/null 2>&1
    container rm --force ac-e2e-run >/dev/null 2>&1
    container image rm ac-e2e-tiny:e2e >/dev/null 2>&1
    container image rm ac-e2e-http:e2e >/dev/null 2>&1
    rm -f "$PROJ_DIR/actest1.json" "$PROJ_DIR/actest2.json"
    rm -rf "$BUILD_ROOT"
    note "test project, containers, volume and image removed"
  fi

  if [ "$PRE_DAEMON" = "yes" ]; then
    daemon_up || start_daemon_raw
    rm -f "$OWNER"
    if [ -f "$SUP_PID" ]; then
      p=$(cat "$SUP_PID" 2>/dev/null); [ -n "$p" ] && kill "$p" 2>/dev/null
      rm -f "$SUP_PID"
    fi
    for c in $PRE_RUNNING; do
      if [ "$(cstate "$c")" != "running" ]; then
        container start "$c" >/dev/null 2>&1 && note "restarted $c"
      fi
    done
    note "daemon running, ownership file cleared"
    note "now running: $(running_ids)"
  fi
}

start_daemon_raw() {
  if [ -n "$APP_ROOT" ]; then
    container system start --app-root "$APP_ROOT" --timeout 90 >/dev/null 2>&1
  else
    container system start --timeout 90 >/dev/null 2>&1
  fi
  sleep 2
}

trap restore EXIT

if daemon_up; then
  note "pre-pulling $IMAGE"
  container image pull "$IMAGE" >/dev/null 2>&1
fi

if [ "$PRE_DAEMON" = "yes" ]; then

scen "a. daemon already running: ac never starts, stops or owns it"
  before_owned=$(owned)
  "$AC" actest1 start >/dev/null 2>&1
  check "a1 start leaves no owner file"      "$(owned)"       "no"
  check "a2 start spawns no supervisor"      "$(sup_running)" "no"
  check "a3 daemon still running after start" "$(daemon_up && echo yes || echo no)" "yes"
  check "a4 containers actually came up"     "$(cstate actest1-alpha)" "running"

  "$AC" actest1 stop >/dev/null 2>&1
  check "a5 stop leaves the daemon running"  "$(daemon_up && echo yes || echo no)" "yes"
  check "a6 stop still claims no ownership"  "$(owned)"       "no"
  check "a7 ownership unchanged end to end"  "$(owned)"       "$before_owned"

scen "d. stop is non-destructive: containers restart in place, volume data survives"
  "$AC" actest1 start >/dev/null 2>&1
  "$AC" actest1 exec alpha sh -c 'echo persisted-value > /data/marker' </dev/null >/dev/null 2>&1
  wrote=$("$AC" actest1 exec alpha cat /data/marker </dev/null 2>/dev/null | tr -d '\r\n')
  check "d1 wrote into the named volume" "$wrote" "persisted-value"

  "$AC" actest1 stop >/dev/null 2>&1
  check "d2 stop leaves the container present, not removed" "$(cstate actest1-alpha)" "stopped"

  out=$("$AC" actest1 start 2>&1)
  check "d3 start brings it back"        "$(cstate actest1-alpha)" "running"
  check_contains "d4 start restarted in place rather than recreating" "$out" "restarting actest1-alpha"
  check_contains "d5 restart used container start, not container run" "$out" '$ container start actest1-alpha'

  read_back=$("$AC" actest1 exec alpha cat /data/marker </dev/null 2>/dev/null | tr -d '\r\n')
  check "d6 volume data survived the stop and start" "$read_back" "persisted-value"

  "$AC" actest1 down >/dev/null 2>&1
  check "d7 down removes the container" "$(cstate actest1-alpha)" "absent"
  vols=$(container volume ls 2>/dev/null | awk 'NR>1 {print $1}')
  check_contains "d8 down left the named volume in place" "$vols" "actest1-alpha-data"

  "$AC" actest1 start >/dev/null 2>&1
  survived=$("$AC" actest1 exec alpha cat /data/marker </dev/null 2>/dev/null | tr -d '\r\n')
  check "d9 data survived down and a full recreate" "$survived" "persisted-value"

scen "e. service targeting"
  a_before=$(started_at actest1-alpha)
  b_before=$(started_at actest1-beta)
  sleep 1
  "$AC" actest1 restart beta >/dev/null 2>&1
  a_after=$(started_at actest1-alpha)
  b_after=$(started_at actest1-beta)
  check "e1 the targeted service restarted" "$([ "$b_before" != "$b_after" ] && echo changed || echo same)" "changed"
  check "e2 the untargeted service was untouched" "$([ "$a_before" = "$a_after" ] && echo same || echo changed)" "same"

  err=$("$AC" actest1 restart nosuchsvc 2>&1); rc=$?
  check "e3 a bad service name fails" "$([ $rc -ne 0 ] && echo nonzero || echo zero)" "nonzero"
  check_contains "e4 the error names the bad service" "$err" "nosuchsvc"
  check_contains "e5 the error lists the valid ones" "$err" "alpha beta"

  out1=$("$AC" actest1 ip alpha 2>/dev/null)
  out2=$("$AC" actest1 ip actest1-alpha 2>/dev/null)
  check "e6 bare and prefixed service names agree" "$out1" "$out2"

scen "f. --json output parses, with the documented fields"
  raw=$("$AC" actest1 ls --json 2>/dev/null)
  got=$(printf '%s' "$raw" | python3 -c "
import json,sys
d=json.load(sys.stdin)
need={'service','container','state','ip','ports','image'}
row=d[0]
print('ok' if need <= set(row) and any(r['container']=='actest1-alpha' for r in d) else 'bad:'+str(sorted(row)))
" 2>&1)
  check "f1 ac <p> ls --json has the expected fields" "$got" "ok"

  got=$("$AC" --json status 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if {'daemon','supervisor','projects'} <= set(d) and {'running','ownedByAc','appRoot'} <= set(d['daemon']) else 'bad:'+str(sorted(d)))
" 2>&1)
  check "f2 ac --json status parses" "$got" "ok"

  got=$("$AC" --json daemon status 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if d['running'] is True and 'ownedByAc' in d else 'bad:'+str(d))
" 2>&1)
  check "f3 ac --json daemon status parses" "$got" "ok"

  got=$("$AC" ls --json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
names={p['name'] for p in d}
print('ok' if {'actest1','actest2'} <= names else 'bad:'+str(names))
" 2>&1)
  check "f4 ac ls --json lists the projects" "$got" "ok"

  got=$("$AC" schema --json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if d.get('\$schema') and 'services' in d.get('properties',{}) else 'bad')
" 2>&1)
  check "f5 ac schema emits a JSON Schema" "$got" "ok"

  errout=$("$AC" actest1 ls --json 2>&1 >/dev/null)
  check_not_contains "f6 --json suppresses the command echo" "$errout" '$ container'

scen "j. every container command is echoed, and AC_QUIET silences it"
  errout=$("$AC" actest1 ls 2>&1 >/dev/null)
  check_contains "j1 the underlying command is echoed to stderr" "$errout" '$ container ls -a --format json'
  errout=$(AC_QUIET=1 "$AC" actest1 ls 2>&1 >/dev/null)
  check_not_contains "j2 AC_QUIET=1 suppresses the echo" "$errout" '$ container'
  errout=$("$AC" --quiet actest1 ls 2>&1 >/dev/null)
  check_not_contains "j3 --quiet suppresses the echo" "$errout" '$ container'

scen "h. build flags reach the echoed container command"
  out=$("$AC" actest1 build tiny --platform linux/arm64 --no-cache --progress plain --target final 2>&1)
  rc=$?
  check_contains "h1 --platform reaches the command"  "$out" "--platform linux/arm64"
  check_contains "h2 --no-cache reaches the command"  "$out" "--no-cache"
  check_contains "h3 --progress reaches the command"  "$out" "--progress plain"
  check_contains "h4 --target reaches the command"    "$out" "--target final"
  check_contains "h5 the tag is interpolated from the profile" "$out" "ac-e2e-tiny:e2e"
  check_contains "h6 the resolved build root is printed" "$out" "build root: "
  check_contains "h7 preflight hooks run" "$out" "preflight-ran"
  check "h8 the build succeeded" "$([ $rc -eq 0 ] && echo ok || echo failed)" "ok"

scen "i. a failing hook aborts the build and reports an error"
  out=$("$AC" actest1 build failhook 2>&1); rc=$?
  check "i1 the build exits non-zero" "$([ $rc -ne 0 ] && echo nonzero || echo zero)" "nonzero"
  check_contains "i2 the failure names the hook" "$out" "preflight failed"
  check_not_contains "i3 it never reached the build itself" "$out" "container build"

scen "l. docker-style global commands"
  got=$("$AC" --json ps 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
hit=[r for r in d if r.get('container')=='actest1-alpha']
print('ok' if hit and hit[0].get('project')=='actest1' and hit[0].get('service')=='alpha' else 'bad:'+str(hit))
" 2>&1)
  check "l1 ac ps --json attributes containers to projects" "$got" "ok"

  got=$("$AC" --json image ls 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if isinstance(d,list) and d else 'bad')
" 2>&1)
  check "l2 ac image ls --json parses" "$got" "ok"

  got=$("$AC" --json volume ls 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if isinstance(d,list) else 'bad')
" 2>&1)
  check "l3 ac volume ls --json parses" "$got" "ok"

  got=$("$AC" --json system info 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if d.get('daemon',{}).get('running') is True else 'bad:'+str(d))
" 2>&1)
  check "l4 ac system info --json parses" "$got" "ok"

  check_contains "l5 ac guide teaches the docker mapping" "$("$AC" guide 2>/dev/null)" "docker compose up"
  check_contains "l6 ac guide claude emits a snippet" "$("$AC" guide claude 2>/dev/null)" "use ac"

  "$AC" system stop >/dev/null 2>&1
  check "l7 system stop refuses to touch an external daemon" "$(daemon_up && echo yes || echo no)" "yes"

  img_out=$("$AC" image ls 2>/dev/null)
  check_contains "l8 image ls shows sizes by default" "$img_out" "SIZE"
  check "l8b one row per tag by default" "$(printf '%s' "$img_out" | grep -c 'alpine  *3.20')" "1"
  check_contains "l8c -v expands per-variant detail" "$("$AC" image ls -v 2>/dev/null)" "FULL SIZE"
  check_contains "l9 ps -q prints bare names" "$("$AC" ps -q 2>/dev/null)" "actest1-alpha"
  check_contains "l10 ps table attributes projects" "$("$AC" ps 2>/dev/null | head -1)" "PROJECT"
  got=$("$AC" ps --format json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if isinstance(d,list) else 'bad')
" 2>&1)
  check "l11 --format json maps to --json" "$got" "ok"
  got=$("$AC" --json version 2>/dev/null | python3 -c "
import json,sys
print('ok' if json.load(sys.stdin).get('version') else 'bad')
" 2>&1)
  check "l12 version --json emits JSON" "$got" "ok"
  got=$("$AC" --json registry ls 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if isinstance(d,list) else 'bad')
" 2>&1)
  check "l13 registry ls --json parses" "$got" "ok"
  "$AC" rmi nosuch-image-zzz:tag >/dev/null 2>&1
  check "l14 rmi propagates failure" "$([ $? -ne 0 ] && echo nonzero || echo zero)" "nonzero"
  "$AC" image inspect nosuch-image-zzz:tag >/dev/null 2>&1
  check "l15 image inspect failure exits non-zero" "$([ $? -ne 0 ] && echo nonzero || echo zero)" "nonzero"
  check_contains "l16 ac help <cmd> works" "$("$AC" help ls 2>/dev/null)" "projects"
  err=$("$AC" exec somesvc true 2>&1); rc=$?
  check "l17 top-level docker verbs hint at project form" "$([ $rc -ne 0 ] && echo nonzero || echo zero)" "nonzero"
  check_contains "l18 the hint names the fix" "$err" "ac <project> exec"

scen "o. manifest-free docker verbs: build, run, and a reachable URL"
  RUNC="ac-e2e-run"
  RUNIMG="ac-e2e-http:e2e"
  cat > "$BUILD_ROOT/Dockerfile.http" <<'DOCKER'
FROM docker.io/library/busybox:1.36
RUN mkdir -p /srv && echo 'ac-e2e-ok' > /srv/index.html
EXPOSE 8080
CMD ["httpd", "-f", "-p", "8080", "-h", "/srv"]
DOCKER

  "$AC" --quiet build -t "$RUNIMG" -f "$BUILD_ROOT/Dockerfile.http" "$BUILD_ROOT" >/dev/null 2>&1
  check "o1 ac build produced the image" \
    "$(container image ls -q 2>/dev/null | grep -c "^ac-e2e-http$")" "1"

  out=$("$AC" run -d --name "$RUNC" -p 18100:8080 "$RUNIMG" 2>&1)
  check "o2 ac run started it"          "$(cstate $RUNC)" "running"
  check_contains "o3 ac run printed the URL" "$out" "http://localhost:18100"

  body=""
  waited=0
  while [ $waited -lt 20 ]; do
    body=$(curl -s --max-time 2 http://localhost:18100/ 2>/dev/null)
    [ -n "$body" ] && break
    sleep 1; waited=$((waited+1))
  done
  check "o4 the published port actually serves" "$(printf '%s' "$body" | tr -d '\n')" "ac-e2e-ok"

  check "o5 it carries the ac.managed label" \
    "$(container inspect $RUNC 2>/dev/null | python3 -c "
import json,sys
print(json.load(sys.stdin)[0]['configuration']['labels'].get('ac.managed','missing'))
" 2>&1)" "1"

  check "o6 the refcount counts it even though no manifest declares it" \
    "$("$AC" --json ps 2>/dev/null | python3 -c "
import json,sys
print('yes' if any(r.get('container')=='$RUNC' for r in json.load(sys.stdin)) else 'no')
" 2>&1)" "yes"

  got=$("$AC" --json port "$RUNC" 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(d[0]['hostPort'] if d else 'none')
" 2>&1)
  check "o7 ac port reports the mapping" "$got" "18100"

  check "o8 ac exec runs inside it" \
    "$("$AC" --quiet exec "$RUNC" cat /srv/index.html 2>/dev/null | tr -d '\n')" "ac-e2e-ok"

  check_contains "o9 ac logs reads it" "$("$AC" --quiet logs "$RUNC" 2>&1)" ""

  "$AC" --quiet stop "$RUNC" >/dev/null 2>&1
  check "o10 ac stop stopped it"  "$(cstate $RUNC)" "stopped"
  "$AC" --quiet start "$RUNC" >/dev/null 2>&1
  check "o11 ac start restarted it in place" "$(cstate $RUNC)" "running"
  "$AC" --quiet rm -f "$RUNC" >/dev/null 2>&1
  check "o12 ac rm removed it"    "$(cstate $RUNC)" "absent"

  err=$("$AC" --quiet stop 2>&1); rc=$?
  check "o13 bare ac stop refuses rather than no-oping" "$rc" "1"
  check_contains "o14 and points at the project form" "$err" "ac <project> stop"

  err=$("$AC" --quiet logs actest1 2>&1); rc=$?
  check "o15 naming a project as a container fails" "$rc" "1"
  check_contains "o16 with a pointer to the project form" "$err" "is a project"

  container image rm "$RUNIMG" >/dev/null 2>&1
  rm -f "$BUILD_ROOT/Dockerfile.http"

scen "m. compose-style verbs"
  "$AC" actest1 up >/dev/null 2>&1; rc=$?
  check "m1 up aliases start" "$([ $rc -eq 0 ] && echo ok || echo failed)" "ok"

  "$AC" actest1 wait --timeout 20 >/dev/null 2>&1; rc=$?
  check "m2 wait exits zero when ready" "$([ $rc -eq 0 ] && echo ok || echo failed)" "ok"

  got=$("$AC" --json actest1 wait 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
print('ok' if all(r['ready'] for r in d) else 'bad:'+str(d))
" 2>&1)
  check "m3 wait --json reports per-service readiness" "$got" "ok"

  out=$("$AC" actest1 run beta echo one-off-ok </dev/null 2>/dev/null)
  check_contains "m4 run executes a one-off command" "$out" "one-off-ok"
  leftovers=$(container ls -a --format json 2>/dev/null | grep -c 'actest1-beta-run')
  check "m5 the one-off container removed itself" "$leftovers" "0"

  out=$("$AC" actest1 run alpha --no-volumes cat /etc/alpine-release </dev/null 2>/dev/null)
  check_contains "m6 run --no-volumes works alongside the live service" "$out" "3.2"

  "$AC" actest1 down beta >/dev/null 2>&1
  "$AC" actest1 create beta >/dev/null 2>&1
  st=$(cstate actest1-beta)
  check "m7 create makes the container without starting it" "$([ "$st" != "absent" ] && [ "$st" != "running" ] && echo ok || echo "bad:$st")" "ok"
  "$AC" actest1 start beta >/dev/null 2>&1
  check "m8 start brings a created container up in place" "$(cstate actest1-beta)" "running"

  out=$("$AC" actest1 top 2>/dev/null)
  check_contains "m9 top shows in-container processes" "$out" "sleep"

  err=$("$AC" actest1 export alpha 2>&1 >/dev/null); rc=$?
  check "m10 export refuses a running service" "$([ $rc -ne 0 ] && echo nonzero || echo zero)" "nonzero"
  check_contains "m11 the refusal says what to do" "$err" "stop"

  "$AC" actest1 stop beta >/dev/null 2>&1
  "$AC" actest1 export beta -o /tmp/ac-e2e-beta.tar >/dev/null 2>&1
  check "m12 export writes a tar of a stopped service" "$([ -s /tmp/ac-e2e-beta.tar ] && echo ok || echo failed)" "ok"
  rm -f /tmp/ac-e2e-beta.tar
  "$AC" actest1 start beta >/dev/null 2>&1

scen "n. build summaries"
  got=$("$AC" --json actest1 build tiny 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
r=d[0]
print('ok' if r['build']=='tiny' and r['ok'] is True and r['steps']['done']>=1 and any('ac-e2e-tiny:e2e' in t for t in r['tags']) else 'bad:'+str(r))
" 2>&1)
  check "n1 build --json emits the outcome summary" "$got" "ok"

  out=$("$AC" actest1 build tiny 2>&1)
  check_contains "n2 non-tty builds stream raw buildkit lines" "$out" "DONE"
  check_contains "n3 the run ends with a summary table" "$out" "BUILD"
  check_contains "n4 and an overall verdict" "$out" "all builds finished"

  "$AC" actest1 build tiny --dry-run --progress bogus >/dev/null 2>&1
  check "n5 --progress rejects unknown values" "$([ $? -ne 0 ] && echo nonzero || echo zero)" "nonzero"

else
  scen "a/d/e/f/h/i/j/l/m/n SKIPPED: no daemon was running when the suite started"
fi

scen "g. shell completions generate"
  "$AC" completions zsh  > /tmp/ac-e2e-comp.zsh  2>/dev/null; rc_z=$?
  "$AC" completions bash > /tmp/ac-e2e-comp.bash 2>/dev/null; rc_b=$?
  check "g1 zsh completion generated"  "$([ $rc_z -eq 0 ] && [ -s /tmp/ac-e2e-comp.zsh ] && echo ok || echo failed)" "ok"
  check "g2 bash completion generated" "$([ $rc_b -eq 0 ] && [ -s /tmp/ac-e2e-comp.bash ] && echo ok || echo failed)" "ok"
  check "g3 zsh completion is syntactically valid"  "$(zsh -n /tmp/ac-e2e-comp.zsh 2>&1 && echo ok)"   "ok"
  check "g4 bash completion is syntactically valid" "$(bash -n /tmp/ac-e2e-comp.bash 2>&1 && echo ok)" "ok"
  check_contains "g5 zsh completion mentions the tool" "$(cat /tmp/ac-e2e-comp.zsh)" "#compdef ac"
  rm -f /tmp/ac-e2e-comp.zsh /tmp/ac-e2e-comp.bash

FOREIGN=""
for c in $PRE_RUNNING; do
  case "$c" in
    actest1-*|actest2-*|buildkit) ;;
    *) FOREIGN="$FOREIGN $c" ;;
  esac
done

if [ -n "$FOREIGN" ] && [ "${AC_E2E_DAEMON:-}" != "1" ]; then
  scen "b/c/k SKIPPED: containers not owned by this suite are running (${FOREIGN# })"
  note "these scenarios stop the daemon, which would stop them too"
  note "set AC_E2E_DAEMON=1 to run them anyway"
else

scen "b/c setup: stopping everything so ac can be the one to start the daemon"
  "$AC" actest1 down >/dev/null 2>&1
  "$AC" actest2 down >/dev/null 2>&1
  for c in $PRE_RUNNING; do
    container stop "$c" >/dev/null 2>&1
  done
  container system stop >/dev/null 2>&1
  sleep 3
  rm -f "$OWNER"
  if [ -f "$SUP_PID" ]; then
    p=$(cat "$SUP_PID" 2>/dev/null); [ -n "$p" ] && kill "$p" 2>/dev/null
    rm -f "$SUP_PID"
  fi
  check "b0 the daemon is stopped" "$(daemon_up && echo yes || echo no)" "no"

scen "b. daemon stopped: ac starts it, owns it, and releases it with the last container"
  out=$("$AC" actest1 start 2>&1)
  check "b1 the daemon is running again"   "$(daemon_up && echo yes || echo no)" "yes"
  check "b2 ac recorded ownership"         "$(owned)" "yes"
  check_contains "b3 it said so"           "$out" "daemon started (owned by ac)"
  check "b4 a supervisor was spawned"      "$(sup_running)" "yes"
  check "b5 the containers came up"        "$(cstate actest1-alpha)" "running"
  check_contains "b6 --app-root was passed on start" "$out" "--app-root"

  out=$("$AC" actest1 down 2>&1)
  sleep 2
  check "b7 the last down stopped the daemon" "$(daemon_up && echo yes || echo no)" "no"
  check "b8 the ownership file is gone"       "$(owned)" "no"
  check_contains "b9 it said so"              "$out" "daemon stopped"
  check "b10 the supervisor is gone"          "$(sup_running)" "no"

scen "c. cross-project refcounting: stopping one project leaves the daemon up for the other"
  "$AC" actest1 start >/dev/null 2>&1
  "$AC" actest2 start >/dev/null 2>&1
  check "c1 both projects are up" \
    "$(cstate actest1-alpha)/$(cstate actest2-solo)" "running/running"
  check "c2 ac owns the daemon" "$(owned)" "yes"

  out=$("$AC" actest1 down 2>&1)
  check "c3 the daemon is still running for the other project" "$(daemon_up && echo yes || echo no)" "yes"
  check "c4 ownership is retained"                              "$(owned)" "yes"
  check "c5 the other project is untouched"                     "$(cstate actest2-solo)" "running"
  check_contains "c6 it explained why the daemon stayed up"     "$out" "still running - leaving daemon up"

  out=$("$AC" actest2 down 2>&1)
  sleep 2
  check "c7 the genuinely last down stops the daemon" "$(daemon_up && echo yes || echo no)" "no"
  check "c8 ownership released"                        "$(owned)" "no"

scen "k. the supervisor debounce reaps the daemon when containers vanish behind ac's back"
  : > "$STATE_DIR/supervisor.log"
  AC_POLL_INTERVAL=1 AC_IDLE_GRACE=3 "$AC" actest1 start >/dev/null 2>&1
  check "k1 ac owns the daemon"     "$(owned)"       "yes"
  check "k2 the supervisor is live" "$(sup_running)" "yes"

  container stop actest1-alpha >/dev/null 2>&1
  container stop actest1-beta  >/dev/null 2>&1
  check "k3 the containers are down without ac's involvement" \
    "$(cstate actest1-alpha)/$(cstate actest1-beta)" "stopped/stopped"

  waited=0
  while [ $waited -lt 40 ]; do
    daemon_up || break
    sleep 1; waited=$((waited+1))
  done
  check "k4 the supervisor stopped the daemon on its own" "$(daemon_up && echo yes || echo no)" "no"
  check "k5 it released ownership"                        "$(owned)" "no"
  check "k6 it cleaned up its own pidfile"                "$(sup_running)" "no"
  note "supervisor acted after ${waited}s (3 idle polls at 1s, plus startup)"

  log_tail=$(tail -20 "$STATE_DIR/supervisor.log" 2>/dev/null)
  check_contains "k7 the log shows it armed before acting"     "$log_tail" "armed"
  check_contains "k8 the log shows consecutive idle polls"     "$log_tail" "idle poll"

fi

printf '\n\033[1m== summary\033[0m\n'
for r in "${RESULTS[@]}"; do
  s="${r%%|*}"; rest="${r#*|}"; n="${rest%%|*}"
  if [ "$s" = "PASS" ]; then printf '  \033[32m%s\033[0m %s\n' "$s" "$n"
  else printf '  \033[31m%s\033[0m %s\n' "$s" "$n"; fi
done
printf '\n  %d passed, %d failed\n' "$PASS" "$FAIL"

[ "$FAIL" -eq 0 ]
