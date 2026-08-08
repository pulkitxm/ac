# Project commands

`ac <project> <action> [services...]` acts on the services a manifest declares.
This is the compose-shaped half of `ac`: ordered startup gated on readiness,
named volume creation, registry login filtered to the images actually involved,
and service name resolution. The manifest-free half is documented in
[Container commands](containers.md) and
[Images and registries](images-and-registries.md); the manifest itself in
[Manifest reference](manifest.md).

`ac <project>` with no action is `ac <project> status`.

When a project name collides with one of ac's own commands (`run`, `build`,
`start`, `logs`, `machine`, ...) use the escape hatch:

```console
$ ac -p build status
$ ac --project=build status
$ ac project build status
```

All three expand to the same thing. `ac <project> <action>` is rewritten to
`ac project <project> <action>` in `src/main.rs` before clap ever sees it,
which is why an unknown first word produces "unknown project or command" with
the list of projects and commands rather than a clap error.

Global flags (`--json`, `--quiet`, `--no-color`) are clap globals, so
`ac --json shop ls` and `ac shop ls --json` both work. What does not work is a
global flag after a trailing-argument action (`run`, `exec`, `cp`, or a
manifest script), because everything after those is forwarded verbatim. Putting
the flags first always works:

```console
$ ac --json shop ls
$ ac --json shop run redis redis-cli info
```

## Service name resolution

Every action that takes services accepts either spelling: the short manifest
name (`redis`) or the container name (`shop-redis`). The prefix `<project>-` is
stripped and the remainder must match a service in the manifest.

Passing no services means every service in the manifest, in declaration order.

A name that does not resolve is a hard error naming the valid set, and nothing
runs:

```console
$ ac shop start redsi
err no such service 'redsi' in project 'shop' (have: postgres redis api)
```

Container names are `<project>-<service>` and named volumes are
`<project>-<volume>`. Both are derived, never configured.

## Readiness

A service may declare `readyCmd` (argv) and `readyTimeout` (seconds, default
**90**). Apple `container` has no healthcheck primitive, so `ac` implements it
by polling `container exec <container> <readyCmd...>` every 2 seconds until it
exits 0. Each probe runs under its own kill deadline (the remaining time,
clamped to between 2 and 20 seconds), so a wedged exec channel cannot hang the
loop.

`start` and `restart` **warn and continue** on timeout:

```text
  waiting for shop-postgres ..... ready
  waiting for shop-redis ........ timeout
warn shop-redis did not become ready within 90s (continuing)
```

`wait` is the action that fails instead. A service with no `readyCmd` is
considered ready by `wait` as soon as its container state is `running`, and is
not waited on at all by `start`.

---

## start (alias `up`)

Ensures the daemon (starting and owning it when it was down), logs in to the
registries the project's images actually come from, creates missing named
volumes, then per service: `container start <name>` when a container already
exists in state `stopped`, `exited` or `created`, otherwise
`container run -d --progress none ...` with the manifest's `--name`,
`--label ac.project=<project>`, `--cpus`, `--memory`, `--env`, `--publish`,
`--volume`, the image, and `args` appended. Waits on `readyCmd` after each
service, then spawns the supervisor.

| Flag | Default | What it does |
| --- | --- | --- |
| `--recreate` | off | `container rm` the existing container and run a fresh one, stopping it first when running. Named volumes and their data survive. |
| `-d`, `--detach` | off | Accepted and ignored, for compose muscle memory. Every container is detached anyway. Hidden from `--help`. |
| `[services...]` | all | Services to start. |

The login filter is computed from **every** service image the manifest declares,
not only the ones named on the command line, so naming one service still
authenticates for the whole project.

A container that is already running and not being recreated prints
`already running` and is left alone. If `container run` exits non-zero, `ac`
sleeps 2 seconds and re-checks the observed state before declaring failure,
because Apple `container` sometimes reports an error having actually started
the container.

```console
$ ac shop start
$ ac shop up redis postgres
$ ac shop start --recreate postgres
```

## create

`container create` with exactly the argv `start` would use minus the `run -d
--progress none` preamble, so a later `start` starts it in place. In order: it
ensures the daemon, logs in to the registries the project's images come from,
then per service creates any missing named volumes and the container, and
finally runs the cross-project daemon refcount check.

| Flag | Default | What it does |
| --- | --- | --- |
| `--recreate` | off | Stop (when running) and `container rm` an existing container before creating it again. |
| `[services...]` | all | Services to create. |

Without `--recreate`, a service whose container already exists prints
`already exists (<state>)` and is skipped.

```console
$ ac shop create
$ ac shop create --recreate api
```

## run

A one-off container built from a service definition, compose `run` style. The
container is named `<project>-<service>-run-<timestamp>` so it can never clash
with the long-running service, and **published ports are not bound**, so it
does not fight the real service for a host port. Ensures the daemon, logs in to
the registry that one image comes from, creates the service's volumes unless
`--no-volumes`, then runs `container run [--rm] -i [-t] ...` with
`--label ac.project=<project>`, the service's cpus, memory, env and (unless
`--no-volumes`) volumes, then the image, then your command (or the service's
`args` when you give none). Exits with the command's exit code, and runs the
cross-project daemon refcount check afterwards.

| Flag | Default | What it does |
| --- | --- | --- |
| `--keep` | off | Do not pass `--rm`; the container survives the command and the name is printed with the `container rm` line to clean it up. |
| `--rm` | off | Accepted and ignored, removal is the default. Hidden. |
| `-i` | off | Accepted and ignored, interactivity is automatic. Hidden. |
| `-t` | off | Accepted and ignored; `-t` is passed to `container` only when stdin and stdout are both terminals. Hidden. |
| `-e`, `--env <KEY=VALUE>` | none | Extra environment entries, repeatable, appended after the manifest's env so they override it. |
| `--no-volumes` | off | Do not attach the service's named volumes. Needed while the long-running service is up, because a named volume is a block device and cannot be attached twice. |
| `<service>` | required | Service whose definition to use. |
| `[command...]` | image default | Command and arguments, taken verbatim (hyphens allowed). |

Use `exec` instead when you want to enter the container that is already
running.

```console
$ ac shop run postgres psql -U user -h shop-postgres
$ ac shop run --no-volumes -e LOG_LEVEL=debug api node scripts/seed.js
$ ac shop run --keep web node --version
```

## top

`ps aux` (falling back to plain `ps`) through `container exec` in each running
service. Requires a running daemon. Services that are not running print
`not running` and are skipped.

| Flag | Default | What it does |
| --- | --- | --- |
| `[services...]` | all | Services to show. |

`--json` emits `[{service, container, processes: [<line>, ...]}]`.

```console
$ ac shop top
$ ac --json shop top api
```

## wait

Polls readiness and exits non-zero on timeout, so scripts and agents can gate
on a stack being up. A service with a `readyCmd` is polled through
`container exec`; a service without one is waited on until its container state
is `running`. Requires a running daemon (it does not start one).

| Flag | Default | What it does |
| --- | --- | --- |
| `--timeout <SECS>` | each service's `readyTimeout` (90 unless set) | Seconds to wait **per service** before giving up on it. |
| `[services...]` | all | Services to wait for. |

Exit codes: 0 when every named service became ready, 1 with
`err not ready: <names>` when any did not. Every service is polled even when an
earlier one timed out, so the error lists all of them.

`--json` emits `[{service, ready: true|false}]` and still exits non-zero when
any entry is false.

```console
$ ac shop wait
$ ac shop wait postgres --timeout 30 && psql -h 127.0.0.1 -p 5433 -U user
$ ac --json shop wait redis
```

## push

Pushes the tags `build` would produce for a profile, without building anything.
Resolves the same interpolation, logs in to the registries those images come
from, then `container image push <tag>` per tag. `postPush` hooks do **not**
run here. See [Builds](builds.md) for how tags are resolved.

| Flag | Default | What it does |
| --- | --- | --- |
| `-P`, `--profile <NAME>` | `$AC_PROFILE`, then `local` | Profile whose registry, account and tag template to use. An unknown profile is an error listing the declared ones. |
| `[names...]` | all builds | Build names to push. An unknown build name is an error listing the valid ones. |

A project that declares no builds is an error. Any failed push makes the
command exit non-zero after the remaining tags have been attempted.

`--json` emits `[{build, profile, tags, pushed}]`.

```console
$ ac shop push --profile pre-prod
$ ac shop push api -P prod
```

## export

`container export -o <output> <project>-<service>`. Apple `container` refuses to
export a running container, so `ac` checks first and tells you to stop the
service. Requires a running daemon.

| Flag | Default | What it does |
| --- | --- | --- |
| `-o`, `--output <PATH>` | `<project>-<service>.tar` in the current directory | Where to write the archive. |
| `<service>` | required | Service to export. |

```console
$ ac shop stop postgres
$ ac shop export postgres -o /tmp/pg.tar
```

## stop

`container stop [--time N] <name>` per running service, then the cross-project
daemon refcount check. **Containers are kept**, so `ac <project> start` brings
them back in place with their filesystem intact. Volumes are untouched.

| Flag | Default | What it does |
| --- | --- | --- |
| `-t`, `--time <SECS>` | unset (`--time` is passed to `container stop` only when given; the escalation grace then defaults to 5) | Seconds to wait before the container is killed, docker `stop -t` style. |
| `[services...]` | all | Services to stop. |

Stopping escalates, because a wedged container ignores `container stop`:
bounded `container stop` (deadline `max(2 x grace, 20)` seconds), then
`container kill --signal KILL`, then terminating that container's own
`container-runtime-linux` shim matched by `--uuid`. Each step warns before it
acts.

The result is verified against a fresh `container ls -a`, not the exit code: a
container still running after the stop makes the command exit non-zero with
`N container(s) did not stop`. Services that were absent or already stopped are
reported and skipped.

Afterwards `ac` counts ac-managed containers across **all** projects and stops
the daemon only if it owns it and nothing is left.

```console
$ ac shop stop
$ ac shop stop -t 30 postgres
```

## down

`container stop` (same escalation ladder) then `container rm` per service, then
the refcount check. Named volumes and their data survive unless you ask
otherwise.

| Flag | Default | What it does |
| --- | --- | --- |
| `-v`, `--volumes` | off | **Also delete the services' named volumes and the data in them**, via `container volume delete <project>-<volume>`. Irreversible. |
| `-t`, `--time <SECS>` | none | Seconds to wait before the container is killed. |
| `[services...]` | all | Services to bring down. |

A volume that cannot be deleted (usually still attached to a container) warns
and the command exits non-zero with `N volume(s) could not be deleted`.

```console
$ ac shop down
$ ac shop down -t 20 api
$ ac shop down -v          # deletes postgres data
```

`stop` versus `down` in one line: `stop` keeps the container so restart is
fast, `down` removes it so the next `start` creates it fresh. Neither touches
volumes without `-v`.

## restart

`stop` then `start` for the named services, **without releasing the daemon in
between**, so restarting the only running project cannot stop and immediately
restart an ac-owned daemon. Uses the same escalation ladder on the way down and
the same readiness wait on the way up.

| Flag | Default | What it does |
| --- | --- | --- |
| `--recreate` | off | Recreate rather than restart in place on the way back up. |
| `[services...]` | all | Services to restart. |

There is deliberately no `-t/--time` here: the stop half always uses the
default grace of 5 seconds.

```console
$ ac shop restart
$ ac shop restart --recreate api
```

## ls (aliases `ps`, `status`)

One `container ls -a --format json`, joined against the manifest. Every service
is shown, including ones that were never created, which appear as `absent`.
If the daemon is not running it warns that state is unknown rather than failing.

| Flag | Default | What it does |
| --- | --- | --- |
| `-a`, `--all` | off | Accepted and ignored, every service is always shown. Hidden. |

Human output is a table of `CONTAINER STATE IP PORTS`. `--json` emits
`[{service, container, state, ip, ports, image}]`, where `ip` is null when
unknown.

```console
$ ac shop
$ ac shop ps
$ ac --json shop status
```

## logs

With a service, this is `container logs [flags] <project>-<service>`. With no
service it fans out: one `container logs` child per service, stdout and stderr
prefixed with the service name padded to 11 characters and coloured per service
the way `docker compose logs` does, and Ctrl-C tears down the whole group.

| Flag | Default | What it does |
| --- | --- | --- |
| `-f`, `--follow` | off | Follow log output. |
| `-n`, `--tail <N>` | none (the runtime's default) | Number of lines to show from the end. |
| `--boot` | off | Show the VM boot log instead of the container's stdio. |
| `[service]` | all, interleaved | Service to read. |

An unknown service name errors with the valid list. There is no `--json` shape
here; log lines are passed through.

```console
$ ac shop logs -f
$ ac shop logs -n 100 postgres
$ ac shop logs --boot api
```

## exec

`container exec -i [-t] <project>-<service> <command...>`. The `-t` is added
only when stdin **and** stdout are terminals, because Apple `container` fails
with ENODEV otherwise. Exits with the command's exit code.

| Flag | Default | What it does |
| --- | --- | --- |
| `-i` | off | Accepted and ignored, interactivity is detected. Hidden. |
| `-t` | off | Accepted and ignored, a TTY is allocated automatically. Hidden. |
| `<service>` | required | Service to run in. |
| `<command...>` | required | Command and arguments, taken verbatim (hyphens allowed). |

Note that an exec into a wedged container blocks until you Ctrl-C; only ac's
own probes are bounded.

```console
$ ac shop exec postgres psql -U user -c 'select 1'
$ ac shop exec redis redis-cli info server
```

## sh (alias `shell`)

`container exec -i [-t] <container> sh -c 'command -v bash >/dev/null && exec
bash || exec sh'`, so you get bash when the image has it and sh otherwise.

| Flag | Default | What it does |
| --- | --- | --- |
| `-i` | off | Accepted and ignored. Hidden. |
| `-t` | off | Accepted and ignored. Hidden. |
| `[service]` | first service in the manifest | Service to enter. |

```console
$ ac shop sh
$ ac shop shell redis
```

## stats

`container stats <containers...>`.

| Flag | Default | What it does |
| --- | --- | --- |
| `--no-stream` | off | Take one sample and exit instead of streaming. |
| `[services...]` | all | Services to include. |

`--json` implies `--no-stream`, adds `--format json`, and is killed after 20
seconds if the runtime wedges. The daemon's own JSON is emitted unchanged.

```console
$ ac shop stats
$ ac shop stats --no-stream api redis
$ ac --json shop stats
```

## inspect

`container inspect <containers...>`, the daemon's full JSON.

| Flag | Default | What it does |
| --- | --- | --- |
| `[services...]` | all | Services to include. |

`--json` parses and re-emits the daemon's document so stdout stays a single
parseable value.

```console
$ ac shop inspect postgres
$ ac --json shop inspect | jq '.[0].status'
```

## kill

`container kill --signal <SIG> <containers...>`.

| Flag | Default | What it does |
| --- | --- | --- |
| `-s`, `--signal <SIG>` | `KILL` | Signal name, without the `SIG` prefix. |
| `[services...]` | all | Services to signal. |

```console
$ ac shop kill
$ ac shop kill -s TERM api
```

## rm

`container rm --force` per service that exists, then the refcount check.
Volumes survive. Absent services are reported and skipped, like `down` does.
Any failure exits non-zero with `N container(s) could not be removed`.

| Flag | Default | What it does |
| --- | --- | --- |
| `[services...]` | all | Services to remove. |

```console
$ ac shop rm api
$ ac shop rm
```

## cp

`container cp <src> <dst>`, with `svc:/path` rewritten to
`<project>-<svc>:/path` on either side. An argument is treated as a container
reference only when it does not start with `/`, has no `/` before the colon, and
the part after the colon starts with `/`; anything else is a host path. An
unknown service before the colon is an error listing the valid ones.

| Flag | Default | What it does |
| --- | --- | --- |
| `<src>` | required | Source, host path or `svc:/path`. |
| `<dst>` | required | Destination, host path or `svc:/path`. |

`container cp` is unreliable in Apple container 1.1.0, so `ac` guards it. When
the source is inside a container it first probes `test -e` with a 10 second
deadline and refuses if the file is missing, or if the container is not
answering exec probes at all (a copy out of a wedged container hangs forever).
When the destination is inside a container it verifies afterwards that
something actually landed, and errors when `cp` reported success but nothing
appeared. Prefer `exec` with shell redirection where you can.

```console
$ ac shop cp ./dump.sql postgres:/tmp/dump.sql
$ ac shop cp postgres:/tmp/out.csv ./out.csv
```

## pull

`container image pull <image>` per service, after ensuring the daemon and
logging in to the registries the project's images come from. A failed pull
warns and the run continues.

| Flag | Default | What it does |
| --- | --- | --- |
| `[services...]` | all | Services whose images to pull. |

```console
$ ac shop pull
$ ac shop pull postgres redis
```

## images

The images this project's services and builds declare. With no subcommand it
lists them, so `ac shop images` keeps working. Build images containing `{{...}}`
are interpolated with `$AC_PROFILE`, else the `local` profile, else the first
declared profile.

| Subcommand | Flags | What it does |
| --- | --- | --- |
| `ls` (default) | none | Table of `NAME IMAGE SIZE LOCAL`. Sizes come from `container image ls --format json`; with the daemon down, SIZE is `-` and LOCAL is `?`. |
| `rm [names...]` | `[names...]` default all | `container image rm <image>` per resolved name. Names are services or builds, either short or `<project>-<name>`. An unknown name is an error listing the valid set. Exits non-zero on any failure. |
| `prune` | none | `container image prune`, removing unused images. |

`rm` and `prune` ensure the daemon and settle the refcount afterwards; `ls`
reads the manifest and works with the daemon stopped.

`--json` on `ls` emits `[{name, image, present, size}]`, where `present` is
null when the daemon is not running.

```console
$ ac shop images
$ ac shop images rm redis
$ ac shop images prune
```

## volumes

The named volumes this project declares. Removing one is the only destructive
operation in ac.

| Subcommand | Flags | What it does |
| --- | --- | --- |
| `ls` (default) | none | Table of `NAME VOLUME STATE`, joining the manifest's declared volumes against `container volume ls`. STATE is `present`, `absent`, or `unknown` with the daemon down. |
| `rm [names...]` | `[names...]` default all this project declares | `container volume delete <project>-<name>`. **This destroys the data.** Names are the manifest names (`postgres-data`) or the full names (`shop-postgres-data`). Exits non-zero on any failure, with a hint that the volume may still be attached. |
| `inspect [names...]` | `[names...]` default all | `container volume inspect <full names...>`. |
| `prune` | none | `container volume prune`, removing volumes no container references, across the whole daemon. |

An unknown volume name is an error listing the declared ones. `rm`, `inspect`
and `prune` ensure the daemon; `ls` does not.

`--json` on `ls` emits `[{name, volume, state}]`.

```console
$ ac shop volumes
$ ac shop volumes rm postgres-data
$ ac shop volumes prune
```

## port

Published port mappings as declared in the manifest, not read from the daemon.

| Flag | Default | What it does |
| --- | --- | --- |
| `[services...]` | all | Services to show. |

`--json` emits `[{service, ports: [{host, container, raw}]}]`; a mapping with
no colon has host and container equal to the raw value.

```console
$ ac shop port
$ ac --json shop port postgres
```

## ip

Container IPs as reported by the daemon (`container ls -a --format json`, with
the prefix length stripped). Apple `container` gives every container a routable
`192.168.64.x` address, so services are reachable without publishing ports.
ICMP is blocked, so `ping` fails even when TCP works.

| Flag | Default | What it does |
| --- | --- | --- |
| `[services...]` | all | Services to show. |

Naming exactly one service prints just the address, so it can be captured
directly. With no arguments, or more than one, it prints a name and address per
line. `--json` emits `[{service, container, ip, state}]`.

```console
$ ac shop ip
$ PGHOST=$(ac shop ip postgres)
```

## env

Environment variables a service is started with, from the manifest. Scalars are
rendered unquoted.

| Flag | Default | What it does |
| --- | --- | --- |
| `<service>` | required | Service to show. |

`--json` emits the env object as written in the manifest.

```console
$ ac shop env postgres
POSTGRES_USER=user
POSTGRES_PASSWORD=pass
```

## build

Builds the project's images: resolves every setting through CLI flag > profile
> build entry > project default, runs preflight hooks, then `container build`,
pushing and running `postPush` when the profile pushes. Flags:
`-P/--profile`, `--root`, `--platform`, `--push`, `--no-push`, `--no-cache`,
`--progress <auto|plain|tty>`, `--target`, `--builder-cpus`,
`--builder-memory`, `--sequential`, `--rollout`, `--no-rollout`, `--dry-run`,
and `[names...]`. Each is documented in full in [Builds](builds.md), and the
rollout pair in [Rollouts](rollouts.md).

```console
$ ac shop build
$ ac shop build api --profile prod
```

## rollout

Runs a profile's `rollout.preflight` then `rollout.run` hooks against images
already pushed, without rebuilding. Flags: `-P/--profile` (default
`$AC_PROFILE`, then `local`), `--root`, `--dry-run`, and `[names...]`
(default every build). See [Rollouts](rollouts.md).

```console
$ ac shop rollout --profile prod
$ ac shop rollout --profile prod --dry-run --json
```

## login

Runs each registry's `passwordCmd` and pipes its stdout to
`container registry login --username <username> --password-stdin <server>`.
Credentials are never stored in the manifest, which suits tokens that expire.
`start`, `create`, `run`, `pull`, `push` and (when the run resolves to pushing)
`build` call this on your behalf, filtered to the registries whose server
appears in the images involved; calling `login` explicitly passes no image
filter, so **every** declared registry is used. Ensures the daemon first.

| Flag | Default | What it does |
| --- | --- | --- |
| `-P`, `--profile <NAME>` | `$AC_PROFILE`, then `local` | Profile whose `{{account}}` and `{{region}}` fill in the server template. |

A registry whose server still contains `{{`, is empty, or starts with `.` is
skipped. A failing `passwordCmd` or login warns and the run continues, since
public images may still pull.

```console
$ ac shop login
$ ac shop login -P prod
```

## config

Prints the project manifest exactly as written on disk, adding a trailing
newline when the file lacks one. `--json` parses and re-emits it.

```console
$ ac shop config
$ ac --json shop config | jq .services
```

## services, builds, profiles

Three manifest-only listings. They read the JSON and nothing else, so they work
with the daemon stopped, which makes them the reliable way for a script or
agent to discover what can be passed where. No flags.

| Command | Prints |
| --- | --- |
| `services` | Service names, in manifest order. These are the arguments to `start`, `stop`, `logs`, `exec` and friends. |
| `builds` | Build names, in manifest order. These are the arguments to `build` and `push`. |
| `profiles` | Profile names, sorted. These are the values for `--profile`. |

`--json` emits a plain array of strings for each.

```console
$ ac shop services
$ ac --json shop builds
$ ac shop profiles
```

## scripts

Lists the manifest's `scripts` map as a `NAME RUNS` table. Manifest-only, so it
works with the daemon stopped. A project with no scripts prints a dimmed hint
naming the manifest file. `--json` emits the map, with each entry either the
string or the `{run, complete}` object as written.

```console
$ ac shop scripts
NAME     RUNS
forward  ~/.config/ac/scripts/shop-tunnels.sh
psql     psql -h 127.0.0.1 -p 5433 -U user postgres
```

## &lt;script&gt; (user defined)

Any name in the manifest's `scripts` map becomes a project subcommand. `ac
<project> <name> [args...]` hands the script string to `sh -c` with the extra
arguments appended **shell-quoted**, and propagates the exit code. `ac` never
interprets the string, so the script owns its own subcommands.

The script's environment carries:

| Variable | Value |
| --- | --- |
| `AC_PROJECT` | the project name |
| `AC_PROJECT_FILE` | absolute path to the manifest that was loaded |
| `AC_PROJECT_ROOT` | the manifest's `root`, set only when the manifest declares one |

Script names must be single words and must not collide with a project action;
the manifest is rejected otherwise. A name that matches neither an action nor a
script errors with both lists:

```console
$ ac shop nope
err no action or script named 'nope' in project 'shop'
  scripts: forward psql
  actions: ac shop --help
```

Shell completion offers script names next to the built-in actions, and offers a
script's declared `complete` words for its arguments. `ac` never executes a
script to complete it.

```console
$ ac shop psql
$ ac shop forward status
$ ac shop forward logs -f
```

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [The project manifest](manifest.md): every field these actions read.
- [Container commands](containers.md): the same verbs without a manifest.
- [Builds](builds.md) and [Rollouts](rollouts.md): `ac <project> build`,
  `push` and `rollout` in full.
- [ac for scripts, CI and agents](agents-and-json.md): the `--json` shapes and
  how to gate a script on `ac <project> wait`.
