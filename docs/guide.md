# ac, in five minutes, for agents and humans

ac runs project-scoped container stacks on Apple `container`, the macOS native
runtime. It fills the role of docker compose: one JSON manifest per project,
`ac <project> start` brings the stack up. If you know docker, you already know
most of ac.

## Discover, then act

```
ac ls                     every project ac can see
ac <project>              status of one project (same as: ac <project> status)
ac <project> services     service names the manifest declares
ac <project> config       the manifest as written
ac schema                 JSON Schema for authoring a manifest
ac guide                  this text
ac guide claude           a CLAUDE.md snippet for making another repo ac-aware
```

Add `--json` to any read command for machine-readable output on stdout with
stable field names. Human log lines move to stderr, so stdout stays one
parseable document. Every underlying `container` command is echoed to stderr
prefixed with `$ ` before it runs; copy one to re-run it by hand. Suppress the
echo with `--quiet` or AC_QUIET=1.

## Docker to ac, command by command

| docker | ac |
| --- | --- |
| docker compose up -d | ac \<project\> start (or: up; -d is accepted and ignored) |
| docker compose down | ac \<project\> down (containers removed, volumes survive) |
| docker compose down -v | ac \<project\> down -v (volumes AND DATA deleted too) |
| docker compose stop / start | ac \<project\> stop / start (restart in place) |
| docker stop -t 5 | ac \<project\> stop -t 5 |
| docker compose restart | ac \<project\> restart |
| docker compose ps | ac \<project\> ls |
| docker compose logs -f | ac \<project\> logs -f |
| docker compose run --rm svc cmd | ac \<project\> run svc cmd (--rm is the default; --keep retains) |
| docker compose pull | ac \<project\> pull |
| docker compose exec svc cmd | ac \<project\> exec svc cmd |
| docker kill -s TERM | ac \<project\> kill -s TERM |
| docker compose rm -f | ac \<project\> rm |
| docker compose create | ac \<project\> create |
| docker exec -it c sh | ac \<project\> sh [svc] |
| docker exec c cmd | ac \<project\> exec svc cmd |
| docker top | ac \<project\> top |
| docker cp | ac \<project\> cp svc:/path ./local |
| docker build | ac \<project\> build |
| docker push | ac \<project\> push -P \<profile\> |
| docker export | ac \<project\> export svc (service must be stopped) |
| docker ps [-a] [-q] | ac ps [-a] [-q] (table includes project and service) |
| docker images | ac image ls (sizes shown by default) |
| docker images -q | ac image ls -q |
| docker rmi ref | ac rmi ref (or: ac image rm) |
| docker pull / push | ac image pull / push \<full-reference\> |
| docker tag | ac image tag src dst |
| docker save / load | ac image save -o f refs... / ac image load -i f |
| docker image prune | ac image prune [--all] |
| docker volume ls/create/rm/inspect/prune | ac volume ... |
| docker network ls/create/rm/inspect/prune | ac network ... |
| docker system df / prune | ac system df / prune |
| docker system prune -a | ac system prune --all |
| docker login | ac registry login -u user server |
| docker inspect | ac \<project\> inspect [svc] or ac image inspect ref |
| docker stats [--no-stream] | ac \<project\> stats [--no-stream] |
| docker wait (rough) | ac \<project\> wait (readiness, not exit) |

Services are addressed by short name (`redis`) or container name
(`shop-redis`). Naming an unknown service fails loudly and lists the valid
ones. When a project name collides with an ac command, use `ac -p <project>`.

More docker habits that just work: `exec -it` and `run -it` parse and are
ignored, since interactivity and TTY allocation are detected automatically.
`--format json` anywhere maps to `--json`. `list`, `delete` and `remove`
work as spellings of `ls` and `rm` in the image, volume and network groups.
`ac help <command>` works for the global commands; project verbs use
`ac <project> <verb> --help` or `ac help project <verb>`.

Discovery commands that read only the manifest, so they work with the
daemon stopped: `ac <project> services`, `ac <project> builds`,
`ac <project> profiles`, `ac <project> config`, `ac <project> images`.

## The rules that are different from docker

1. Daemon ownership. If the `container` daemon was already running, ac never
   stops or restarts it. If ac started it, ac stops it once the last
   ac-managed container across ALL projects is gone. Never run
   `container system stop` yourself; use `ac system stop`, which refuses to
   stop a daemon ac does not own.
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

## Adding a project

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
- Do not stop or restart services you did not start; another agent or the
  user may be relying on them. `ac ps --json` shows what is running and
  which project owns it.
- Destructive commands, in increasing severity: `stop` (container kept),
  `down` and `rm` (container removed, volumes survive), `volumes rm` (data
  gone). Ask before the last one.
