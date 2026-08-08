# ac CLI reference

`ac` is the command line for Apple `container`, the macOS native container
runtime. It replaces both halves of the docker CLI: the plain container verbs
(`run`, `build`, `logs`, `exec`) and the compose-style stack orchestration that
Apple `container` does not ship at all.

Everything `ac` does ends in a call to `container`. Every one of those calls is
echoed to stderr, dimmed and prefixed with `$ `, before it runs, so any step can
be copied and re-run by hand. Suppress the echo with `--quiet` or `AC_QUIET=1`.

```console
$ ac shop start
$ container system status
$ container volume create shop-postgres-data
$ container run -d --name shop-postgres --label ac.project=shop ...
```

## The two invocation forms

There are two forms and they are not aliases of each other.

```text
ac <project> <action> [services...]     acts on SERVICES resolved through a manifest
ac <verb> <container|image>             acts on ONE container or image by its real name
```

The project form is the compose surface. It needs a manifest (a JSON file under
`~/.config/ac/projects/` or `<repo>/projects/`), and only it does ordered
startup gated on `readyCmd`, named volume creation, registry login filtered to
the images actually involved, and short service name resolution.

The global form is the docker surface. It needs no manifest and knows nothing
about readiness or ordering. It is a thin, faithful pass to `container` with
ac's daemon ownership contract, command echo and `--json` handling layered on.

```console
$ ac shop start                 # compose: the whole stack, in order, ready-gated
$ ac shop restart redis         # compose: one service, by short name
$ ac build -t app:dev .         # docker: a Dockerfile with no project around it
$ ac run -d -p 3000:3000 app:dev
$ ac logs -f shop-redis         # docker: one container, by its real name
```

### Choosing between them

- **The thing is in a manifest: use the project form.** `ac shop restart`
  restarts a stack in dependency order and waits for each service to report
  ready. `ac restart shop-redis` restarts one container and knows nothing about
  readiness.
- **The thing is not in a manifest: use the global form.** A one-off `ac run`, a
  container someone else created, an image operation, a Dockerfile sitting on
  its own.
- **Do not write a manifest to run one container.** `ac run` is what that is
  for. A manifest earns its keep when several services must come up in order.
- **Both forms reach the same container, with different amounts of work.**
  `ac stop shop-redis` and `ac shop stop redis` both stop it and both use the
  same stop escalation ladder, but only the project form resolves the short name
  and reports against the manifest.
- **When in doubt, `ac ps --json`.** It lists everything on the daemon and
  attributes what it can to a project. `ac ls` and `ac <project> ...` only ever
  see what a manifest declares, so a container from `ac run` shows up in `ac ps`
  with an empty PROJECT column and nowhere else.

### `ac <project>` on its own

`ac <project>` with no action is rewritten to `ac <project> status`, which is an
alias of `ac <project> ls`: one `container ls -a --format json` joined against
the manifest, with services that were never created shown as `absent`.

```console
$ ac shop
$ ac shop status --json
```

The same rule applies to the explicit form: `ac -p shop` means
`ac project shop status`.

## Reserved words and the `-p` escape hatch

`ac <project> <action>` is a shorthand. Before clap sees the arguments, `ac`
rewrites `ac shop start` into `ac project shop start` (`rewrite_argv` in
`src/main.rs`). The rewrite only fires when the first non-flag word is **not**
one of ac's own commands and **is** the name of a discoverable manifest. A word
that is neither produces an error listing both the known projects and the known
commands.

When a project really is named after one of ac's commands, name it explicitly:

```console
$ ac -p status start          # project called "status"
$ ac --project status start
$ ac --project=status start
$ ac project status start     # the long form the shorthand expands to
```

`-p` / `--project` must come first, after the global flags and before the
action, because it is consumed by `rewrite_argv` rather than by clap. That also
means `--project=<name>` is a valid spelling while `-p=<name>` is not, and that
`ac -p` with no name is the error `-p requires a project name`. `ac -p build`
with no action means `ac project build status`.

The reserved words, in full, from `src/cli/reserved.rs`. A project named after
any of them is reachable only via `-p`:

```text
ls          projects    status      daemon      ps          image
images      volume      volumes     network     networks    system
registry    df          prune       config      schema      guide
completions version     project     rmi         help        run
build       create      start       stop        restart     rm
delete      exec        sh          shell       logs        inspect
kill        cp          copy        export      stats       top
port        pull        push        tag         save        load
login       logout      builder     machine     machines    __supervise
```

`__supervise` is the hidden detached supervisor loop and is never typed by hand.

A word starting with `-` is also never treated as a project name, so
`ac --help` and `ac --json status` behave as written.

## How a container name resolves

The global verbs take a container by its real name, which is the name `ac ps`
prints. Three spellings reach the same container:

| You type | Resolves to |
| --- | --- |
| `shop-redis` | `shop-redis`, used as is |
| `shop/redis` | `shop-redis`, the slash is rewritten to a dash |
| `angry_torvalds` | itself, whatever Apple `container` named it |

The rewrite is purely textual: the first `/` becomes `-` when both sides are
non-empty. No manifest is consulted, so `shop/redis` works even without a
manifest, as long as a container by that name exists.

The resolved name is then checked against a live `container ls -a`. Three
failure modes, all loud:

```console
$ ac logs shop
err 'shop' is a project, not a container
  try: ac shop status, or name a service directly (ac logs shop-<service>)

$ ac logs shop-redsi
err no such container: shop-redsi
  containers: shop-postgres shop-redis

$ ac stop
err stop needs a container name, or --all
  for a whole project stack: ac <project> stop
```

The last one is deliberate: a bare `ac stop` is an error rather than a silent
no-op, so nobody reaches for `-a` to make it do something. `--all` on `stop`,
`rm` and `kill` acts on every container the daemon has, including other
projects' and other people's.

In the project form the rules are different: services resolve from either
spelling, `redis` or `shop-redis`, and naming a service that does not exist
fails and lists the valid ones.

## docker to ac

Drop the word `docker`. Nearly every plain docker command works as written.

### Container lifecycle

| docker | ac |
| --- | --- |
| `docker run -d -p 3000:3000 img` | `ac run -d -p 3000:3000 img` (prints the URL) |
| `docker run --rm -it img sh` | `ac run --rm -it img sh` |
| `docker create img` | `ac create img` |
| `docker start c` | `ac start c` |
| `docker stop [-t N] c` | `ac stop [-t N] c` |
| `docker restart [-t N] c` | `ac restart [-t N] c` |
| `docker rm [-f] c` | `ac rm [-f] c` |
| `docker kill -s TERM c` | `ac kill -s TERM c` |
| `docker exec -it c sh` | `ac exec -it c sh`, or `ac sh c` |
| `docker logs -f --tail 100 c` | `ac logs -f -n 100 c` |
| `docker inspect c` | `ac inspect c` |
| `docker cp c:/path ./local` | `ac cp c:/path ./local` |
| `docker export c -o f.tar` | `ac export c -o f.tar` (container must be stopped) |
| `docker stats [--no-stream]` | `ac stats [--no-stream]` |
| `docker top c` | `ac top c` |
| `docker port c` | `ac port c` |
| `docker ps [-a] [-q]` | `ac ps [-a] [-q]` (adds PROJECT and SERVICE columns) |
| `docker attach c` | no equivalent |
| `docker pause` / `unpause` | no equivalent |
| `docker rename` | no equivalent |
| `docker commit` / `diff` | no equivalent |
| `docker wait c` | no equivalent (`ac <project> wait` is readiness polling, not the same) |
| `docker events` | no equivalent |
| `docker update` | no equivalent |
| `docker run --restart=always` | no equivalent, Apple `container` has no restart policies |
| `docker run --health-cmd ...` | no equivalent, use `readyCmd` in a manifest |

### Images and registries

| docker | ac |
| --- | --- |
| `docker build -t app:dev .` | `ac build -t app:dev .` |
| `docker build --cache-from ...` | no equivalent, `container build` has no `--cache-from` |
| `docker buildx ...` | `ac builder status\|start\|stop\|delete`, one shared builder |
| `docker images` | `ac image ls` (sizes shown by default), or `ac images` |
| `docker images -q` | `ac image ls -q` |
| `docker rmi ref` | `ac rmi ref`, or `ac image rm ref` |
| `docker pull ref` | `ac pull ref`, or `ac image pull ref` |
| `docker push ref` | `ac push ref`, or `ac image push ref` |
| `docker tag src dst` | `ac tag src dst`, or `ac image tag src dst` |
| `docker image inspect ref` | `ac image inspect ref` |
| `docker save -o f.tar ref` | `ac save -o f.tar ref`, or `ac image save -o f.tar ref` |
| `docker load -i f.tar` | `ac load -i f.tar`, or `ac image load -i f.tar` |
| `docker image prune [-a]` | `ac image prune [-a]` |
| `docker login -u user server` | `ac login -u user server`, or `ac registry login -u user server` |
| `docker logout server` | `ac logout server`, or `ac registry logout server` |
| (no equivalent) | `ac registry ls`, the stored logins |
| `docker history` | no equivalent |
| `docker manifest ...` | no equivalent |

### Daemon, volumes, networks

| docker | ac |
| --- | --- |
| `docker volume ls/create/rm/inspect/prune` | `ac volume ls/create/rm/inspect/prune` |
| `docker network ls/create/rm/inspect/prune` | `ac network ls/create/rm/inspect/prune` |
| `docker system df` | `ac system df`, or `ac df` |
| `docker system prune [-a]` | `ac system prune [-a]`, or `ac prune` |
| `docker system info` | `ac system info`, or `ac daemon status` |
| `docker version` | `ac version` |
| `docker help <cmd>` | `ac help <cmd>`, or `ac <cmd> --help` |
| (no docker equivalent) | `ac system start` / `ac system stop`, daemon lifecycle |
| (no docker equivalent) | `ac daemon status` / `ac daemon stop`, ownership aware |
| (no docker equivalent) | `ac machine ...`, passed through verbatim |
| `docker context ...` | no equivalent |
| `docker ps --filter ...` | no equivalent, the listings take no `--filter` |

### ac's own commands, with no docker counterpart

| Command | What it does |
| --- | --- |
| `ac ls` (alias `projects`) | List the manifests ac can see. |
| `ac status` | Daemon, supervisor and every project in one view. |
| `ac config` | The resolved `~/.config/ac/config.json`. |
| `ac schema` | The manifest JSON Schema, for authoring a project file. |
| `ac guide` / `ac guide claude` | The built-in manual, and a CLAUDE.md snippet for another repo. |
| `ac completions <shell>` | A static completion script. See [Shell completion](completions.md). |

### Compose

| docker compose | ac |
| --- | --- |
| `docker compose up -d` | `ac <project> start` (alias `up`; `-d` accepted and ignored) |
| `docker compose up --force-recreate` | `ac <project> start --recreate` |
| `docker compose down` | `ac <project> down` (containers removed, volumes survive) |
| `docker compose down -v` | `ac <project> down -v` (volumes and their data deleted) |
| `docker compose stop` / `start` | `ac <project> stop` / `start` (restart in place) |
| `docker compose restart` | `ac <project> restart` |
| `docker compose ps` | `ac <project> ls` (aliases `ps`, `status`) |
| `docker compose logs -f` | `ac <project> logs -f` (fans out across services) |
| `docker compose run --rm svc cmd` | `ac <project> run svc cmd` (`--rm` is the default, `--keep` retains) |
| `docker compose exec svc cmd` | `ac <project> exec svc cmd` |
| `docker compose create` | `ac <project> create` |
| `docker compose rm -f` | `ac <project> rm` |
| `docker compose pull` | `ac <project> pull` |
| `docker compose build` | `ac <project> build` (profiles, interpolation, rollout) |
| `docker compose push` | `ac <project> push -P <profile>` |
| `docker compose top` | `ac <project> top` |
| `docker compose kill` | `ac <project> kill` |
| `docker compose cp` | `ac <project> cp` |
| `docker compose port` | `ac <project> port` |
| `docker compose config` | `ac <project> config` |
| `docker compose images` | `ac <project> images` |
| `docker compose ls` | `ac ls` (aliases `projects`) |
| (no compose equivalent) | `ac <project> wait`, gate scripts on readiness |
| (no compose equivalent) | `ac <project> rollout -P <profile>` |
| `docker compose watch` | no equivalent |
| `docker compose events` | no equivalent |
| `docker compose --profile` | no equivalent (ac profiles are build profiles, not service filters) |

### Spellings that also work

`list` for `ls`, `delete` and `remove` for `rm`, `copy` for `cp`, `shell` for
`sh`, `up` for `<project> start`, `ps` and `status` for `<project> ls`,
`images`/`volumes`/`networks`/`machines` as plural aliases of their noun groups,
`remove` alongside `delete` for `image rm`, `volume rm` and `network rm`, and
`--format json` as a spelling of `--json` anywhere outside the `run`, `exec`,
`cp` and `machine` passthrough zone. Any other `--format` value is an error
that points at `--json`.

`-it` parses on `ac run` and `ac exec`, and `-t` is honoured only when stdin
**and** stdout are terminals, because Apple `container` fails with ENODEV
otherwise.

## Global flags

Three flags are global and apply to every command. They must come **before** the
verb when the verb forwards its trailing arguments (`run`, `create`, `exec`,
`machine`, and the project-scoped `run` and `exec`): write
`ac --json machine ls`, not `ac machine --json ls`.

| Flag | Default | What it does |
| --- | --- | --- |
| `--json` | off | Machine readable JSON on stdout. Implies `--quiet` and moves human log lines to stderr. |
| `--quiet` | off | Do not echo the underlying `container` commands. Same as `AC_QUIET=1`. No short form. |
| `--no-color` | off | Disable ANSI colour. Colour is off anyway when stdout is not a terminal or `NO_COLOR` is set. |

`-q` is deliberately not a short form of `--quiet`: it means "names only" on
`ac ps` and `ac image ls`, and `--build-quiet` on `ac build`. See
[Global flags and environment](global-flags.md) for the full list of flags,
environment variables and exit behaviour.

## Contents

Every other page in this reference, and what it is for.

| Page | What it covers |
| --- | --- |
| [Global flags and invocation-wide behaviour](global-flags.md) | `--json`, `--quiet`, `--no-color`, `-p`, the three meanings of `-q`, every `AC_*` environment variable, colour and TTY detection, exit codes and daemon gating. |
| [Project commands](project-commands.md) | Every `ac <project> <action>`, from `start` and `down` through `wait`, `logs`, `exec`, `env`, `ip` and manifest scripts, with readiness semantics and service name resolution. |
| [Container commands](containers.md) | The manifest-free docker verbs: `run`, `create`, `start`, `stop`, `restart`, `rm`, `exec`, `sh`, `logs`, `inspect`, `kill`, `cp`, `export`, `stats`, `top`, `port`. |
| [Images and registries](images-and-registries.md) | `ac build`, `ac pull`/`push`/`tag`/`save`/`load`/`rmi`, the `ac image` and `ac registry` groups, and `ac login`/`logout`. |
| [Builds](builds.md) | `ac <project> build` and `push`: profiles, setting precedence, build root resolution, `{{...}}` interpolation, parallelism, progress modes and the summary. |
| [Rollouts](rollouts.md) | Profile rollout hooks, when each list runs, the environment and interpolation handed to them, `--rollout` / `--no-rollout` and `--dry-run`. |
| [The project manifest](manifest.md) | Field-by-field schema reference: discovery and shadowing, validation, every top level field, `scripts`, and a worked manifest. |
| [Daemon, system and host-level commands](daemon-and-system.md) | The daemon ownership contract and supervisor, plus `ac status`, `ac ps`, `ac daemon`, `ac system`, `ac volume`, `ac network`, `ac builder`, `ac machine`, `ac config`, `ac schema`, `ac guide`. |
| [ac for scripts, CI and agents](agents-and-json.md) | The `--json` contract and the shape each command emits, which stream carries what, exit codes, gating on readiness, and non-TTY behaviour. |
| [Shell completion](completions.md) | The `COMPLETE=<shell>` hook versus the static script, what completes where, the bounded daemon-backed completers, `AC_COMPLETE_OFFLINE`, and troubleshooting. |

Outside this directory: the [project README](../../README.md) for install and a
quickstart, and [`docs/guide.md`](../guide.md) for the manual `ac guide` prints
from inside the binary.
