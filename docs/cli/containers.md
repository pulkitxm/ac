# Container commands

These are the manifest-free container verbs. They act on **one container (or a
list of containers) by its real name**, with no project manifest involved, and
they are a thin pass to Apple's `container` CLI with ac's daemon ownership
contract, command echo and `--json` handling layered on top.

For a whole stack declared in a manifest, use the project form instead: see
[Project commands](project-commands.md). `ac stop shop-redis` and
`ac shop stop redis` reach the same container, but only the project form
resolves short service names and knows about readiness.

Image and registry verbs live in
[Images and registries](images-and-registries.md), the noun groups in
[Daemon and system](daemon-and-system.md).

## Contents

- [Naming a container](#naming-a-container)
- [Daemon gating](#daemon-gating)
- [TTY detection](#tty-detection)
- [`run`](#run)
- [`create`](#create)
- [Shared run and create flags](#shared-run-and-create-flags)
- [`start`](#start)
- [`stop`](#stop)
- [`restart`](#restart)
- [`rm`](#rm)
- [`exec`](#exec)
- [`sh`](#sh)
- [`logs`](#logs)
- [`inspect`](#inspect)
- [`kill`](#kill)
- [`cp`](#cp)
- [`export`](#export)
- [`stats`](#stats)
- [`top`](#top)
- [`port`](#port)

## Naming a container

Every command that takes a container accepts two spellings of the same name:

- the real container name, `shop-redis`
- `project/service`, `shop/redis`, which is rewritten to `shop-redis`

Resolution happens against a live `container ls -a` snapshot. Three failure
modes, all loud:

- an unknown name lists the containers that do exist
  (`no such container: web; none exist right now` when there are none)
- a name that is a **project** rather than a container is rejected with a
  pointer to `ac <project> status`
- naming nothing at all on `stop`, `rm` or `kill` is an error, not a silent
  no-op (see the `--all` warning below)

```console
$ ac logs shop/redis
$ ac logs shop-redis      # identical
$ ac logs shop
error: 'shop' is a project, not a container
  try: ac shop status, or name a service directly (ac logs shop-<service>)
```

## Daemon gating

The verbs split three ways, and the split decides whether ac will start the
daemon for you.

| Group | Commands | Behaviour |
| --- | --- | --- |
| Reads | `exec`, `sh`, `logs`, `inspect`, `cp`, `export`, `stats`, `top`, `port` | Require a running daemon. If it is down they fail with ``container daemon is not running; start it with `ac system start` or any `ac <project> start` `` rather than starting one. |
| Mutations that leave nothing running | `stop`, `rm`, `kill` | Also require a running daemon (there is nothing to act on otherwise), then run the cross-project refcount check afterwards so an ac-owned daemon can be released. |
| Mutations that leave a container running | `run`, `create`, `start`, `restart` | Ensure the daemon, record ownership if ac started it, and spawn the supervisor, so the daemon stays up and is still reaped once the container goes. |

Containers created by `ac run` and `ac create` are labelled `ac.managed=1`.
That label is what makes the supervisor count them, so a daemon started for a
one-off `ac run` is not stopped out from under the container it just started.

## TTY detection

Apple `container` fails with ENODEV when `-t` is passed and there is no TTY,
which is what breaks execs in scripts and CI. So ac passes `-t` **only when
stdin and stdout are both terminals**, whatever you asked for. Asking for `-t`
without a terminal prints:

```text
not allocating a TTY: stdin and stdout are not both terminals
```

and continues without it. `ac run` in the foreground and `ac exec` add `-t`
automatically when both are terminals, even without `-t` on the command line.
`ac exec` always passes `-i`; `ac run` passes `-i` when `-i` was given or when
the run is non-detached on a terminal.

## `run`

Run a container from an image, docker run style.

Underneath: `container run [options] <image> [command...]`, always with
`--label ac.managed=1` appended.

```text
ac run [OPTIONS] <IMAGE> [COMMAND...]
```

| Flag | Default | What it does |
| --- | --- | --- |
| `--rm` | off | Remove the container when it exits. |

Everything else comes from the shared option set, see
[Shared run and create flags](#shared-run-and-create-flags).

`COMMAND...` is a trailing var-arg with hyphens allowed, so everything after
the image is handed to the container verbatim. Global flags such as `--json`
and `--quiet` must therefore come **before** the subcommand.

After a **detached** run that succeeded, ac prints one
`http://localhost:<port>` line per published TCP port (UDP ports are skipped).
Sharp edge: the URLs are read back by inspecting the container by name, so they
are printed only when you passed `--name`. A detached run without `--name`
starts fine and prints nothing.

The exit code is the exit code of `container run`.

```console
$ ac run -d --name web -p 3000:3000 my-app:dev
  http://localhost:3000

$ ac run --rm -it docker.io/library/alpine:3.20 sh
/ #

$ ac --json run -d --name web -p 3000:3000 my-app:dev
```

## `create`

Create a container without starting it, docker create style. Start it later
with [`ac start`](#start).

Underneath: `container create [options] <image> [command...]`, same flags as
`run` minus `--progress`, which `container create` does not accept. Passing
`--progress` warns (`container create has no --progress; ignoring it`) and the
flag is dropped.

```text
ac create [OPTIONS] <IMAGE> [COMMAND...]
```

| Flag | Default | What it does |
| --- | --- | --- |
| `--rm` | off | Remove the container when it exits. |

`create` never allocates a TTY on its own: `-i` is passed through when given,
`-t` is recorded on the command line but not added, and no automatic TTY
detection runs (there is no process to attach to yet).

```console
$ ac create --name web -p 3000:3000 -e NODE_ENV=production my-app:dev
$ ac start web
```

## Shared run and create flags

One option set, flattened into both `run` and `create` (`src/cli/run_opts.rs`).
The only difference is `--progress`, which `create` ignores.

### Process and identity

| Flag | Default | run | create | What it does |
| --- | --- | --- | --- | --- |
| `-d`, `--detach` | off | yes | ignored | Run in the background and print the container name. `create` never starts anything, so this has no effect there. |
| `--name <NAME>` | generated | yes | yes | Name for the container. Also what makes `run -d` able to print published URLs. |
| `-i`, `--interactive` | off | yes | yes | Keep stdin open. |
| `-t`, `--tty` | off | yes | recorded, not passed | Request a TTY. Honoured only when stdin and stdout are both terminals; see [TTY detection](#tty-detection). |
| `-u`, `--user <NAME\|UID[:GID]>` | image default | yes | yes | User for the process. |
| `--uid <UID>` | image default | yes | yes | User ID for the process. |
| `--gid <GID>` | image default | yes | yes | Group ID for the process. |
| `-w`, `--workdir <PATH>` (alias `--cwd`) | image default | yes | yes | Initial working directory inside the container. |
| `--entrypoint <CMD>` | image default | yes | yes | Override the image entrypoint. |
| `--init` | off | yes | yes | Run an init process that forwards signals and reaps children. |
| `--init-image <IMAGE>` | built in | yes | yes | Custom init image. |
| `--cidfile <PATH>` | none | yes | yes | Write the container ID to this path. |

### Environment

| Flag | Default | run | create | What it does |
| --- | --- | --- | --- | --- |
| `-e`, `--env <KEY=VALUE>` | none | yes | yes | Environment entry. Repeatable. A bare `KEY` inherits the host value. |
| `--env-file <PATH>` | none | yes | yes | File of `KEY=VALUE` entries. Repeatable. |

### Ports, volumes and mounts

| Flag | Default | run | create | What it does |
| --- | --- | --- | --- | --- |
| `-p`, `--publish <SPEC>` | none | yes | yes | Publish a port, `[host-ip:]host-port:container-port[/protocol]`. Repeatable. |
| `--publish-socket <SPEC>` | none | yes | yes | Publish a socket, `host_path:container_path`. Repeatable. |
| `-v`, `--volume <SPEC>` | none | yes | yes | Bind mount, `source:target`. Repeatable. |
| `--mount <SPEC>` | none | yes | yes | Mount in long form, `type=<>,source=<>,target=<>,readonly`. Repeatable. |
| `--tmpfs <PATH>` | none | yes | yes | Add a tmpfs mount at the given path. Repeatable. |
| `--shm-size <SIZE>` | runtime default | yes | yes | Size of `/dev/shm`, for example `64M`. |
| `--read-only` | off | yes | yes | Mount the root filesystem read-only. |

### Sizing

| Flag | Default | run | create | What it does |
| --- | --- | --- | --- | --- |
| `-c`, `--cpus <N>` | runtime default | yes | yes | CPUs to allocate. This sizes the container's **VM**, not a cgroup. |
| `-m`, `--memory <SIZE>` | runtime default | yes | yes | Memory to allocate, with optional `K`, `M`, `G`, `T` or `P` suffix. |
| `--ulimit <LIMIT>` | none | yes | yes | Resource limit, `<type>=<soft>[:<hard>]`. Repeatable. |

### Image selection

| Flag | Default | run | create | What it does |
| --- | --- | --- | --- | --- |
| `--platform <os/arch[/variant]>` | host | yes | yes | Platform for a multi-platform image. |
| `-a`, `--arch <ARCH>` | host | yes | yes | Architecture for a multi-arch image. `--platform` wins. Note `-a` here is **not** `--all`; on `stop`, `rm` and `kill` the same letter means `--all`. |
| `--os <OS>` | `linux` | yes | yes | OS for a multi-OS image. `--platform` wins. |
| `--scheme <http\|https\|auto>` | `auto` | yes | yes | Registry scheme used when pulling. |
| `--progress <STYLE>` | auto | yes | ignored with a warning | Progress output style for the implicit pull. |
| `--max-concurrent-downloads <N>` | runtime default | yes | yes | Maximum concurrent image layer downloads. |

### Networking and DNS

| Flag | Default | run | create | What it does |
| --- | --- | --- | --- | --- |
| `--network <NAME[,mac=..][,mtu=..]>` | `default` | yes | yes | Attach to a network. Non-default networks need macOS 26 or newer. |
| `--dns <IP>` | inherited | yes | yes | DNS nameserver address. Repeatable. |
| `--dns-domain <DOMAIN>` | inherited | yes | yes | Default DNS domain. |
| `--dns-option <OPTION>` | none | yes | yes | DNS option. Repeatable. |
| `--dns-search <DOMAIN>` | none | yes | yes | DNS search domain. Repeatable. |
| `--no-dns` | off | yes | yes | Do not configure DNS in the container at all. |

### Runtime, capabilities and extras

| Flag | Default | run | create | What it does |
| --- | --- | --- | --- | --- |
| `-l`, `--label <KEY=VALUE>` | none | yes | yes | Container label. Repeatable. ac always appends `ac.managed=1` on top of whatever you pass. |
| `--cap-add <CAP>` | none | yes | yes | Add a Linux capability, for example `CAP_NET_RAW` or `ALL`. Repeatable. |
| `--cap-drop <CAP>` | none | yes | yes | Drop a Linux capability. Repeatable. |
| `-k`, `--kernel <PATH>` | bundled | yes | yes | Custom kernel path. |
| `--runtime <HANDLER>` | default | yes | yes | Runtime handler. |
| `--ssh` | off | yes | yes | Forward the SSH agent socket into the container. |
| `--rosetta` | off | yes | yes | Enable Rosetta in the container. |
| `--virtualization` | off | yes | yes | Expose virtualization capabilities to the container. |

```console
$ ac run -d --name api \
    -p 8080:8080 -e PORT=8080 --env-file .env \
    -v "$PWD/data:/data" --cpus 4 --memory 4g \
    --label team=platform my-api:dev
  http://localhost:8080
```

## `start`

Start one or more stopped or created containers.

Underneath: `container start` once **per container**, because `container start`
takes a single id. Failures are collected and reported at the end, and the
command exits non-zero if any container failed.

```text
ac start [-a] [-i] <CONTAINER...>
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-a`, `--attach` | off | Attach stdout and stderr. |
| `-i`, `--interactive` | off | Attach stdin. |

At least one container is required. Every container that starts prints
`<name> started` followed by its published `http://localhost:<port>` URLs.

```console
$ ac start web api
web started
  http://localhost:3000
api started
  http://localhost:8080
```

## `stop`

Stop one or more running containers.

```text
ac stop [-t SECS] [-s SIG] [-a] [CONTAINER...]
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-t`, `--time <SECS>` | unset | Seconds to wait before killing the container. Passed as `--time N` only when you give it; otherwise Apple `container` uses its own default and ac's kill deadline assumes 5. |
| `-s`, `--signal <SIG>` | none | Send this signal instead. **Bypasses ac's escalation ladder** and runs a plain `container stop --signal <SIG> [--time N]`. |
| `-a`, `--all` | off | Stop every running container on the daemon. |

### The escalation ladder

Without `-s`, ac uses the same ladder as a project stop, because a wedged
container ignores `container stop` and `container kill` alike:

1. `container stop [--time N] <container>` under a kill deadline of
   `max(2 * time, 20)` seconds, where `time` defaults to 5.
2. If the container is still running, `container kill --signal KILL` under a
   10 second deadline, with a warning.
3. If it is *still* running, ac finds that container's own
   `container-runtime-linux` shim (matched on `--uuid <name>` via `pgrep -f`)
   and `kill -9`s it, waits 2 seconds, and re-checks.

Success is decided by **observed state**, not by an exit code: after each rung
ac re-reads the container list. Anything still running at the end is reported
as `still running: <names>` and the command exits non-zero.

### The `--all` blast radius

`-a`/`--all` stops **every** container the daemon has, including other
projects' and other people's. Bare `ac stop` with no target is an error rather
than a no-op, precisely so nobody reaches for `-a` to make it do something:

```text
stop needs a container name, or --all
  for a whole project stack: ac <project> stop
```

```console
$ ac stop -t 30 web
web stopped

$ ac stop -s TERM web       # no escalation, plain container stop --signal TERM
```

## `restart`

Stop then start, docker restart style. The stop half uses the full escalation
ladder above, and the daemon is **not** released in between, so restarting the
only running container never bounces an ac-owned daemon.

```text
ac restart [-t SECS] <CONTAINER...>
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-t`, `--time <SECS>` | unset | Seconds to wait before killing the container during the stop half. When omitted, ac's kill deadline assumes 5. |

Containers that are not currently running are simply started. Published URLs
are printed for each container that comes back. At least one container is
required. Anything that fails to come back up is reported as
`failed to restart: <names>` and the command exits non-zero.

```console
$ ac restart web
web restarted
  http://localhost:3000
```

## `rm`

Remove containers. Volumes survive; images are [`ac image rm`](images-and-registries.md).

```text
ac rm [-f] [-a] [CONTAINER...]
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-f`, `--force` | off | Remove even if the container is running. |
| `-a`, `--all` | off | Remove **every** container on the daemon, passed through as `container rm --all`. |

Alias: `ac delete`.

Same rule as `stop`: with neither a name nor `--all` it errors instead of doing
nothing. `--all` is a whole-daemon blast radius.

```console
$ ac rm -f web api
```

## `exec`

Run a command in a running container.

Underneath: `container exec -i [-t] [flags] <container> <command...>`. The `-i`
is always present.

```text
ac exec [-i] [-t] [-d] [-e KEY=VALUE] [-w PATH] [-u USER] <CONTAINER> <COMMAND...>
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-i`, `--interactive` | off | Accepted for docker muscle memory. `-i` is always passed underneath, so this changes nothing. |
| `-t`, `--tty` | off | Request a TTY. Added only when stdin and stdout are both terminals; a TTY is also added automatically in that case even without `-t`. |
| `-d`, `--detach` | off | Run detached (`--detach`). |
| `-e`, `--env <KEY=VALUE>` | none | Environment entry for the exec'd process. Repeatable. |
| `-w`, `--workdir <PATH>` | container default | Working directory inside the container. |
| `-u`, `--user <NAME\|UID[:GID]>` | container default | User to run as. |

`COMMAND...` is required, is a trailing var-arg, and allows hyphens, so global
flags must come first. The exit code is the exit code of the command inside the
container.

```console
$ ac exec -it web sh
$ ac exec -e PGPASSWORD=pass -u postgres shop-postgres psql -c 'select 1'
```

Note: `container exec` into a container whose exec channel has wedged will
block until you Ctrl-C. ac does not bound this one, only its own readiness
probes.

## `sh`

Open a shell in a running container. ac probes for bash with
`container exec <c> sh -c 'command -v bash'` and runs `bash` when it is there,
otherwise `sh`. It then goes through the same exec path, asking for a TTY (so
you get one when your terminal has one).

```text
ac sh <CONTAINER>
```

No flags. Alias: `ac shell`.

```console
$ ac sh web
root@web:/#
```

## `logs`

Fetch container logs.

Underneath: `container logs [--follow] [--boot] [-n N] <container>`.

```text
ac logs [-f] [-n N] [--boot] <CONTAINER>
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-f`, `--follow` | off | Follow log output. |
| `-n`, `--tail <N>` | all | Number of lines to show from the end. |
| `--boot` | off | Show the VM boot log instead of the container's stdio. |

Exactly one container. To fan out across a whole stack with per-service
prefixes and colours, use `ac <project> logs`.

```console
$ ac logs -f -n 100 shop-redis
```

## `inspect`

Detailed information about containers, pretty-printed.

Underneath: `container inspect <container...>`, one call for all of them. The
output is parsed as JSON and re-printed indented; under `--json` it is emitted
as the single stdout document. If it does not parse as JSON it is printed
through unchanged.

```text
ac inspect <CONTAINER...>
```

No flags. At least one container is required. For images use
[`ac image inspect`](images-and-registries.md).

```console
$ ac inspect web
$ ac --json inspect web api | jq '.[].configuration.publishedPorts'
```

## `kill`

Send a signal to containers.

Underneath: `container kill --signal <SIG> <container...>`.

```text
ac kill [-s SIG] [-a] [CONTAINER...]
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-s`, `--signal <SIG>` | `KILL` | Signal to send. |
| `-a`, `--all` | off | Signal **every** running container on the daemon (`container kill --all`). |

As with `stop` and `rm`, naming nothing without `--all` is an error, and
`--all` is a whole-daemon blast radius. The refcount check runs afterwards.

```console
$ ac kill -s HUP web
```

## `cp`

Copy files between a container and the host.

Underneath: `container cp <src> <dst>`.

```text
ac cp <SRC> <DST>
```

No flags. Alias: `ac copy`.

### Path rewriting

Each side is examined independently. A side is treated as a container path when
it contains a `:`, the part before the `:` is non-empty, and the whole string
is **not** an existing path on the host. In that case the head is resolved as a
container name (including the `project/service` spelling) and rewritten to the
real container name. Anything else is passed through untouched.

```console
$ ac cp ./seed.sql shop/postgres:/tmp/seed.sql   # becomes shop-postgres:/tmp/seed.sql
$ ac cp web:/app/build ./build
```

Every `ac cp` prints a warning first:

```text
container cp is unreliable in Apple container 1.1.0; prefer `ac exec` with shell redirection
```

That is not paranoia. In 1.1.0 copies **into** a container can silently no-op
while exiting 0, and copies out can hang forever; killing the hung `cp` can
wedge the container so `stop` and `kill` stall too. ac passes `cp` through and
cannot mask this.

## `export`

Export a container's filesystem as a tar archive.

Underneath: `container export -o <output> <container>`.

```text
ac export [-o PATH] <CONTAINER>
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-o`, `--output <PATH>` | `<container>.tar` | Output path. |

Apple `container` refuses to export a **running** container, so ac checks the
observed state first and fails with an actionable message rather than letting
the runtime produce a confusing one:

```text
web is running; Apple container can only export a stopped container
  stop it first: ac stop web
```

```console
$ ac stop web
$ ac export web -o /tmp/web.tar
exported web to /tmp/web.tar
```

## `stats`

Live resource usage.

Underneath: `container stats [--no-stream] [container...]`.

```text
ac stats [--no-stream] [CONTAINER...]
```

| Flag | Default | What it does |
| --- | --- | --- |
| `--no-stream` | off | Take one sample and exit instead of streaming. |

With no containers named, every running container is included (ac passes no
names through, letting the runtime decide).

`--json` **implies `--no-stream`**: ac runs
`container stats --no-stream --format json` and kills it after 20 seconds, so
a wedged runtime cannot hang the command. The parsed document is emitted on
stdout as-is.

```console
$ ac stats --no-stream web api
$ ac --json stats web | jq '.[].cpu'
```

## `top`

Processes running inside containers.

There is no `container top`, so ac runs `ps aux` through `container exec` per
container, falling back to plain `ps` when `ps aux` fails (busybox images).
Each container's name is printed before its table.

```text
ac top [CONTAINER...]
```

No flags. With no containers named, every running container is used; if there
are none, ac warns `no running containers` and exits 0.

```console
$ ac top web
web
PID   USER     TIME  COMMAND
    1 root      0:00 node server.js
```

Note this is exec-based, so it inherits the exec channel's failure mode: a
container under heavy load whose exec channel has wedged will not report.

## `port`

Published port mappings for a container.

There is no `container port`, so ac reads
`configuration.publishedPorts` out of `container inspect` and formats it.

```text
ac port <CONTAINER>
```

No flags. Exactly one container.

Human output is one line per mapping, `containerPort/proto -> hostAddress:hostPort`,
with `0.0.0.0` assumed when the runtime reports no host address and `tcp`
assumed when it reports no protocol. Under `--json` the raw `publishedPorts`
array is emitted.

When the container publishes nothing, ac says so and, because Apple containers
get a routable `192.168.64.x` address, prints the container's IP (with the
prefix length stripped) so you can reach it directly anyway.

```console
$ ac port web
3000/tcp -> 0.0.0.0:3000

$ ac port shop-redis
shop-redis publishes no ports
  reachable directly at 192.168.64.4
```

## Sharp edges worth repeating

- **`-q` means three different things** across the CLI, and none of them is the
  global quiet flag. Global quiet is `--quiet` or `AC_QUIET=1`, with no short
  form. `-q` is "names only" on `ac ps` and `ac image ls`, and `--build-quiet`
  on `ac build`.
- **Trailing arguments swallow global flags.** `run`, `create`, `exec`, `cp`
  and `machine` forward everything after their target, so `--json`, `--quiet`
  and `--no-color` must come first: `ac --json run ...`, never
  `ac run --json ...`. The same applies to the `--format json` rewrite, which
  is deliberately skipped once one of those verbs has been seen.
- **`-a` is overloaded.** `--arch` on `run`, `create` and `build`, `--attach`
  on `start`, `--all` on `stop`, `rm`, `kill`, `ps`, `image prune` and
  `system prune`.
- **`container run` sometimes exits non-zero having actually started the
  container.** The project start path sleeps and re-checks; the bare `ac run`
  passes the exit code straight through, so trust `ac ps` over a surprising
  exit code.
- **Every underlying `container` command is echoed to stderr**, dimmed and
  prefixed with `$ `, so any step can be copied and re-run by hand. Suppress
  with `--quiet` or `AC_QUIET=1`.

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [Project commands](project-commands.md): the same verbs against a manifest,
  with ordered startup and readiness.
- [Images and registries](images-and-registries.md): `ac build`, `ac pull`,
  `ac push` and the image store.
- [Daemon, system and host-level commands](daemon-and-system.md): `ac ps`,
  `ac status` and the ownership contract these verbs obey.
- [Global flags and invocation-wide behaviour](global-flags.md): `--json`,
  `--quiet`, TTY detection and exit codes.
