# ac, in five minutes, for agents and humans

ac drives Apple `container`, the macOS native runtime, and replaces both halves
of the docker CLI. If you know docker, you already know most of ac.

There are two surfaces, and picking the right one is the only thing you need to
learn:

```
ac <verb> <container|image>     the docker CLI.  No manifest. Acts on one
                                container or image by its real name.
                                ac run, ac build, ac logs, ac exec, ac stop

ac <project> <verb> [services]  the docker compose CLI.  Needs a manifest.
                                Acts on a declared stack, with readiness
                                gating, volumes and registry login handled.
                                ac shop start, ac shop logs -f
```

Use the first for anything ad hoc: a Dockerfile with no project around it, a
one-off container, an image someone else built. Use the second when the thing
is declared in a manifest, because only that form knows about service
ordering, `readyCmd`, named volumes and per-profile registries.

Do not write a manifest just to run one container. `ac run` is right there.

## Discover, then act

```
ac ps                     every container on the daemon, whoever started it
ac ls                     every project ac can see
ac <project>              status of one project (same as: ac <project> status)
ac <project> services     service names the manifest declares
ac <project> config       the manifest as written
ac image ls               every image in the local store
ac schema                 JSON Schema for authoring a manifest
ac guide                  this text
ac guide claude           a CLAUDE.md snippet for making another repo ac-aware
```

`ac ps` is the one that sees everything. `ac ls` and `ac <project> ...` only
know what a manifest declares, so a container from `ac run` appears in `ac ps`
with an empty PROJECT column and nowhere else.

Add `--json` to any read command for machine-readable output on stdout with
stable field names. Human log lines move to stderr, so stdout stays one
parseable document. Every underlying `container` command is echoed to stderr
prefixed with `$ ` before it runs; copy one to re-run it by hand. Suppress the
echo with `--quiet` or AC_QUIET=1.

## docker to ac

Drop the word `docker`. Nearly every plain docker command works as written.

| docker | ac | notes |
| --- | --- | --- |
| docker run -d -p 3000:3000 img | ac run -d -p 3000:3000 img | prints the URL |
| docker run --rm -it img sh | ac run --rm -it img sh | |
| docker create img | ac create img | start it later with ac start |
| docker build -t app:dev . | ac build -t app:dev . | |
| docker start / stop / restart c | ac start / stop / restart c | |
| docker rm [-f] c | ac rm [-f] c | images are ac rmi |
| docker exec -it c sh | ac exec -it c sh | or just: ac sh c |
| docker logs -f c | ac logs -f c | |
| docker inspect c | ac inspect c | |
| docker kill -s TERM c | ac kill -s TERM c | |
| docker cp c:/path ./local | ac cp c:/path ./local | see the cp warning below |
| docker export c | ac export c | container must be stopped |
| docker stats [--no-stream] | ac stats [--no-stream] | |
| docker top c | ac top c | |
| docker port c | ac port c | |
| docker ps [-a] [-q] | ac ps [-a] [-q] | adds PROJECT and SERVICE columns |
| docker images / images -q | ac image ls / ls -q | sizes shown by default |
| docker rmi ref | ac rmi ref (or: ac image rm) | |
| docker pull / push ref | ac pull / push ref | |
| docker tag src dst | ac tag src dst | |
| docker save -o f ref / load -i f | ac save -o f ref / ac load -i f | |
| docker image prune [-a] | ac image prune [--all] | |
| docker login / logout server | ac login -u user server / ac logout server | |
| docker volume ls/create/rm/inspect/prune | ac volume ... | |
| docker network ls/create/rm/inspect/prune | ac network ... | |
| docker system df / prune [-a] | ac system df / prune [--all] | |
| docker buildx ... | ac builder status/start/stop/delete | one shared builder |

Things docker has and Apple `container` does not, so ac cannot offer them:
`--restart` policies, healthchecks, `--cache-from`, `pause`/`unpause`,
`rename`, `commit`, `diff`, `wait`, `attach`, `events`, and `--filter` on the
listings. `ac <project> wait` is readiness polling, which is the nearest thing
to `docker wait` and is not the same.

## docker compose to ac

| docker compose | ac |
| --- | --- |
| docker compose up -d | ac \<project\> start (or: up; -d is accepted and ignored) |
| docker compose down | ac \<project\> down (containers removed, volumes survive) |
| docker compose down -v | ac \<project\> down -v (volumes AND DATA deleted too) |
| docker compose stop / start | ac \<project\> stop / start (restart in place) |
| docker compose restart | ac \<project\> restart |
| docker compose ps | ac \<project\> ls |
| docker compose logs -f | ac \<project\> logs -f (fans out across services) |
| docker compose run --rm svc cmd | ac \<project\> run svc cmd (--rm is the default; --keep retains) |
| docker compose pull | ac \<project\> pull |
| docker compose exec svc cmd | ac \<project\> exec svc cmd |
| docker compose create | ac \<project\> create |
| docker compose rm -f | ac \<project\> rm |
| docker compose build | ac \<project\> build (profiles, interpolation, rollout) |
| docker compose push | ac \<project\> push -P \<profile\> |
| (no equivalent) | ac \<project\> wait, gate scripts on readiness |
| docker build && push && kubectl rollout | ac \<project\> build --rollout -P \<profile\> |
| kubectl rollout restart (after a push) | ac \<project\> rollout -P \<profile\> |

In the project form services are addressed by short name (`redis`) or container
name (`shop-redis`), and naming an unknown service fails loudly and lists the
valid ones. In the global form there is no manifest to resolve against, so you
name the container exactly as `ac ps` prints it; `shop/redis` is accepted as a
spelling of `shop-redis`.

When a project name collides with an ac command, use `ac -p <project>`. The set
of reserved words is now large and includes `run`, `build`, `start`, `stop`,
`rm`, `logs`, `exec`, `top`, `port`, `push`, `tag`, `login` and `machine`, so a
project named after any of them needs `-p` on every invocation.

More docker habits that just work: `exec -it` and `run -it` parse, and `-t` is
honoured only when stdin and stdout are both terminals, because Apple
`container` fails with ENODEV otherwise. `--format json` maps to `--json`.
`list`, `delete`, `remove` and `copy` work as spellings of `ls`, `rm` and `cp`.
`ac help <command>` works for the global commands; project verbs use
`ac <project> <verb> --help`.

Two flags need care because ac's global `--json`/`--quiet` must come before a
verb that forwards its trailing arguments: write `ac --json machine ls`, not
`ac machine --json ls`. The same applies to `ac run`, `ac exec` and `ac cp`,
where everything after the container or image is passed through untouched.

Discovery commands that read only the manifest, so they work with the
daemon stopped: `ac <project> services`, `ac <project> builds`,
`ac <project> profiles`, `ac <project> scripts`, `ac <project> config`,
`ac <project> images`.

## Project scripts

A manifest may declare a `scripts` map, npm run style: a name mapped to one
shell string. `ac <project> <name> [args...]` hands the string to `sh -c`,
appending any extra arguments shell-quoted, and propagates its exit code. ac
does not interpret the string at all; the script owns its own subcommands and
flags, which is how project-specific tooling (ssh tunnels, port-forwards, db
consoles) lives behind the ac front door without ac learning about it.

```json
"scripts": {
  "forward": {
    "run": "~/.config/ac/scripts/noveum-tunnels.sh",
    "complete": ["up", "restart", "stop", "status", "logs", "pg", "ch", "all"]
  },
  "psql": "psql -h 127.0.0.1 -p 5433 -U user postgres"
}
```

```
ac noveum forward            the script decides what no-args means
ac noveum forward status     extra words arrive as $1, $2, ...
ac noveum psql -c 'select 1'
ac noveum scripts            list what the manifest declares
```

The script inherits the caller's environment plus `AC_PROJECT`,
`AC_PROJECT_FILE` (the manifest path) and, when the manifest sets `root`,
`AC_PROJECT_ROOT`. Script names must be single words and cannot shadow ac's
own project actions; the manifest is rejected loudly if they try. Shell
completion offers script names next to the built-in actions.

A script entry is either a plain string or `{"run": ..., "complete": [...]}`.
The `complete` words are what TAB offers for the script's arguments, at every
position: ac never executes a script to complete it, so the manifest simply
lists the subcommands and targets the script understands.

## The rules that are different from docker

1. Daemon ownership. If the `container` daemon was already running, ac never
   stops or restarts it. If ac started it, ac stops it once the last
   ac-managed container is gone, counting across ALL projects. Never run
   `container system stop` yourself; use `ac system stop`, which refuses to
   stop a daemon ac does not own.

   "ac-managed" means carrying an ac label, not being in a manifest. Every
   container ac creates is labelled: `ac.project=<name>` for a service,
   `ac.managed=1` for anything from `ac run` or `ac create`. So a container
   with no manifest anywhere still holds the daemon up for as long as it runs.
   A container you started with plain `container run` carries no label, is
   never counted, and is never ac's to stop.
2. Every container is a lightweight VM. `cpus` and `memory` size the VM.
   Containers get a routable 192.168.64.x IP, so services are reachable
   without publishing ports. ICMP is blocked; a failing ping means nothing.
3. Named volumes are real ext4 devices. A fresh one contains `lost+found`, so
   point PGDATA and similar at a subdirectory. `ac <project> volumes rm` is
   the only data-destroying command in ac.
4. Readiness is ac's own: `readyCmd` in the manifest is polled through
   `container exec`. `ac <project> wait` exits non-zero on timeout, so gate
   follow-up steps on it.
5. `container run` can exit non-zero even though the container started. ac
   already re-checks observed state before declaring failure.
6. A container's runtime shim can wedge, ignoring stop and kill. `ac stop`
   escalates automatically: bounded stop, then SIGKILL, then terminating the
   wedged shim, and only reports success once the container is observed down.

## Builds

For a Dockerfile with no project around it, use the global form. It is a thin
pass to `container build` and takes the docker flags:

```
ac build -t app:dev .
ac build -t app:dev -f docker/Dockerfile --target runner --platform linux/arm64 .
ac run -d -p 3000:3000 app:dev          then run what you just built
```

Everything below is the project form, `ac <project> build`, which is a
different and much larger command: profiles, `{{...}}` interpolation, parallel
builds, live progress, filtered registry login, and rollout hooks. None of that
applies to the global `ac build`.

```
ac <project> build                     every build, parallel, live progress
ac <project> build web -P pre-prod     one build, one profile
ac <project> build --dry-run --json    the resolved plan, nothing executed
ac <project> push -P pre-prod          push already-built tags, no rebuild
```

Settings resolve CLI flag > profile > build entry > project default. On a TTY
each build renders one live line: step position, instruction, per-step and
total elapsed. `--progress plain` streams raw buildkit lines instead. When a
build fails, the last output lines are replayed so the cause is visible.
`--json` emits a per-build summary: tags, seconds, steps, pushed, error.

Registry login is filtered: a registry is only contacted when an image
actually comes from it, and `passwordCmd` in the manifest re-runs on every
start, which suits expiring credentials such as ECR tokens.

The build root prefers, in order: `--root`, `$AC_ROOT`, the git worktree
containing $PWD when it holds the first declared dockerfile, `$PWD` outside
git repos when it holds every dockerfile, the manifest `root`, `$PWD`. So
running a build from inside a worktree builds that worktree.

## Rollouts

`ac` does not deploy. It runs the hooks a profile declares and hands them the
image references it just pushed, so build, push and ship become one command
while the deployment logic stays in your repo.

```
ac <project> build --rollout -P prod    build, push, then roll out
ac <project> rollout -P prod            roll out what is already pushed
ac <project> rollout -P prod --dry-run  the hooks and their env, nothing run
ac <project> build --no-rollout         never roll out, even if auto is set
```

```json
"prod": {
  "push": true,
  "tag": "latest",
  "rollout": {
    "preflight": [["./scripts/preflight.sh", "app", "workers"]],
    "run":       [["./scripts/rollout.sh", "app", "workers"]]
  }
}
```

`preflight` runs **before anything is built**, so an unreachable cluster or
expired credentials fail in seconds rather than after a long build. `run`
fires only once every build and push in that invocation has succeeded. A
non-zero exit from either aborts. Because the block hangs off the profile,
each profile gets its own blast radius: one may restart every deployment,
another only the pre-prod ones. `"auto": true` rolls out without the flag.

Hooks are argv from the build root, with `{{...}}` interpolation plus
`{{image.<build>}}`, and receive the resolved references in the environment:

| Variable | Value |
| --- | --- |
| `AC_IMAGE_<BUILD>` | primary tag, e.g. `AC_IMAGE_WEB` (`-` becomes `_`) |
| `AC_IMAGES_<BUILD>` | every tag for that build, space separated |
| `AC_IMAGES` | every tag pushed in this run |
| `AC_BUILDS` | build names in this run, so a hook can tell what is fresh |
| `AC_PROFILE`, `AC_ACCOUNT`, `AC_REGISTRY`, `AC_TAG`, `AC_REGION` | profile values |
| `AC_VERSION`, `AC_GIT_SHA`, `AC_GIT_SHORT_SHA`, `AC_GIT_BRANCH`, `AC_GIT_DIRTY` | source values |

`AC_IMAGE_*` is set for every build the manifest declares, not only the ones
built, so a hook can pin a service that was not rebuilt. Check `AC_BUILDS`
before doing that, and verify the tag exists.

## Adding a project

A manifest buys you one command that brings up several services in order,
gated on readiness, with named volumes and registry login handled. If you want
one container, you do not need one: `ac run` is the answer.

Write `~/.config/ac/projects/<name>.json`, validate with `ac <name> config`,
then `ac <name> start`. `ac schema` gives the full schema; unknown fields are
rejected by name, so typos surface immediately. A file in the ac repo's
`projects/` directory ships with the tool; a user file of the same name wins.

## Agent etiquette

- Prefer `--json` and parse stdout; treat stderr as commentary. On failure
  stdout may be empty; the exit code is the contract.
- Gate on exit codes: `wait`, `build`, `push` and `run` all propagate failure.
- `wait` enforces its timeout as a wall clock even when a readiness probe
  itself wedges, so it is safe to gate on unconditionally.
- Do not stop or restart containers you did not start; another agent or the
  user may be relying on them. `ac ps --json` shows what is running and, where
  a manifest declares it, which project owns it. A container started by
  `ac run` has no project, which makes it someone's ad hoc work, not yours.
- Name your containers. `ac run --name <n>` makes a container findable and
  stoppable later; without it Apple `container` assigns a UUID.
- Destructive commands, in increasing severity: `stop` (container kept),
  `down` and `rm` (container removed, volumes survive), `volumes rm` and
  `volume rm` (data gone, unrecoverable). Ask before the last one.
- `--all` is a blast radius, not a convenience. `ac stop -a`, `ac rm -a` and
  `ac kill -a` act on EVERY container on the daemon, including other projects'
  and other people's. Name your targets instead. `ac system prune --all` is
  the same hazard for images.
