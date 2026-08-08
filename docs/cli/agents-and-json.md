# ac for scripts, CI and agents

Everything in `ac` that matters to a non-human caller: the `--json` contract and
the exact shape each command emits, which stream carries what, what the exit
code promises, how to block until a stack is actually ready, and the
environment variables that turn the decoration off.

If you are writing an agent that will drive `ac`, start with `ac guide` (see
[Self-teaching](#self-teaching-ac-guide)), then come back here for the JSON
shapes.

See also [Global flags](global-flags.md),
[Project commands](project-commands.md),
[Container commands](containers.md),
[Images and registries](images-and-registries.md),
[Builds](builds.md), [Rollouts](rollouts.md) and
[Daemon, system and host-level commands](daemon-and-system.md).

## The five rules

1. `--json` is a global flag. It works on every read command and on the
   commands that produce a report (`build`, `push`, `wait`, `rollout --dry-run`).
2. `--json` implies `--quiet` and disables colour, so stdout is exactly one
   parseable document and nothing else.
3. The command echo (`$ container ...`), `warn` and `err` always go to
   **stderr**. The `==>` and `ok` progress lines normally go to stdout, and
   move to stderr under `--json`, which is what keeps stdout a single document.
4. The **exit code is the contract**. On failure stdout may be empty or absent;
   do not try to parse an error out of stdout.
5. `--format json` is rewritten to `--json` for docker muscle memory. Any other
   `--format` value is a hard error telling you to use `--json`.

```console
$ ac ps --json > containers.json      # stdout: the array. stderr: the echo.
$ ac ps --format json                 # identical to ac ps --json
$ ac ps --format table
err --format table is not supported; ac emits JSON only, use --json
```

### Global flags

| Flag | Default | What it does |
| --- | --- | --- |
| `--json` | off | Machine readable JSON on stdout. Implies `--quiet`, disables colour, moves human lines to stderr. |
| `--quiet` | off | Suppress the `$ container ...` echo and the human log lines. Same as `AC_QUIET=1`. No short form on purpose. |
| `--no-color` | off | Never emit ANSI. Colour is also off automatically when stdout is not a terminal or `NO_COLOR` is set. |

There is no global `-q`. `-q` belongs to the listings (`ac ps -q`,
`ac image ls -q`, meaning names only) and to `ac build -q` (meaning
`--build-quiet`). See `src/cli/root.rs` if you want to see the three
declarations side by side.

### Where the global flags have to go

`run`, `exec`, `cp` and `machine` forward everything after their target
verbatim, so a global flag placed after them is passed to the container, not to
`ac`. Put global flags first.

```console
$ ac --json machine ls      # correct
$ ac machine --json ls      # --json goes to `container machine`
```

The same applies to `--format json` rewriting: inside those passthrough zones
`--format json` is left alone, because it belongs to the command you are
running inside the container.

## JSON shapes, command by command

All output is pretty-printed with a trailing newline. `null` appears wherever a
value is genuinely unknown (no IP yet, no project attribution, daemon down).

### ac version

The value is the crate version, so it moves with the binary.

```json
{ "version": "0.3.2" }
```

### ac config

The resolved `~/.config/ac/config.json`. This command prints JSON with or
without `--json`; the flag only moves the surrounding chatter to stderr.

```json
{
  "appRoot": "/Volumes/ContainerData/app-root/",
  "sparseBundle": "",
  "imageMount": "",
  "startTimeout": 90
}
```

### ac schema

Always JSON, `--json` is not needed. The JSON Schema for a project manifest.

```console
$ ac schema > manifest.schema.json
```

### ac ls (alias ac projects)

One object per discoverable project. A manifest that fails to parse yields
`{name, error}` instead of the full record, so one bad file does not sink the
listing.

```json
[
  {
    "name": "shop",
    "description": "shop local backing services",
    "file": "/Users/me/.config/ac/projects/shop.json",
    "services": ["postgres", "redis"],
    "builds": ["api"]
  },
  { "name": "broken", "error": "unknown field `imagee` at line 7" }
]
```

### ac status

Daemon, supervisor and every project in one document.

```json
{
  "daemon": { "running": true, "ownedByAc": true, "appRoot": "/Volumes/ContainerData/app-root/" },
  "supervisor": { "running": true, "pid": 40122 },
  "projects": [
    {
      "name": "shop",
      "description": "shop local backing services",
      "services": [
        {
          "service": "postgres",
          "container": "shop-postgres",
          "state": "running",
          "ip": "192.168.64.4/24",
          "ports": ["5433:5432"],
          "image": "docker.io/library/postgres:16-alpine"
        }
      ]
    }
  ]
}
```

`state` is whatever the daemon reports (`running`, `stopped`, `created`), or
`absent` when the container was never created. `ip` carries the prefix length,
so strip everything from the `/` if you want a bare address.

### ac daemon status

```json
{ "running": true, "ownedByAc": false, "appRoot": "/Users/me/Library/Application Support/com.apple.container/" }
```

`ownedByAc` is the whole ownership contract in one boolean: true means `ac`
started this daemon and may stop it, false means it was already up and `ac` will
never touch it. `appRoot` is `null` when the daemon is not running.

### ac system info

The daemon and supervisor halves of `ac status`, without the projects.

```json
{
  "daemon": { "running": true, "ownedByAc": true, "appRoot": "/Volumes/ContainerData/app-root/" },
  "supervisor": { "running": true, "pid": 40122 }
}
```

`supervisor.pid` is `null` when no supervisor is alive.

### ac ps

Every container on the daemon, attributed to a project where possible.
`project` and `service` are `null` for a container that no manifest claims (an
`ac run` one-off, or somebody else's container).

```json
[
  {
    "container": "shop-redis",
    "project": "shop",
    "service": "redis",
    "state": "running",
    "ip": "192.168.64.5/24",
    "image": "docker.io/library/redis:7-alpine"
  }
]
```

`ac ps -q --json` emits a flat array of names instead:

```json
["shop-redis", "shop-postgres"]
```

Without `-a`, only `running` containers appear.

### ac image ls

`--json` passes `container image ls --format json` through unchanged, so the
shape is the runtime's: `[{configuration: {name, creationDate}, variants: [...]}]`.
The human table is ac's own summarisation of that. `ac image ls -q --json` emits
a flat array of names.

```console
$ ac --json image ls | jq -r '.[].configuration.name'
```

### ac image inspect, ac volume inspect, ac network inspect, ac inspect

Straight passthrough of `container ... inspect`, re-emitted as pretty JSON.
`ac inspect` (containers) resolves `project/service` and `project-service`
names before inspecting.

```console
$ ac --json inspect shop/redis | jq '.[0].status.state'
```

### ac volume ls, ac network ls, ac registry ls, ac df, ac system df, ac builder status

All of these run the matching `container` verb with `--format json` and emit its
document unchanged. Their shapes are Apple's, not ac's, so treat them as opaque
and index by key.

```console
$ ac --json df
$ ac --json registry ls
$ ac --json builder status
```

### ac stats

`--json` implies `--no-stream` (one snapshot, no streaming) and the underlying
command is killed after 20 seconds if the runtime wedges, which fails the
command rather than hanging your script. Output is the runtime's
`container stats --format json` document.

```console
$ ac --json stats shop-redis
```

### ac port

The `publishedPorts` array lifted out of `container inspect`, verbatim.

```console
$ ac --json port shop-redis
```

An empty array means the container publishes nothing; it is still reachable on
its `192.168.64.x` address.

### ac <project> ls (aliases ps, status)

The per-service join against the manifest.

```json
[
  {
    "service": "postgres",
    "container": "shop-postgres",
    "state": "running",
    "ip": "192.168.64.4/24",
    "ports": ["5433:5432"],
    "image": "docker.io/library/postgres:16-alpine"
  }
]
```

### ac <project> services, builds, profiles

Flat arrays of strings.

```console
$ ac --json shop services
["postgres", "redis"]
```

### ac <project> scripts

An object mapping script name to its manifest value. The value is either the
shell string or the `{"run": ..., "complete": [...]}` object, exactly as
written.

```json
{
  "forward": "~/.config/ac/scripts/shop-tunnels.sh",
  "psql": { "run": "psql -h 127.0.0.1 -p 5433 -U user postgres", "complete": ["prod", "staging"] }
}
```

### ac <project> wait

One entry per service polled, plus a non-zero exit if any is false. See
[Gating on readiness](#gating-on-readiness-ac-project-wait).

```json
[
  { "service": "postgres", "ready": true },
  { "service": "redis", "ready": false }
]
```

### ac <project> images

`present` is `null` when the daemon is not running (presence is unknowable),
otherwise a boolean. `size` is bytes, or `null` when the image is not local.

```json
[
  { "name": "postgres", "image": "docker.io/library/postgres:16-alpine", "present": true, "size": 132214784 },
  { "name": "api", "image": "shop-api", "present": false, "size": null }
]
```

### ac <project> volumes

`state` is `present`, `absent`, or `unknown` when the daemon is down.

```json
[
  { "name": "postgres-data", "volume": "shop-postgres-data", "state": "present" }
]
```

### ac <project> port

Manifest-declared mappings, split for you. A port with no colon repeats itself
in both fields.

```json
[
  { "service": "postgres", "ports": [{ "host": "5433", "container": "5432", "raw": "5433:5432" }] }
]
```

### ac <project> ip

```json
[
  { "service": "redis", "container": "shop-redis", "ip": "192.168.64.5/24", "state": "running" }
]
```

Without `--json`, naming exactly one service prints just the bare address, which
is the form to use in a shell substitution.

### ac <project> env

The service's environment as a flat object, values as written in the manifest.

```json
{ "POSTGRES_USER": "user", "POSTGRES_PASSWORD": "pass", "PGDATA": "/var/lib/postgresql/data/pgdata" }
```

### ac <project> config

The manifest, parsed and re-emitted as JSON. Without `--json` you get the file's
bytes exactly as written (a trailing newline is added when the file lacks one),
which is the form to diff against.

### ac <project> build

One object per build in the run, emitted after everything has finished. `seconds`
is rounded to one decimal. `pushed` is whether the tags reached a registry.
`error` is `null` on success.

```json
[
  {
    "build": "api",
    "ok": true,
    "seconds": 42.7,
    "steps": { "done": 14, "cached": 11 },
    "tags": ["shop-api:dev-local"],
    "pushed": false,
    "error": null
  }
]
```

`--json` also forces plain (non-TTY) build progress, so the live one-line
renderer never contaminates the stream.

### ac <project> build --dry-run

The resolved plan, nothing executed. `command` is the argv that would be handed
to `container`, so you can print it and run it by hand.

```json
[
  {
    "build": "api",
    "profile": "prod",
    "root": "/Users/me/code/shop",
    "dockerfile": "apps/api/Dockerfile",
    "platform": "linux/amd64",
    "tags": ["123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-api:latest"],
    "push": true,
    "command": ["build", "--platform", "linux/amd64", "..."]
  }
]
```

### ac <project> push

Tag resolution plus what actually went up. A tag that failed to push appears in
`tags` but not in `pushed`, and the command exits non-zero.

```json
[
  {
    "build": "api",
    "profile": "prod",
    "tags": ["123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-api:latest"],
    "pushed": ["123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-api:latest"]
  }
]
```

### ac <project> rollout --dry-run

The resolved hooks and the environment they would be handed. `preflight` and
`run` are arrays of argv arrays with `argv[0]` made absolute against the build
root.

```json
{
  "profile": "prod",
  "root": "/Users/me/code/shop",
  "builds": ["api"],
  "preflight": [["/Users/me/code/shop/extras/ac-scripts/preflight.sh", "app"]],
  "run": [["/Users/me/code/shop/extras/ac-scripts/rollout.sh", "app"]],
  "env": { "AC_IMAGE_API": "...", "AC_IMAGES": "...", "AC_PROFILE": "prod" }
}
```

### Commands with no JSON form

`logs`, `exec`, `sh`, `cp`, `export`, `save`, `load`, `login`, `logout`,
`system logs`, `machine` and the manifest-free `ac top` produce whatever the
underlying tool produces. (`ac <project> top` is the exception: it does have a
JSON form, `[{service, container, processes}]`, where `processes` is the raw
`ps` output split into lines.)
`--json` on those is accepted (it is global) but only suppresses ac's own
chatter; the payload is not reshaped. For `logs` in particular, parse the lines,
not a document.

## Streams

| Stream | What lands there |
| --- | --- |
| stdout | The requested payload. Under `--json`, exactly one JSON document. Without `--json`, the human table or the passthrough output. |
| stderr | The `$ container ...` echo, `warn` and `err` at all times, plus (under `--json`) the `==>` and `ok` lines that would otherwise be on stdout. |

That split is what makes `ac --json ... | jq` safe even while ac is narrating
what it does. Redirect stderr away if you want silence, or set `AC_QUIET=1`.

```console
$ ac --json shop ls 2>/dev/null | jq -r '.[] | select(.state != "running") | .service'
```

## Exit codes

- `0`: success.
- `1`: any ac-level failure (bad manifest, unknown project or service, daemon
  not running for a read, a build that failed, a stop that did not take, a
  `wait` that timed out). The message is on stderr, prefixed `err`.
- Anything else: the exit code of the process ac ran on your behalf. `ac exec`,
  `ac sh`, `ac run` (foreground), `ac <project> exec`, `ac <project> sh`,
  `ac <project> run` and manifest scripts propagate the child's code exactly, so
  `ac shop exec api false` exits 1 because `false` did, not because ac failed.

Failures are loud on purpose. `ac <project> stop` re-reads the container list
and fails if something is still running, rather than trusting the exit code of
`container stop`. `ac ps` and the other reads fail with a hint instead of
starting a daemon behind your back:

```console
$ ac ps
err container daemon is not running; start it with `ac system start` or any `ac <project> start`
```

Naming a service that does not exist is also a failure, and the error lists the
valid names, which is usually enough for an agent to correct itself without a
second round trip.

## Gating on readiness: ac <project> wait

`start` already waits on each service's `readyCmd`, but it warns and continues
on timeout rather than failing. When a script must not proceed until the stack
is genuinely up, use `wait`, which exits non-zero instead.

| Flag | Default | What it does |
| --- | --- | --- |
| `--timeout <SECS>` | each service's `readyTimeout` from the manifest | Seconds to wait per service before giving up. |
| `[services...]` | every service | Which services to wait for. Short (`redis`) or full (`shop-redis`) names both resolve. |

A service with a `readyCmd` is polled through `container exec` until it exits 0.
A service without one is waited on until its container state is `running`.
`wait` requires a running daemon and will not start one.

The timeout is a **wall clock**, not a probe count. Each probe is spawned under
its own kill deadline (capped between 2 and 20 seconds and never past the
overall deadline), so a wedged `container exec`, which is a real failure mode on
Apple container, cannot hang the loop past your timeout.

```console
$ ac shop start
$ ac shop wait --timeout 60
$ ac shop wait postgres --timeout 30 && psql -h 127.0.0.1 -p 5433 -U user postgres
```

Bring a stack up and block until ready, failing the script if it does not come
up:

```bash
#!/usr/bin/env bash
set -euo pipefail

export AC_QUIET=1

ac shop start
if ! ac --json shop wait --timeout 90 > /tmp/shop-ready.json; then
  echo "not ready:" >&2
  jq -r '.[] | select(.ready | not) | .service' /tmp/shop-ready.json >&2
  ac shop logs -n 50 >&2
  exit 1
fi

PGPORT=$(ac --json shop port postgres | jq -r '.[0].ports[0].host')
psql -h 127.0.0.1 -p "$PGPORT" -U user postgres -c 'select 1'
```

Parse `ac ps --json` with jq:

```bash
# every ac-managed container, project and state, tab separated
ac --json ps -a | jq -r '.[] | [.container, (.project // "-"), .state] | @tsv'

# bare IP of one service, prefix length stripped
ac --json ps | jq -r '.[] | select(.container == "shop-redis") | .ip | split("/")[0]'

# fail if anything ac manages is not running
if ac --json ps -a | jq -e 'map(select(.project != null and .state != "running")) | length > 0' >/dev/null; then
  echo "some services are down" >&2
  exit 1
fi
```

## Environment variables

| Variable | Default | What it does |
| --- | --- | --- |
| `AC_QUIET` | unset | Any value suppresses the command echo and human log lines. Same as `--quiet`. |
| `NO_COLOR` | unset | Any value disables ANSI, the same as `--no-color`. |
| `AC_PROFILE` | unset | Default profile for `build`, `push`, `rollout`, `login` and `<project> images` interpolation, when `-P/--profile` is not given. Falls back to `local`. |
| `AC_ROOT` | unset | Build root, second in precedence after `--root`. Must exist, or the command errors. |
| `AC_HOME` | the nearest ancestor of the resolved executable that holds a `projects/` directory, else `~/scripts/ac` | Where bundled `projects/` manifests are looked for. Used verbatim when set. |
| `AC_POLL_INTERVAL` | `5` | Seconds between supervisor polls. Read from the environment the supervisor was spawned with. |
| `AC_IDLE_GRACE` | `4` | Consecutive idle polls before an ac-owned daemon is stopped. |
| `AC_COMPLETE_OFFLINE` | unset | Any value stops shell completion shelling out to `container`, so TAB is instant and empty. |
| `XDG_CONFIG_HOME` | `~/.config` | Parent of `ac/config.json` and `ac/projects/`. |
| `XDG_STATE_HOME` | `~/.local/state` | Parent of `ac/daemon.owned`, `ac/supervisor.pid`, `ac/supervisor.log`. |
| `HOME` | required | ac errors immediately if it is not set. |

Because the supervisor reads its two variables at spawn time, a fast watchdog is
a per-invocation thing:

```console
$ AC_POLL_INTERVAL=1 AC_IDLE_GRACE=3 ac shop start
```

Manifest scripts additionally receive `AC_PROJECT`, `AC_PROJECT_FILE`, and
`AC_PROJECT_ROOT` when the manifest sets `root`. Rollout hooks receive the
resolved image references (`AC_IMAGE_<BUILD>`, `AC_IMAGES_<BUILD>`, `AC_IMAGES`,
`AC_BUILDS`) plus the profile values; see [Builds and rollouts](builds.md).

## Non-TTY behaviour

ac checks the terminals rather than guessing, so behaviour in CI is predictable.

- **Colour** is emitted only when stdout is a terminal, `NO_COLOR` is unset,
  `--no-color` was not passed and `--json` was not passed. All four must hold.
- **`-t` is passed to `container exec` only when stdin AND stdout are both
  terminals.** Apple container fails with ENODEV otherwise, which is the classic
  way execs break in scripts. `ac exec -it` in CI silently drops the `-t` and
  works; `ac run -t` with no terminal warns and drops it.
- **`ac run`** adds `-i` when you asked for it with `-i`, or when the run is not
  detached and both stdin and stdout are terminals. `-t` is added only for a
  non-detached run on a real terminal; asking for `-t` without one warns
  (`not allocating a TTY: stdin and stdout are not both terminals`) and drops
  it. A detached run never gets `-t`.
- **Build progress** renders the live one-line-per-build display only on a TTY.
  Off a TTY, or with `--json`, or with `--progress plain`, ac streams raw
  prefixed buildkit lines instead. The live renderer keeps the last 200 raw
  lines per build and replays the last 40 of them when a build fails; the
  streaming renderer has already printed every line.
- **Interactive project verbs** (`ac <project> exec`, `ac <project> sh`) accept
  docker's `-i`/`-t` and ignore them; interactivity is detected, not declared.

## The command echo

Before running anything, ac prints the exact command to stderr, dimmed and
prefixed with `$ `, with every argument shell-quoted:

```console
$ ac shop start redis
$ container ls -a --format json
$ container volume create shop-redis-data
$ container run -d --progress none --name shop-redis --label ac.project=shop ...
```

Those lines are copy-pasteable as written. When something goes wrong, the
fastest debugging move is to take the last echoed line and run it by hand
against `container` directly, which tells you immediately whether the problem is
ac's or the runtime's. For an agent, the echo is also the cheapest way to learn
what ac actually does with a manifest.

Repeated probes (the readiness poll, for instance) are echoed once rather than
on every iteration. `--quiet` and `AC_QUIET=1` suppress the echo entirely, and
`--json` implies it.

## Self-teaching: ac guide

```console
$ ac guide                          # the full manual, embedded in the binary
$ ac guide claude                   # a CLAUDE.md snippet for another repo
$ ac guide claude >> ../my-app/CLAUDE.md
```

`ac guide` is written for someone (or something) with no other context: the
docker-to-ac command table, the two command forms and when to use each, the
daemon ownership rules, build behaviour and manifest authoring. It ships inside
the binary, so it is always in step with the version installed.

`ac guide claude` prints a shorter block designed to be appended to another
repository's `CLAUDE.md`, so an agent working in that repo drives `ac` correctly
without being told.

Two more commands are worth knowing for the same reason: `ac schema` gives you
the manifest JSON Schema to author against, and every subcommand's `--help` is
written to be read cold, including what it runs underneath and worked examples.

```console
$ ac shop wait --help
$ ac run --help
```

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [Global flags and invocation-wide behaviour](global-flags.md): the flags and
  environment variables behind every shape on this page.
- [Project commands](project-commands.md) and
  [Container commands](containers.md): the commands whose JSON is described
  here.
- [Shell completion](completions.md): the other half of driving `ac` without
  reading its source.
