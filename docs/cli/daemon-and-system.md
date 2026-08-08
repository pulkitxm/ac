# Daemon, system and host-level commands

Everything on this page acts on the machine rather than on one project: the
`container` daemon itself, the supervisor `ac` spawns to reap it, and the
whole-daemon noun groups (volumes, networks, builder, machines) plus ac's own
introspection commands.

For service stacks see [Project commands](project-commands.md). For the
docker-style verbs that act on a single container see
[Container commands](containers.md). For `ac image` and
`ac registry` see [Images and registries](images-and-registries.md).

## The daemon ownership contract

Apple's `container` runtime has one daemon per machine, shared by everybody.
`ac` therefore follows one rule, and every command on this page is a
consequence of it.

- If the daemon is **already running** when `ac` needs it, `ac` uses it and
  never starts, restarts or stops it. Not on `start`, not on `stop`, not from
  the supervisor. Someone else's containers are running on it.
- If the daemon is **not running**, `ac` starts it with `--app-root` from
  `~/.config/ac/config.json`, records ownership, and becomes responsible for
  stopping it once the last ac-managed container is gone.
- Ownership is a **file**, not process state: `~/.local/state/ac/daemon.owned`.
  A second `ac` in another terminal reads the same file and agrees about who
  may stop the daemon.
- Refcounting spans **all** projects. Two projects up, `ac projA down`, and the
  daemon stays up for projB.

The daemon is started as `container system start [--app-root <path>] --timeout
<startTimeout>`, and stopped as `container system stop`. `--app-root` is not
sticky in Apple `container`, so `ac` passes it on every start.

If `sparseBundle` and `imageMount` are set in the config and the mount point is
not a directory yet, `ac` attaches the bundle with
`hdiutil attach -owners on <bundle>` before starting the daemon. A volume
mounted `noowners` makes `container-apiserver` abort.

### What counts as an ac-managed container

The refcount counts **labels**, not manifest membership.

| Label | Applied by |
| --- | --- |
| `ac.project=<name>` | a manifest service, and one-off `ac <project> run` |
| `ac.managed=1` | `ac run` and `ac create`, which have no project |

A running container carrying either label counts, unioned with the
`<project>-<service>` names the manifests declare (that union covers containers
created by an older `ac` before labelling existed). A container started with
plain `container run` carries no ac label, is never counted, and is never ac's
to stop.

### Three-way daemon gating

Every command decides for itself what it may do to the daemon.

| Bucket | What it does | Commands on this page |
| --- | --- | --- |
| **Require** | Fails with ``container daemon is not running; start it with `ac system start` or any `ac <project> start` `` rather than starting a daemon for a read. | `ac ps`, `ac system df`, `ac df`, `ac volume ls`/`inspect`, `ac network ls`/`inspect`, `ac builder status`, `ac builder stop`, `ac builder delete`, `ac machine` with no args or `ls`/`list`/`inspect`/`logs` |
| **Ensure and release** | Starts the daemon if needed, runs the command, then re-checks the refcount and stops the daemon again if it owns it and nothing ac-managed is running. | `ac system prune`, `ac prune`, `ac volume create`/`rm`/`prune`, `ac network create`/`rm`/`prune`, `ac builder start`, any mutating `ac machine` subcommand |
| **Ensure and supervise** | Starts the daemon, spawns the supervisor, and leaves both up because something is now running. | `ac system start` |

The mutating volume, network and registry subcommands go through one shared
passthrough that also calls the supervisor's `ensure` before running the
command. That is a no-op unless ac owns the daemon, so in practice they behave
as "ensure and release"; `ac system prune` and `ac builder start` skip the
spawn entirely.

`ac status`, `ac daemon status` and `ac system info` read the daemon state
without requiring it: they report `stopped` instead of failing. `ac config`,
`ac ls`, `ac schema`, `ac guide`, `ac version` and `ac completions` never touch
the daemon at all.

`ac system logs` is a straight passthrough with no gating, so it can be run
while the daemon is coming up or wedged. It is also the one command here that
swallows the underlying exit status: `ac system logs` exits 0 even when
`container system logs` fails.

### The supervisor

When `ac` owns the daemon it spawns a detached supervisor
(`nohup ac __supervise`) which polls and stops the daemon when the last
ac-managed container across **all** projects has gone. It exists because
containers also disappear without `ac down`: they crash, exit on their own, or
get stopped with plain `container stop`.

State files, all under `~/.local/state/ac/` (or `$XDG_STATE_HOME/ac/`):

| File | Contents |
| --- | --- |
| `daemon.owned` | Written when ac starts the daemon; its existence is what "owned by ac" means. Removed when ac stops the daemon. |
| `supervisor.pid` | PID of the detached supervisor. A stale PID (process gone) counts as not running. |
| `supervisor.log` | The supervisor's stdout and stderr, appended to. Its `armed` and `idle poll n/m` lines live here. |

The watchdog does not act on a single idle sample.

| Variable | Default | What it does |
| --- | --- | --- |
| `AC_POLL_INTERVAL` | `5` | Seconds between polls. |
| `AC_IDLE_GRACE` | `4` | Consecutive idle polls required before the daemon is stopped. |

- The idle counter **resets to zero** the moment any ac container reappears, so
  a container restarting does not look like an empty stack.
- An **armed** flag means the watchdog never acts before it has seen at least
  one container. Without it, a supervisor spawned during `ac start` would race
  the containers it is waiting for and stop the daemon mid startup.
- The loop also exits immediately (removing its pidfile) if the ownership file
  disappears or the daemon goes away underneath it.

Both variables are read from the environment the supervisor was spawned with,
so a fast watchdog is:

```console
$ AC_POLL_INTERVAL=1 AC_IDLE_GRACE=3 ac shop start
```

`ac __supervise` is the loop itself. It is hidden and not for direct use.

## `ac status`

Daemon, supervisor, then every discoverable project with its services.

```console
$ ac status
daemon  running (owned by ac)  appRoot=/Volumes/ContainerData/app-root/
supervisor  running (pid 48122)

shop - shop local backing services
SERVICE   CONTAINER      STATE     IP              PORTS
postgres  shop-postgres  running   192.168.64.4    5433:5432
redis     shop-redis     running   192.168.64.5    6379:6379
```

No flags of its own. Runs `container system status` plus one
`container ls -a --format json`.

`--json` shape:

```json
{
  "daemon": { "running": true, "ownedByAc": true, "appRoot": "/Volumes/ContainerData/app-root/" },
  "supervisor": { "running": true, "pid": 48122 },
  "projects": [
    {
      "name": "shop",
      "description": "shop local backing services",
      "services": [
        { "service": "redis", "container": "shop-redis", "state": "running",
          "ip": "192.168.64.5/24", "ports": ["6379:6379"], "image": "docker.io/library/redis:7-alpine" }
      ]
    }
  ]
}
```

## `ac daemon`

With no subcommand this is `ac daemon status`.

| Subcommand | What it does |
| --- | --- |
| `status` | Prints one line: whether the daemon runs, who owns it, and its appRoot. |
| `stop` | Kills the supervisor, then stops the daemon **only if ac started it**. |

Neither takes flags. The status line is one of:

| Line | Meaning |
| --- | --- |
| `stopped` | No daemon running. |
| `running (owned by ac)  appRoot=<path>` | `ac` started it and may stop it, and the supervisor will reap it. |
| `running (external, untouched)  appRoot=<path>` | It was already up when ac first needed it. `ac daemon stop` and `ac system stop` will do nothing to it, by design. |

```console
$ ac daemon status
running (external, untouched)  appRoot=/Users/me/Library/Application Support/com.apple.container/

$ ac daemon stop
daemon was not started by ac - leaving it running

$ ac --json daemon status
{
  "running": true,
  "ownedByAc": false,
  "appRoot": "/Users/me/Library/Application Support/com.apple.container/"
}
```

If the ownership file exists but the daemon is already gone, `ac daemon stop`
just removes the stale file and exits 0.

## `ac ps`

Containers across every project, docker ps style. One `container ls -a
--format json`, joined against the manifests so ac-managed containers carry
their project and service names.

| Flag | Default | What it does |
| --- | --- | --- |
| `-a`, `--all` | off | Include containers that are not running. Without it only `running` rows are shown. |
| `-q` | off | Print container names only, one per line. |

Attribution: a row gets a PROJECT when its `ac.project` label matches a known
project, or when its name starts with `<project>-`. It gets a SERVICE only when
the name matches `<project>-<service>` exactly. A labelled container with no
manifest behind it (an `ac run`, or `ac <project> run --keep`) therefore shows
`-` in one or both columns. Requires a running daemon.

```console
$ ac ps
CONTAINER      PROJECT  SERVICE   STATE    IP             IMAGE
shop-postgres  shop     postgres  running  192.168.64.4   docker.io/library/postgres:16-alpine
web            -        -         running  192.168.64.9   my-app:dev

$ ac ps -aq
shop-postgres
shop-redis
web
```

`--json` emits an array (and with `-q`, an array of names):

```json
[
  {
    "container": "shop-postgres",
    "project": "shop",
    "service": "postgres",
    "state": "running",
    "ip": "192.168.64.4/24",
    "image": "docker.io/library/postgres:16-alpine"
  }
]
```

`project`, `service`, `ip` and `image` are `null` when unknown. IPs come back
from the runtime with a prefix length.

## `ac system`

Daemon lifecycle and disk usage. With no subcommand this is `ac system info`.

| Subcommand | Flags | What it runs underneath |
| --- | --- | --- |
| `info` | none | No subprocess beyond `container system status`; prints daemon state, ownership, appRoot and the supervisor pid. Same information as `ac daemon status` plus the supervisor. |
| `df` | none | `container system df` (`--format json` under `--json`). Requires a running daemon. |
| `start` | none | Starts the daemon if it is not running, records ownership, and spawns the supervisor. A daemon that was already up is left exactly as it is. |
| `stop` | none | Kills the supervisor, then `container system stop`, **only** if ac owns the daemon. Same as `ac daemon stop`. |
| `prune` | `-a`, `--all` (default off) | `container prune`, then `container image prune [--all]`, then re-checks whether the daemon can be released. `--all` removes every unused image, not just dangling ones. |
| `logs` | `-f`/`--follow` (off), `--last <PERIOD>` (unset) | `container system logs [-f] [--last <period>]`. These are the logs of the `container` system services themselves, not of any container. Periods look like `10m`, `2h`, `1d`. |

```console
$ ac system start
==> starting container daemon
  app root: /Volumes/ContainerData/app-root/
ok daemon started (owned by ac)

$ ac system prune -a
==> removing stopped containers
==> removing unused images
2 ac container(s) still running across all projects - leaving daemon up

$ ac system logs -f --last 10m
```

`ac system info --json`:

```json
{
  "daemon": { "running": true, "ownedByAc": true, "appRoot": "/Volumes/ContainerData/app-root/" },
  "supervisor": { "running": true, "pid": 48122 }
}
```

### `ac df` and `ac prune`

Top-level shortcuts that route straight through the system group. `ac df` is
`ac system df`. `ac prune` is `ac system prune` with `all` false, so there is
no `-a` on the shortcut; use `ac system prune -a` for that.

```console
$ ac --json df
$ ac prune
```

## `ac volume`

Named volumes across the whole daemon (alias `ac volumes`). With no subcommand
this lists. For the volumes one project declares, use `ac <project> volumes`,
documented in [Project commands](project-commands.md).

| Subcommand | Aliases | Arguments and flags | What it runs underneath |
| --- | --- | --- | --- |
| `ls` | `list` | none | `container volume ls --format json`, rendered as NAME / DRIVER / FORMAT / CREATED. Under `--json` it is `container volume ls --format json` emitted verbatim. Requires a running daemon. |
| `create <name>` | | `name` (required) | `container volume create <name>` |
| `rm <names...>` | `delete`, `remove` | one or more names (required) | `container volume rm <names...>`. **This destroys the data in them.** |
| `inspect <names...>` | | one or more names (required) | `container volume inspect <names...>`, pretty-printed |
| `prune` | | none | `container volume prune`, removing volumes no container references |

Volumes are real ext4 block devices, so a fresh one already contains
`lost+found`. Postgres refuses to initialise into a non-empty directory, which
is why manifests point `PGDATA` at a subdirectory.

```console
$ ac volume ls
NAME           DRIVER  FORMAT  CREATED
shop-pgdata    local   ext4    2026-07-14 09:12
$ ac volume create scratch
$ ac volume rm scratch
```

## `ac network`

Container networks (alias `ac networks`). With no subcommand this lists. Every
container joins `default` unless told otherwise, and networks other than the
default require macOS 26 or newer.

| Subcommand | Aliases | Arguments and flags | What it runs underneath |
| --- | --- | --- | --- |
| `ls` | `list` | none | `container network ls`, or `container network ls --format json` under `--json`. Requires a running daemon. |
| `create <name>` | | `name` (required); `--internal` (off), `--subnet <CIDR>` (unset) | `container network create [--internal] [--subnet <cidr>] <name>`. `--internal` restricts it to host-only networking. |
| `rm <names...>` | `delete`, `remove` | one or more names (required) | `container network rm <names...>` |
| `inspect <names...>` | | one or more names (required) | `container network inspect <names...>`, pretty-printed |
| `prune` | | none | `container network prune` |

```console
$ ac network create devnet --subnet 192.168.66.0/24
$ ac --json network ls
```

Containers get a routable `192.168.64.x` address on the default network, so
services are reachable without publishing ports. ICMP is blocked, so `ping`
fails even when TCP works.

## `ac builder`

The shared image builder container. With no subcommand this is
`ac builder status`.

| Subcommand | Aliases | Flags | Default | What it does |
| --- | --- | --- | --- | --- |
| `status` | | none | | `container builder status` (`--format json` under `--json`). Requires a running daemon. |
| `start` | | `-c`, `--cpus <N>` | unset | CPUs for the builder. Applied only at creation. |
| | | `-m`, `--memory <SIZE>` | unset | Memory for the builder, e.g. `8g`. Applied only at creation. |
| `stop` | | none | | `container builder stop`. Requires a running daemon. |
| `delete` | `rm` | `-f`, `--force` | off | Delete the builder even if it is running. Warns first: deleting discards the layer cache. |

Builder sizing only applies at **creation**. Passing `-c`/`-m` while the
builder is already running is silently ignored by `container`, which is why
`ac build -c/-m` stops and recreates the builder when a resize is needed, and
says so loudly.

```console
$ ac builder status
$ ac builder start -c 8 -m 8g
$ ac builder delete -f
warn deleting the builder discards its layer cache
```

## `ac machine`

`container machine`, passed through verbatim (alias `ac machines`). Everything
after `machine` is forwarded, including flags.

| Argument | Default | What it does |
| --- | --- | --- |
| `[args...]` | `list` | Subcommand and arguments handed to `container machine` unchanged. |

Note there is no `machine start` in `container`: booting happens via `create`,
or implicitly via `machine run`.

Because the arguments are trailing, **global flags must come first**:
`ac --json machine ls`, not `ac machine --json ls`. Gating follows the args:
no args, `ls`, `list`, `inspect` and `logs` require a daemon; anything else
ensures one and releases it afterwards.

```console
$ ac machine
$ ac machine inspect default
$ ac --json machine ls
```

## `ac ls` / `ac projects`

Every project manifest `ac` can see. Discovery is `~/.config/ac/projects/*.json`
(user) then `<repo>/projects/*.json` (bundled, findable via `AC_HOME`); a user
file shadows a bundled one of the same name. No flags, no daemon needed.

```console
$ ac ls
shop
blog
```

`--json` emits one object per project, and an `error` field instead of the
other keys when the manifest fails to parse:

```json
[
  {
    "name": "shop",
    "description": "shop local backing services",
    "file": "/Users/me/.config/ac/projects/shop.json",
    "services": ["postgres", "redis"],
    "builds": ["api"]
  },
  { "name": "broken", "error": "unknown field `porst` at .services[0]" }
]
```

## `ac config`

The resolved ac configuration, read from `~/.config/ac/config.json` (or
`$XDG_CONFIG_HOME/ac/config.json`). No flags. The file is seeded on first run;
if a daemon happens to be running at that moment its current `appRoot` is
adopted, so `ac` keeps using the image store you already have instead of
silently starting a second one.

| Key | Default | What it does |
| --- | --- | --- |
| `appRoot` | `""` | Passed as `--app-root` on every daemon start. Empty means the runtime default. |
| `sparseBundle` | `""` | Disk image attached with `hdiutil attach -owners on` before starting the daemon, when `imageMount` is not already a directory. |
| `imageMount` | `""` | Mount point that tells ac the bundle is already attached. |
| `startTimeout` | `90` | Seconds passed as `--timeout` to `container system start`. |

```console
$ ac config
{
  "appRoot": "/Volumes/ContainerData/app-root/",
  "sparseBundle": "/Volumes/SomeDisk/container-data.sparsebundle",
  "imageMount": "/Volumes/ContainerData",
  "startTimeout": 90
}
```

Output is the same document with or without `--json`; `--json` only moves any
incidental log lines to stderr.

## `ac schema`

The JSON Schema for a project manifest, on stdout. No flags, no daemon. Use it
to author a manifest without guessing at field names; unknown fields are
rejected by `ac`, so the schema is the whole contract.

```console
$ ac schema > manifest.schema.json
```

## `ac guide [topic]`

The manual embedded in the binary.

| Argument | Default | What it prints |
| --- | --- | --- |
| (none) | full guide | `docs/guide.md`: the docker-to-ac command table, the ownership rules, build behaviour and manifest authoring. |
| `claude` | | `docs/claude-snippet.md`, a concise block to paste into another repository's CLAUDE.md so agents working there drive ac correctly. |

```console
$ ac guide | less
$ ac guide claude >> ../my-app/CLAUDE.md
```

## `ac version`

Prints `ac <version>`. `--version` and `-V` work on every subcommand too
(clap's `propagate_version`).

```console
$ ac version
ac 0.3.2

$ ac --json version
{
  "version": "0.3.2"
}
```

## `ac help`

`ac help`, `ac --help` and `-h` print the top-level help; `ac <command> --help`
prints the per-command help, which states what the command does, what it runs
underneath, and gives examples.

## Global flags and environment

These are accepted on every command on this page (see `src/cli/root.rs`), but
must be written **before** any trailing-argument command such as
`ac machine`.

| Flag | Default | What it does |
| --- | --- | --- |
| `--json` | off | Machine readable JSON on stdout instead of a human table. Implies `--quiet` and moves human log lines to stderr, so stdout stays one parseable document. On failure stdout may be empty; the exit code is the contract. |
| `--quiet` | off | Do not echo the underlying `container` commands. Deliberately no short form: `-q` means "names only" on the listings. |
| `--no-color` | off | Disable ANSI colour. |
| `-p <name>`, `--project <name>`, `--project=<name>` | | Escape hatch: treat the next word as a project name even when it collides with one of the commands above. |

| Environment variable | Default | What it does |
| --- | --- | --- |
| `AC_QUIET` | unset | Any value is the same as `--quiet`. |
| `NO_COLOR` | unset | Any value disables colour. |
| `AC_POLL_INTERVAL` | `5` | Supervisor poll interval, in seconds. |
| `AC_IDLE_GRACE` | `4` | Consecutive idle polls before an ac-owned daemon is stopped. |
| `AC_HOME` | derived from the binary path | Where bundled `projects/` is looked for. |
| `XDG_CONFIG_HOME` | `~/.config` | Parent of `ac/config.json` and `ac/projects/`. |
| `XDG_STATE_HOME` | `~/.local/state` | Parent of `ac/daemon.owned`, `ac/supervisor.pid`, `ac/supervisor.log`. |
| `AC_COMPLETE_OFFLINE` | unset | Skip the daemon-backed completers, so TAB never shells out to `container`. |

`--format json` anywhere is rewritten to `--json` for docker muscle memory; any
other `--format` value is an error that says so.

Colour is off automatically when stdout is not a terminal, when `NO_COLOR` is
set, with `--no-color`, or under `--json`. Every underlying `container` command
is echoed to stderr, dimmed and prefixed with `$ `, before it runs, so any step
can be copied and re-run by hand.

Exit codes: `0` on success, `1` on any error raised by `ac` (the message goes to
stderr prefixed `err`). Only the commands that hand you a process's own output,
`ac run`, `ac create`, `ac exec`, `ac sh` and `ac logs`, exit with the exact
underlying status. Every group passthrough on this page, `ac volume`,
`ac network`, `ac system`, `ac builder` and `ac machine` included, turns a
non-zero `container` exit into an ac error (`command exited <status>`) and so
exits `1`.

## `ac completions <shell>`

A static completion script for `bash`, `zsh`, `fish`, `elvish` or `powershell`
(also spelled `power-shell`). It carries no dynamic values and no project
subcommands. The dynamic completions, which offer project names, service names,
container names and image references, come from the `COMPLETE=<shell>` hook
that `make completions` prints.

```console
$ ac completions zsh > ~/.zsh/completions/_ac
```

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [Container commands](containers.md) and
  [Images and registries](images-and-registries.md): the verbs whose daemon
  gating this page explains.
- [Project commands](project-commands.md): the stack-level commands that start
  and release the daemon.
- [Shell completion](completions.md): why a stopped daemon makes TAB empty
  rather than slow.
