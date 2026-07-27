# CLAUDE.md

Operating manual for `ac`. An agent should be able to drive the tool from this
file alone, without reading the source.

## What ac is

`ac` is a project runner for Apple `container`, the macOS native container
runtime. Apple ships no `docker compose` equivalent, so there is no way to
declare "these four services make up my local stack" and bring them up
together. `ac` is that missing layer: a project is one JSON manifest, and
`ac <project> start` turns it into running containers.

It also manages the `container` daemon itself, under a strict ownership rule
described below, so the daemon is running exactly when it needs to be and is
never taken away from someone else.

Two implementations live in this repo, side by side:

- **bash** (`bin/ac`, `lib/*.sh`) the original, and the behavioural spec.
- **Rust** (`src/*.rs`, `Cargo.toml`) the rewrite. Build it with `make build`;
  the binary lands at `target/release/ac`.

They are intended to behave identically. Where they deliberately differ, see
[Deliberate differences from bash](#deliberate-differences-from-bash).

## The daemon ownership contract

This is the single most important rule in the tool. Violating it means stopping
a daemon somebody else was using, taking their containers down with it.

- **If the daemon is already running when `ac` needs it, `ac` never starts,
  restarts or stops it.** Not on `start`, not on `stop`, not on `down`, not
  from the supervisor. It is someone else's.
- **If the daemon is not running, `ac` starts it** with `--app-root` from
  `~/.config/ac/config.json`, records ownership, and becomes responsible for
  stopping it once the last ac-managed container is gone.
- **Ownership is a file, not memory**: `~/.local/state/ac/daemon.owned`. A
  second `ac` invocation in another terminal reads the same file and agrees
  about who may stop the daemon. Nothing is inferred from process state.
- When `ac` owns the daemon it spawns a detached **supervisor**
  (`~/.local/state/ac/supervisor.pid`) which polls, and stops the daemon when
  the last ac-managed container across **all** projects has gone. It exists
  because containers also disappear without `ac down`: they crash, exit on
  their own, or get stopped with plain `container stop`.
- Refcounting spans **all** projects. Two projects up, `ac projA down`, and the
  daemon stays up for projB.

### The supervisor debounce

The watchdog does not act on a single idle sample.

- `AC_POLL_INTERVAL` seconds between polls, default **5**.
- `AC_IDLE_GRACE` consecutive idle polls required before stopping the daemon,
  default **4**.
- The idle counter **resets to zero** the moment any ac container reappears, so
  a container restarting does not look like an empty stack.
- An `armed` flag means the watchdog **never acts before it has seen at least
  one container**. Without it, a supervisor spawned during `ac start` would race
  the containers it is waiting for and stop the daemon mid startup.

Both variables are read from the environment the supervisor was spawned with,
so `AC_POLL_INTERVAL=1 AC_IDLE_GRACE=3 ac myproj start` yields a fast watchdog.

### Reading the current state

```
ac status              # daemon, supervisor, and every project
ac daemon status       # just the daemon, and who owns it
ac --json status       # the same, machine readable
```

`running (owned by ac)` means `ac` started it and may stop it.
`running (external, untouched)` means it was already up and `ac` will not touch
it. `ac daemon stop` does nothing in the external case, by design.

## Make targets

Every target exports the Rust toolchain, which lives on an external SSD rather
than in `~/.cargo`. **Plain `cargo build` will not find a compiler**; go through
`make`, or export `CARGO_HOME` and `RUSTUP_HOME` yourself.

| Target | What it does |
| --- | --- |
| `make` / `make help` | Self documenting target list. The default. |
| `make build` | Optimised release binary at `target/release/ac`. |
| `make dev` | Fast unoptimised build, for iterating. |
| `make test` | Unit tests (`cargo test`). |
| `make lint` | `cargo clippy --all-targets -- -D warnings`. |
| `make fmt` | Format the source in place. |
| `make install` | Build, generate completions, symlink into `BIN_DIR`. |
| `make completions` | zsh, bash and fish completions into `completions/rust/`. |
| `make e2e` | Integration tests against real containers. |
| `make clean` | Remove build artefacts. |

`make install` accepts `BIN_DIR` (default `~/.local/bin`) and `BIN_NAME`
(default `ac`). To keep the bash `ac` on PATH at the same time:

```
make install BIN_NAME=ac-rs
```

Note for anyone editing the Makefile: macOS ships GNU Make 3.81, which execs a
recipe line directly when it contains no shell metacharacters, and that direct
exec searches the PATH make itself started with rather than the exported one.
That is why `CARGO` is an absolute path, quoted at every use site (the toolchain
path contains a space).

## Manifest schema

A project is a JSON file. Discovery, highest priority first:

1. `~/.config/ac/projects/<name>.json` (user)
2. `<repo>/projects/<name>.json` (bundled)

A user file **shadows** a bundled one of the same name, so the repo stays
cleanly updatable while remaining customisable.

Naming conventions, which the tool relies on:

- container name is `<project>-<service>`
- named volume is `<project>-<volume>`

Every field is typed and **unknown fields are rejected**, so a typo produces an
error naming the bad field rather than being silently ignored. Get the full
machine readable schema with:

```
ac schema > manifest.schema.json
```

### Worked example

```json
{
  "name": "shop",
  "description": "shop local backing services",
  "root": "/Users/me/code/shop",
  "region": "us-east-1",

  "builder": { "cpus": 8, "memory": "8g" },

  "profiles": {
    "local": { "platform": "linux/arm64", "push": false, "tag": "dev-local", "registry": "" },
    "prod":  {
      "platform": "linux/amd64",
      "push": true,
      "account": "123456789012",
      "tag": "latest",
      "registry": "{{account}}.dkr.ecr.{{region}}.amazonaws.com/"
    }
  },

  "registries": [
    {
      "server": "{{account}}.dkr.ecr.{{region}}.amazonaws.com",
      "username": "AWS",
      "passwordCmd": ["aws", "ecr", "get-login-password", "--region", "{{region}}"]
    }
  ],

  "builds": [
    {
      "name": "api",
      "dockerfile": "apps/api/Dockerfile",
      "context": ".",
      "target": "runner",
      "image": "{{registry}}shop-api",
      "tags": ["{{tag}}", "{{version}}-{{git.shortSha}}{{git.dirtySuffix}}"],
      "buildArgs": { "BUILDKIT_INLINE_CACHE": "1" },
      "secrets": [{ "id": "NPM_TOKEN", "env": "NPM_TOKEN" }],
      "labels": { "org.opencontainers.image.revision": "{{git.sha}}" },
      "preflight": [["sh", "-c", "test -n \"$AWS_PROFILE\""]],
      "postPush": [["kubectl", "rollout", "restart", "deploy/shop-api"]]
    }
  ],

  "services": [
    {
      "name": "postgres",
      "image": "docker.io/library/postgres:16-alpine",
      "cpus": 2,
      "memory": "1g",
      "ports": ["5433:5432"],
      "env": {
        "POSTGRES_USER": "user",
        "POSTGRES_PASSWORD": "pass",
        "PGDATA": "/var/lib/postgresql/data/pgdata"
      },
      "volumes": [{ "name": "postgres-data", "target": "/var/lib/postgresql/data" }],
      "readyCmd": ["pg_isready", "-U", "user"],
      "readyTimeout": 90
    },
    {
      "name": "redis",
      "image": "docker.io/library/redis:7-alpine",
      "args": ["redis-server", "--appendonly", "yes"],
      "ports": ["6379:6379"],
      "volumes": [{ "name": "redis-data", "target": "/data" }],
      "readyCmd": ["sh", "-c", "redis-cli ping | grep PONG"]
    }
  ]
}
```

### Field notes

- `readyCmd` is polled through `container exec` until it exits 0. Apple
  `container` has no healthcheck primitive, so readiness is implemented here. On
  timeout `ac` warns and continues rather than failing.
- `cpus` and `memory` on a service size that container's **VM**, not a cgroup.
  Every container is its own virtual machine.
- `args` are appended after the image reference.
- `volumes[].name` is the logical name; the real volume is `<project>-<name>`.
- `preflight` and `postPush` are argv **arrays of arrays**, run from the
  resolved build root. **A hook failure aborts immediately** and is reported as
  an error; `postPush` only runs after a successful push.
- `passwordCmd` is argv that is executed and piped to
  `container registry login --password-stdin`. Credentials are never stored in
  the manifest, which suits tokens that expire (ECR tokens last 12 hours, so
  this re-runs on every start).

### Interpolation

`{{...}}` placeholders are expanded in `image`, `tags`, `buildArgs`, `labels`,
hook arguments, and registry `server` and `passwordCmd`:

| Placeholder | Value |
| --- | --- |
| `{{profile}}` | the profile name being built |
| `{{account}}` | `profiles.<p>.account` |
| `{{tag}}` | `profiles.<p>.tag` |
| `{{region}}` | `profiles.<p>.region`, then `.region`, then `us-east-1` |
| `{{registry}}` | `profiles.<p>.registry`, itself interpolated |
| `{{version}}` | `version` from `package.json` at the build root, else `0.0.0` |
| `{{git.sha}}` | full HEAD sha |
| `{{git.shortSha}}` | short HEAD sha |
| `{{git.branch}}` | current branch |
| `{{git.dirtySuffix}}` | `-local-<timestamp>` when the tree is dirty, else empty |
| `{{timestamp}}` | `YYYYMMDDHHMMSS`, fixed once per build run |

`{{git.dirtySuffix}}` exists so a dirty local tree can never overwrite the image
CI built for that commit. `{{registry}}` is a host plus trailing slash, and
empty for purely local profiles, so one template yields `app:tag` locally and
`<acct>.dkr.ecr.<region>.amazonaws.com/app:tag` when pushing.

## Commands

Usual form is `ac <project> <action> [services...]`. `ac <project>` alone means
`ac <project> status`. Services resolve from **either** spelling: `redis` or
`shop-redis`. Naming a service that does not exist fails loudly and lists the
valid ones.

When a project name collides with one of ac's own commands, use the escape
hatch `ac -p <project> <action>`.

### Project actions

| Command | What it runs underneath |
| --- | --- |
| `start [svc...]` | Ensures the daemon, logs in to registries the images come from, creates missing volumes, then `container start` for an existing stopped container or `container run -d` otherwise. Waits on `readyCmd`. |
| `start --recreate` | `container rm` then `container run -d`. Volumes and their data survive. |
| `stop [svc...]` | `container stop`. Containers are **kept**, so `start` brings them back in place. Then the cross-project daemon refcount check. |
| `down [svc...]` | `container stop` then `container rm`. Named volumes and data survive. Then the refcount check. |
| `restart [svc...]` | `stop` then `start`, without releasing the daemon in between. |
| `ls`, `ps`, `status` | One `container ls -a --format json`, joined against the manifest. Never created shows as `absent`. |
| `logs [-f] [-n N] [--boot] [svc]` | `container logs`. With no service it fans out across every service, prefixed and coloured per service; Ctrl-C tears down the group. |
| `exec <svc> <cmd...>` | `container exec -i [-t] <project>-<svc> <cmd...>`. |
| `sh`, `shell [svc]` | `container exec` running bash when present, else sh. Defaults to the first service. |
| `stats [svc...]` | `container stats <containers...>` |
| `inspect [svc...]` | `container inspect <containers...>` |
| `kill [-s SIG] [svc...]` | `container kill --signal <SIG> <containers...>`, default KILL. |
| `rm [svc...]` | `container rm --force`, then the refcount check. Volumes survive. |
| `cp <src> <dst>` | `container cp`, rewriting `svc:/path` to `<project>-<svc>:/path` on either side. |
| `pull [svc...]` | `container image pull` per service, after any needed login. |
| `images` | Images the services use, from the manifest. |
| `port [svc...]` | Published port mappings, from the manifest. |
| `ip [svc...]` | Container IPs from the daemon. A single named service prints just the address. |
| `env <svc>` | Environment variables from the manifest. |
| `build [name...]` | See [Builds](#builds). |
| `login [-P profile]` | Runs each registry's `passwordCmd` into `container registry login --password-stdin`. |
| `config` | The project manifest as written. |

### Global commands

| Command | What it does |
| --- | --- |
| `ac ls`, `ac projects` | List discoverable projects. |
| `ac status` | Daemon, supervisor, and every project. |
| `ac daemon status` | Daemon state and who owns it. |
| `ac daemon stop` | Stop the daemon, **only** if ac started it. |
| `ac images` | `container image ls` |
| `ac df` | `container system df` |
| `ac prune` | `container prune` then `container image prune`, then the refcount check. |
| `ac config` | Resolved `~/.config/ac/config.json`. |
| `ac schema` | The manifest JSON Schema. |
| `ac completions <shell>` | zsh, bash, fish, elvish or powershell. |
| `ac version`, `ac help` | |

### Agent facing behaviour

- `--json` on every read command, with stable field names. It **implies
  `--quiet`**, and human log lines move to stderr, so stdout stays a single
  parseable document.
- Every underlying `container` command is **echoed to stderr**, dimmed and
  prefixed with `$ `, before it runs, so any step can be copied and re-run by
  hand. Suppress with `AC_QUIET=1` or `--quiet`.
- Colour and progress turn off automatically when stdout is not a TTY, when
  `NO_COLOR` is set, or with `--no-color`.
- Every subcommand has a `--help` written for someone with no other context:
  what it does, what it runs underneath, and examples.

## Builds

```
ac shop build                                   # every build, parallel
ac shop build api --profile prod                # one build, prod profile
ac shop build --platform linux/amd64 --no-cache --sequential
```

Precedence for every setting, highest first:

1. CLI flag (`--platform`, `--target`, ...)
2. profile (`.profiles.<name>`)
3. build entry (`.builds[]`)
4. project default (`.builder`, `.region`)

Flags: `-P/--profile`, `--root`, `--platform`, `--push` / `--no-push`,
`--no-cache`, `--progress <auto|plain|tty>`, `--target`, `--builder-cpus`,
`--builder-memory`, `--sequential`.

Multiple builds run in parallel by default, one `indicatif` spinner each, every
child line prefixed with the build name. `--sequential` instead inherits stdio
so buildkit renders its own progress.

Registry login is **filtered**: a registry is contacted only when an image
actually comes from it. That is what stops `ac shop start` logging in to ECR
merely to pull postgres from docker.io. An explicit `ac shop login` uses every
declared registry.

### Build root resolution

Highest priority first. The resolved root is always printed.

1. `--root <path>`
2. `$AC_ROOT`
3. the git worktree containing `$PWD`, when that tree contains the manifest's
   first declared dockerfile
4. when not in a git repo at all, `$PWD`, when it contains every dockerfile the
   manifest declares
5. `.root` from the manifest
6. `$PWD`

Rule 3 is what makes git worktrees work: running a build from inside a worktree
builds **that** tree, not the path baked into the manifest, without needing a
second manifest per worktree. Requiring the dockerfile to be present is what
stops an unrelated repo hijacking the build.

## Apple Container gotchas

Hard won, do not rediscover:

- **Builder sizing only applies at creation.** `container builder` reads cpu and
  memory only when the builder container is CREATED. Passing `-c`/`-m` to a
  build while it is running is silently ignored, so `ac` stops the builder first
  when a resize is needed. That discards its layer cache, so `ac` says so
  loudly.
- **`container run` sometimes exits non-zero having actually started the
  container.** Trust observed state over the exit code: sleep about 2s and
  re-check before declaring failure.
- **`container exec -t` fails with ENODEV when there is no TTY.** Only pass `-t`
  when stdin **and** stdout are terminals. This is what breaks execs in scripts
  and CI.
- **Named volumes are real ext4 devices**, so a fresh one already contains
  `lost+found`. Postgres refuses to initialise into a non-empty directory, hence
  `PGDATA` pointing at a subdirectory in the example above.
- **`container system start --app-root` is not sticky.** Pass it on every start.
  It reaches the daemon through `CONTAINER_APP_ROOT` in the launchd job.
- **A volume mounted `noowners` makes container-apiserver abort** with
  `XPC connection error: Connection invalid`. Config may declare `sparseBundle`
  and `imageMount`, and `ac` then attaches with `hdiutil attach -owners on`
  before starting the daemon.
- **`container ls -a --format json`** returns
  `[{id, status:{state, networks:[{ipv4Address}]}}]`. IPs come back with a
  prefix length, for example `192.168.64.4/24`.
- **`container build` has no `--cache-from`.** It supports `--platform`,
  `--target`, `--build-arg`, `--secret`, `--label`, `--no-cache`, `--pull`,
  `--progress`, `-c`, `-m`, `-f`, `-t`.
- Containers get a routable `192.168.64.x` address, so services are reachable
  without publishing ports. ICMP is blocked, so `ping` fails even when TCP works.
- `ac` must run on the **host**. A Linux container cannot produce a macOS
  Mach-O binary, and `ac` needs to reach `container-apiserver` over XPC.

## ac's own config

`~/.config/ac/config.json`, seeded on first run. If the daemon happens to be
running at that moment its current `appRoot` is adopted, so `ac` keeps using the
image store you already have instead of silently starting a second one.

```json
{
  "appRoot": "/Volumes/ContainerData/app-root/",
  "sparseBundle": "/Volumes/SomeDisk/container-data.sparsebundle",
  "imageMount": "/Volumes/ContainerData",
  "startTimeout": 90
}
```

State lives in `~/.local/state/ac/`: `daemon.owned`, `supervisor.pid`,
`supervisor.log`.

## Adding a project

1. Write `~/.config/ac/projects/<name>.json`. Start from the worked example
   above, or from `ac schema`.
2. Check it parses and the services look right:
   ```
   ac ls
   ac <name> config
   ac <name> images
   ```
   An unknown field is an error naming the field, so typos surface here.
3. Bring it up and watch what it runs:
   ```
   ac <name> start
   ac <name> ls
   ```
4. No code changes are needed. Manifest discovery is by directory listing.

Put a manifest in `<repo>/projects/` instead when it should ship with the tool;
a user file of the same name still wins.

## Conventions

**No comments in code.** Not in the Rust, not in the Makefile, not in
`tests/e2e.sh`. Names and structure carry the meaning; anything that genuinely
needs explaining belongs in this file instead. That is why the gotchas, the
build root rules, the interpolation table and the ownership contract are all
documented here at length rather than inline.

Two exceptions, both because they are functional rather than explanatory:

- `///` doc comments in `src/cli.rs`. clap turns these into the `--help` text,
  so deleting one deletes user facing output.
- `##` annotations on Makefile target lines. The `help` target parses them with
  awk to build its own listing.

Check for regressions:

```
grep -nE '^\s*//' src/*.rs | grep -v '^src/cli.rs'    # expect no output
grep -nE '^\s*#' Makefile tests/e2e.sh | grep -v '#!' # expect no output
```

The bash implementation in `bin/` and `lib/` predates this rule and is left as
it is; it is the reference spec, not active development.

## Testing

```
make test    # unit tests: argv rewriting, interpolation, cp rewriting, schema
make e2e     # integration tests against real containers
```

`tests/e2e.sh` is not a unit test. It drives the release binary against a live
daemon, because the things most worth protecting (ownership, restart in place,
volume survival) cannot be faked. It:

- writes two throwaway projects into `~/.config/ac/projects/` and removes them,
  with their containers, volume and image, afterwards;
- **stops the daemon**, which is the only way to exercise the ownership
  scenarios, and therefore stops whatever containers were running and restarts
  them afterwards;
- restores the environment exactly as found, from an `EXIT` trap, so an aborted
  run still puts things back, including clearing any ownership file `ac` created
  so a daemon that was external stays external.

`KEEP=1 ./tests/e2e.sh` leaves the test project in place for poking at.

## Deliberate differences from bash

- **`restart` does not release the daemon between the stop and the start.** The
  bash version ran `project_stop` (which calls `supervisor_settle`) and then
  `project_start`, so restarting the only running project could stop an ac-owned
  daemon and immediately start it again. Pure churn, and a window where the
  daemon is down.
- **Colour is genuinely conditional.** One module decides whether any ANSI is
  emitted, so `--no-color`, `NO_COLOR`, `--json` and "stdout is not a terminal"
  all really suppress it.
- **The manifest is typed and rejects unknown fields.** bash read it with `jq`,
  so a misspelled key was silently ignored.
- **The supervisor debounce is implemented.** bash left it as a `TODO(human)`:
  it counted idle polls but never acted on them, so an ac-owned daemon was only
  ever reaped by the synchronous check inside `stop`/`down`, never by the
  watchdog.
- **One `container ls -a` per logical operation** instead of one per service, so
  the echoed output stays readable and status is a single consistent snapshot.
- **`--json` moves human log lines to stderr** so stdout is one parseable
  document.
