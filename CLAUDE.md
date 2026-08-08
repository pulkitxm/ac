# CLAUDE.md

Operating manual for `ac`. An agent should be able to drive the tool from this
file alone, without reading the source.

## What ac is

`ac` is the CLI for Apple `container`, the macOS native container runtime. It
covers two gaps, and the split between them is the shape of the whole tool.

The first gap is orchestration. Apple ships no `docker compose` equivalent, so
there is no way to declare "these four services make up my local stack" and
bring them up together. `ac` is that missing layer: a project is one JSON
manifest, and `ac <project> start` turns it into running containers.

The second gap is ergonomics. Apple's `container` CLI is complete but is not
docker: the verbs sit in different places, the flags differ in small ways, and
nothing tells you a URL at the end. `ac <verb>` is a docker-compatible surface
over the same runtime, needing no manifest at all, so `ac build -t app:dev .`
followed by `ac run -d -p 3000:3000 app:dev` works in any directory with a
Dockerfile. See [Manifest-free commands](#manifest-free-commands).

It also manages the `container` daemon itself, under a strict ownership rule
described below, so the daemon is running exactly when it needs to be and is
never taken away from someone else.

The tool is written in Rust. Build it with
`make build`; the binary lands at `target/release/ac`. It started life as a
bash script that has since been retired; where the rewrite deliberately
changed behaviour, see
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

### What counts as an ac-managed container

The refcount decides when an ac-owned daemon may be stopped, so what it counts
is load-bearing. It counts **labels**, not manifest membership.

Every container `ac` creates is labelled:

| Label | Applied by |
| --- | --- |
| `ac.project=<name>` | a manifest service, and one-off `ac <project> run` |
| `ac.managed=1` | `ac run` and `ac create`, which have no project |

`state::ac_running_containers` returns every running container carrying either
label, unioned with the `<project>-<service>` names the manifests declare. The
union is belt and braces: the names cover a container created by an older `ac`
before labelling existed, the labels cover everything with no manifest behind
it.

This has to be label-based, because the alternative is a bug. If the refcount
were derived from manifests alone, a container from `ac run` would count as
zero: `supervisor::settle` would stop the daemon in the same invocation that
started the container, and the watchdog would stop it about twenty seconds
later. The same reasoning fixes a pre-existing case, `ac <project> run --keep`,
whose container is named `<project>-<service>-run-<timestamp>` and so never
matched the manifest-derived name either.

Two things the label deliberately does not do. It does not make a container
appear in `ac <project> ls`, which is still a manifest join, and it does not
give `ac ps` a PROJECT to show, so a labelled container with no manifest prints
an empty PROJECT column. Discover those with `ac ps`.

A container started with plain `container run` carries no ac label, is never
counted, and is never ac's to stop. That is the ownership contract applied one
level down.

`daemon::ensure` records ownership but does **not** spawn the supervisor;
`supervisor::ensure` does, and is a no-op unless the daemon is ours. Every path
that starts the daemon and then leaves something running must call both, or the
daemon is owned and unwatched: `ac run`, `ac create`, `ac start`, `ac restart`,
`ac system start`, the mutating global passthrough, and `project::start`.

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

Every target exports the Rust toolchain locations, defaulting to `~/.cargo`
and `~/.rustup`. A toolchain that lives elsewhere (an external disk, say) is
pointed at from an untracked `Makefile.local` that sets `CARGO_HOME` and
`RUSTUP_HOME`; every target picks it up automatically.

| Target | What it does |
| --- | --- |
| `make` / `make help` | Self documenting target list. The default. |
| `make build` | Optimised release binary at `target/release/ac`. |
| `make dev` | Fast unoptimised build, for iterating. |
| `make test` | Unit tests (`cargo test`). |
| `make lint` | `cargo clippy --all-targets -- -D warnings`. |
| `make fmt` | Format the source in place. |
| `make install` | Build, then symlink into `BIN_DIR` and print the shell setup. |
| `make completions` | Print the shell hook to source. Completions are dynamic, so nothing is written to disk. |
| `make test-completions` | Drive the real binary and assert every command, flag and value completes. |
| `make e2e` | Integration tests against real containers. |
| `make clean` | Remove build artefacts. |

`make install` accepts `BIN_DIR` (default `~/.local/bin`) and `BIN_NAME`
(default `ac`), so `make install BIN_NAME=ac-dev` installs under another name.

Note for anyone editing the Makefile: macOS ships GNU Make 3.81, which execs a
recipe line directly when it contains no shell metacharacters, and that direct
exec searches the PATH make itself started with rather than the exported one.
That is why `CARGO` is an absolute path, quoted at every use site (the
toolchain path may contain a space).

## Publishing

The crate is **`ac-cli`**, the binary is **`ac`**. They differ because `ac` was
already taken on crates.io by an unrelated project whose only version is
yanked, and a yanked name is never released. `[[bin]] name = "ac"` is what
keeps `cargo install ac-cli` putting `ac` on the PATH, so it must not be
dropped in favour of the package name.

`exclude` keeps `.github/`, `tests/`, `projects/` and `scripts/` out of the
published tarball. `projects/` in particular would be actively misleading:
`ac_home()` finds bundled manifests by walking up from the executable, and
nothing above `~/.cargo/bin/ac` has a `projects/` directory, so a bundled
manifest could never be discovered by an installed binary. `scripts/` is wiki
tooling that only `wiki-sync.yml` runs, from the git checkout rather than the
crate. `docs/` must stay: `dispatch.rs` embeds `guide.md` and
`claude-snippet.md` with `include_str!`.

0.3.2 shipped `scripts/sync-wiki.mjs` before this was tightened, and was
yanked. A published version is immutable, so that tarball stays as it is; the
exclusion takes effect from 0.3.3 onward.

Nothing in the source is gated on `target_os`; `ac` shells out to `container`
and compiles cleanly on Linux. That is deliberate, so docs.rs and non-macOS CI
build it, at the cost of `cargo install` succeeding on a platform where the
binary cannot do anything. The description and README say so rather than a
`compile_error!`.

### The publish workflow

`.github/workflows/publish.yml` runs on every push to `main`, so **a release is
a version bump**: edit `version` in `Cargo.toml`, merge, and the workflow
publishes. Nothing else is a release trigger, and there are no tags to
remember.

The workflow is idempotent, which is what makes "publish on every merge" safe.
It reads the version from `cargo metadata` and asks crates.io whether that
exact version exists:

- **200** — already published, so the job prints that and stops. Every merge
  that does not touch the version is a green no-op.
- **404** — not published, so it runs `cargo test` and then `cargo publish`.
- **anything else** — the job fails rather than guess. A 5xx or a rate limit
  must never be read as "not published yet", because publishing is
  irreversible: a version can be yanked, which only hides it from new
  dependents, but it can never be reused or truly withdrawn.

It runs on `ubuntu-latest`, which is only possible because of the no-`cfg`
decision above; the rest of CI is on `macos-latest`.

The token is the repo secret `CARGO_REGISTRY_TOKEN`, passed through the
environment rather than `--token` so it stays out of the process list. Without
it the publish step fails with instructions rather than a cargo backtrace.

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
  ],

  "scripts": {
    "forward": "~/.config/ac/scripts/shop-tunnels.sh",
    "psql": "psql -h 127.0.0.1 -p 5433 -U user postgres"
  }
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
- `scripts` is a map of name to **one shell string**, npm run style.
  `ac <project> <name> [args...]` hands the string to `sh -c` with the extra
  arguments appended shell-quoted, and propagates the exit code. `ac` never
  interprets the string: the script owns its own subcommands, which is how
  project-specific tooling (ssh tunnels, port-forwards, db consoles) sits
  behind `ac` without `ac` learning about it. The script sees `AC_PROJECT`,
  `AC_PROJECT_FILE` and, when `root` is set, `AC_PROJECT_ROOT`. Names must be
  single words and must not collide with a project action; validation rejects
  the manifest otherwise, and the
  `every_project_action_is_listed_in_project_actions` test keeps that
  collision list in step with the CLI. Completion offers script names next to
  the built-in actions.
- A script entry may instead be `{"run": <string>, "complete": [<word>...]}`.
  `complete` is what TAB offers for the script's arguments, at every argument
  position. ac deliberately never executes a script to complete it (a
  completer that runs user code could hang the shell or dial out), so the
  words are static data in the manifest.

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

There are two forms and they are not aliases of each other.

`ac <project> <action> [services...]` acts on **services** resolved through a
manifest. `ac <project>` alone means `ac <project> status`. Services resolve
from **either** spelling: `redis` or `shop-redis`. Naming a service that does
not exist fails loudly and lists the valid ones.

`ac <action> <container|image>` acts on **one container or image** by its real
name, with no manifest involved. See
[Manifest-free commands](#manifest-free-commands).

When a project name collides with one of ac's own commands, use the escape
hatch `ac -p <project> <action>`.

### Choosing between the two forms

- **The thing is in a manifest: use the project form.** Only it does ordered
  startup gated on `readyCmd`, named volume creation, registry login filtered
  to the images actually involved, and service-name resolution. `ac shop
  restart` restarts a stack; `ac restart shop-redis` restarts one container and
  knows nothing about readiness.
- **The thing is not in a manifest: use the global form.** A one-off `ac run`,
  a container someone else created, an image operation, a Dockerfile with no
  project around it.
- **Do not write a manifest to run one container.** That is what `ac run` is
  for. A manifest earns its keep when several services must come up in order.
- **The two forms reach the same container but do different amounts of work.**
  `ac stop shop-redis` and `ac shop stop redis` both stop it and both use the
  escalation ladder, but only the project form resolves the short service name
  and reports against the manifest.
- **When in doubt, `ac ps --json`**: it lists everything on the daemon and
  attributes what it can to a project.

### Manifest-free commands

Global verbs, added so `ac` covers the plain docker CLI as well as compose.
Each is a thin, faithful pass to `container`, with ac's ownership contract,
command echo and `--json` handling layered on.

| Command | What it runs underneath |
| --- | --- |
| `run [opts] <image> [cmd...]` | `container run`, plus `--label ac.managed=1`. `-t` only when stdin and stdout are both terminals. Prints `http://localhost:<port>` for each published port after a detached run. |
| `create [opts] <image> [cmd...]` | `container create`, same flags minus `--progress`, which `container create` does not accept. |
| `build [-t ref] [-f file] [ctx]` | `container build`. No profiles, no interpolation, no rollout, no build-root resolution: for those use `ac <project> build`. |
| `start [-a] [-i] <c...>` | `container start` per container, since it takes a single id. |
| `stop [-t N] [-s SIG] [-a] <c...>` | the escalation ladder from `project::stop_container`: bounded `container stop`, then SIGKILL, then the runtime shim. `-s` bypasses it for a plain `container stop --signal`. |
| `restart [-t N] <c...>` | stop then start, without releasing the daemon in between. |
| `rm [-f] [-a] <c...>` | `container rm`. Volumes survive. Images are `ac rmi`. |
| `exec [-it] [-d] [-e] [-w] [-u] <c> <cmd...>` | `container exec -i [-t] ...` |
| `sh <c>` | `container exec` running bash when present, else sh. |
| `logs [-f] [-n N] [--boot] <c>` | `container logs` |
| `inspect <c...>` | `container inspect`, pretty-printed |
| `kill [-s SIG] [-a] <c...>` | `container kill --signal` |
| `cp <src> <dst>` | `container cp`, rewriting `<container>:/path` on either side |
| `export <c> [-o file]` | `container export`. Refuses on a running container and says to stop it. |
| `stats [--no-stream] [c...]` | `container stats`. `--json` implies `--no-stream` and is killed after 20s. |
| `top [c...]` | `ps aux` (fallback `ps`) through `container exec` |
| `port <c>` | published ports read from `container inspect` |
| `pull` / `push` / `tag` / `save` / `load` | the matching `container image` verb |
| `login` / `logout` | `container registry login` / `logout`. `container registry login` has **no** `--password`; ac accepts one and pipes it to `--password-stdin`. |
| `builder <status\|start\|stop\|delete>` | `container builder ...` |
| `machine [args...]` | `container machine ...`, passed through verbatim |

Daemon gating splits three ways, extending the read/mutate rule below:

- **Reads** (`logs`, `inspect`, `port`, `stats`, `top`, `export`, `cp`, `exec`,
  `sh`, `logout`, `save`) call `daemon::require` and fail with a hint rather
  than starting a daemon for a read.
- **Mutations that leave nothing behind** (`build`, `pull`, `push`, `tag`,
  `load`, `login`) ensure the daemon and run the refcount check afterwards, so a
  daemon started for a one-off is released again.
- **Mutations against an existing container** (`stop`, `rm`, `kill`) call
  `daemon::require`, not `ensure`, because with the daemon down there is nothing
  to act on, and then run the refcount check so an ac-owned daemon is released.
- **Mutations that leave a container running** (`run`, `create`, `start`,
  `restart`) additionally spawn the supervisor, because the daemon must stay up
  and must still be reaped once the container goes.

Sharp edges worth knowing:

- **`--all` is a whole-daemon blast radius.** `ac stop -a`, `ac rm -a` and
  `ac kill -a` act on every container the daemon has, including other projects'
  and other users'. Bare `ac stop` with no target is an error rather than a
  silent no-op, precisely so nobody reaches for `-a` to make it do something.
- **`-q` means three things.** Global `--quiet` has no short form; `-q` is
  "names only" on `ac ps` and `ac image ls`, and `--build-quiet` on `ac build`.
- **Trailing arguments swallow global flags.** `run`, `exec`, `cp` and
  `machine` forward everything after their target, so `--json` and `--quiet`
  must come first: `ac --json machine ls`, not `ac machine --json ls`.
- **`RESERVED` in `src/cli/reserved.rs` grew by 30 words**, including `run`, `build`,
  `start`, `stop`, `rm`, `logs`, `exec`, `top`, `port`, `push`, `tag`, `login`
  and `machine`. A project named after any of them is reachable only as
  `ac -p <name> ...`. The `every_top_level_command_is_reserved` test keeps
  `RESERVED` in step with the command enum; without it the two drift and a new
  subcommand silently parses as a project name.
- **Container names take `project/service` too**, so `ac logs shop/redis` and
  `ac logs shop-redis` are the same thing. Naming a project alone is an error
  that points at `ac <project> status`.

### Project actions

| Command | What it runs underneath |
| --- | --- |
| `start [svc...]` (alias `up`) | Ensures the daemon, logs in to registries the images come from, creates missing volumes, then `container start` for an existing stopped or created container or `container run -d` otherwise. Waits on `readyCmd`. Accepts and ignores `-d`. |
| `start --recreate` | `container rm` then `container run -d`. Volumes and their data survive. |
| `run [--keep] [-e K=V] [--no-volumes] <svc> [cmd...]` | One-off container from the service definition, named `<project>-<svc>-run-<timestamp>`, interactive when on a TTY, `--rm` unless `--keep`. Published ports are not bound, so it never conflicts with the running service. |
| `create [--recreate] [svc...]` | `container create` with exactly the argv `start` would use, so a later `start` starts it in place. |
| `top [svc...]` | `ps aux` (fallback `ps`) through `container exec` per running service. |
| `wait [--timeout N] [svc...]` | Polls `readyCmd` (or the running state when there is none) and exits non-zero on timeout, so scripts can gate on readiness. |
| `push [-P profile] [name...]` | Pushes the tags `build` would produce, without building. Logs in first, filtered to the registries involved. postPush hooks do not run. |
| `export <svc> [-o file]` | `container export`. Apple container refuses on a running container, so `ac` checks and says to stop it first. Default output `<project>-<svc>.tar`. |
| `stop [-t SECS] [svc...]` | `container stop [--time N]`. Containers are **kept**, so `start` brings them back in place. The result is verified against observed state, not the exit code. Then the cross-project daemon refcount check. |
| `down [-v] [-t SECS] [svc...]` | `container stop` then `container rm`. Named volumes and data survive unless `-v/--volumes` is given, which deletes them and their data. Then the refcount check. |
| `restart [svc...]` | `stop` then `start`, without releasing the daemon in between. |
| `ls`, `ps`, `status` | One `container ls -a --format json`, joined against the manifest. Never created shows as `absent`. |
| `logs [-f] [-n N] [--boot] [svc]` | `container logs`. With no service it fans out across every service, prefixed and coloured per service; Ctrl-C tears down the group. |
| `exec <svc> <cmd...>` | `container exec -i [-t] <project>-<svc> <cmd...>`. Docker's `-it` is accepted and ignored; interactivity is detected. |
| `sh`, `shell [svc]` | `container exec` running bash when present, else sh. Defaults to the first service. |
| `stats [--no-stream] [svc...]` | `container stats`. `--json` implies `--no-stream` and is killed after 20s if the runtime wedges. |
| `inspect [svc...]` | `container inspect <containers...>` |
| `kill [-s SIG] [svc...]` | `container kill --signal <SIG> <containers...>`, default KILL. |
| `rm [svc...]` | `container rm --force` on services that exist, absent ones are skipped like `down` does, failures exit non-zero. Then the refcount check. Volumes survive. |
| `cp <src> <dst>` | `container cp`, rewriting `svc:/path` to `<project>-<svc>:/path` on either side. |
| `pull [svc...]` | `container image pull` per service, after any needed login. |
| `images` | Images the services use, from the manifest. |
| `port [svc...]` | Published port mappings, from the manifest. |
| `ip [svc...]` | Container IPs from the daemon. A single named service prints just the address. |
| `env <svc>` | Environment variables from the manifest. |
| `build [name...]` | See [Builds](#builds). |
| `rollout [-P profile] [name...]` | Runs the profile's rollout hooks against images already pushed, without rebuilding. See [Rollouts](#rollouts). |
| `login [-P profile]` | Runs each registry's `passwordCmd` into `container registry login --password-stdin`. |
| `config` | The project manifest as written. |
| `scripts` | The `scripts` map from the manifest. |
| `<script> [args...]` | Any name from `scripts`: the string via `sh -c`, args appended shell-quoted. See [Field notes](#field-notes). |

### Global commands

The noun groups and ac's own commands. The docker-style verbs (`run`, `build`,
`start`, `logs`, ...) are global too and are tabulated under
[Manifest-free commands](#manifest-free-commands).

| Command | What it does |
| --- | --- |
| `ac ls`, `ac projects` | List discoverable projects. |
| `ac status` | Daemon, supervisor, and every project. |
| `ac ps [-a] [-q]` | One `container ls -a` joined against the manifests. The human table carries PROJECT and SERVICE columns; `-q` prints names only; `--json` emits `[{container, project, service, state, ip, image}]`. |
| `ac image <ls\|pull\|push\|rm\|tag\|inspect\|prune\|save\|load>` | The local image store, docker image style. `ls` shows sizes (`container image ls --verbose`) and takes `-q` for names only; `rm` also answers to `delete`/`remove`, `ls` to `list`; `ac images` and `ac rmi` aliases work. |
| `ac volume <ls\|create\|rm\|inspect\|prune>` | Volumes across the whole daemon. Per-project volumes stay under `ac <project> volumes`. |
| `ac network <ls\|create\|rm\|inspect\|prune>` | Networks. Non-default networks need macOS 26. |
| `ac system <info\|df\|start\|stop\|prune\|logs>` | Daemon lifecycle and disk usage. `start`/`stop` follow the ownership contract; `stop` refuses to touch an external daemon. `prune --all` also removes every unused image. |
| `ac registry <login\|logout\|ls>` | Registry logins outside any project. `ls` is a plain read with `--json`. |
| `ac daemon status` | Daemon state and who owns it. |
| `ac daemon stop` | Stop the daemon, **only** if ac started it. |
| `ac df`, `ac prune` | Route through the `system` group; same behaviour as before. |
| `ac config` | Resolved `~/.config/ac/config.json`. |
| `ac schema` | The manifest JSON Schema. |
| `ac guide [claude]` | The embedded manual (`docs/guide.md`). `claude` prints `docs/claude-snippet.md`, a drop-in block for another repo's CLAUDE.md. |
| `ac builder <status\|start\|stop\|delete>` | The shared image builder. Sizing applies only at creation, so a resize discards the layer cache. |
| `ac machine [args...]` | `container machine`, passed through verbatim. Note there is no `machine start`: booting happens via `create` or implicitly via `machine run`. |
| `ac completions <shell>` | A **static** script for zsh, bash, fish, elvish or powershell. It carries no dynamic values and no project subcommands; the `COMPLETE=<shell>` hook that `make completions` prints is the one to use. |
| `ac version`, `ac help` | |

Global reads (`ps`, `image ls`, `df`, ...) **require** a running daemon and
fail with a hint instead of starting one, because a daemon started for a read
would be silently owned with no supervisor. Mutating globals (`image pull`,
`registry login`, `prune`, ...) ensure the daemon and run the refcount check
afterwards, so a daemon started for a one-off command is released again. The
docker-style verbs follow the same rule with one addition, the ones that leave
a container running; see
[Manifest-free commands](#manifest-free-commands).

### Agent facing behaviour

- `--json` on every read command, with stable field names. It **implies
  `--quiet`**, and human log lines move to stderr, so stdout stays a single
  parseable document. On failure stdout may be empty; the exit code is the
  contract. `--format json` anywhere is rewritten to `--json` for docker
  muscle memory; other `--format` values error with that hint.
- The global quiet flag is `--quiet`/`AC_QUIET=1` only. The short `-q` belongs
  to the docker-style listings (`ac ps -q`, `ac image ls -q`), where it means
  names only.
- `wait` enforces its timeout as a wall clock: each readiness probe runs under
  its own kill deadline, so a wedged `container exec` cannot hang the loop.
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

Multiple builds run in parallel by default. On a TTY each build renders one
live line driven by `src/progress.rs`, which parses the buildkit plain stream
(`ac` forces `--progress plain` underneath): step position `[i/n]`, the
instruction, per-step elapsed and total elapsed, refreshed on a 100ms ticker.
Finished steps print as compact `+`/`-` lines, the last 200 raw lines are kept
per build and replayed on failure, and every build run ends with a summary
table, or a JSON array under `--json` (`{build, ok, seconds, steps, tags,
pushed, error}`). `--progress plain` streams raw prefixed lines instead (also
the non-TTY and `--json` behaviour), and `--progress tty --sequential`
inherits stdio so buildkit renders its own display.

`ac <project> push` reuses the same tag resolution to push without building.

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

## Rollouts

`ac` has no deployment logic and no Kubernetes awareness. A profile declares
hooks, `ac` runs them and hands them the image references it resolved, which is
what turns build, push and ship into one command without teaching `ac` about
anyone's cluster.

```json
"prod": {
  "push": true,
  "tag": "latest",
  "registry": "{{account}}.dkr.ecr.{{region}}.amazonaws.com/",
  "rollout": {
    "description": "restart the app deployments and pin the workers",
    "preflight": [["./extras/ac-scripts/preflight.sh", "app", "workers"]],
    "run":       [["./extras/ac-scripts/rollout.sh", "app", "workers"]],
    "auto": false
  }
}
```

```
ac shop build --rollout -P prod      build, push, then roll out
ac shop rollout -P prod              roll out what is already pushed
ac shop rollout -P prod --dry-run    resolved hooks and env, nothing run
ac shop build --no-rollout           never roll out, even when auto is true
```

- **`preflight` runs before anything is built.** Before the daemon is ensured,
  before the builder is sized, before any registry login. An unreachable
  cluster or an expired token therefore fails in seconds instead of after a
  ten minute build. This is the whole reason the hook list is split in two.
- **`run` fires only after every build and push in the invocation succeeded.**
  A non-zero exit from either list aborts and propagates.
- **The block hangs off the profile, not the project**, so blast radius is
  per profile: `prod` may restart everything while `pre-prod` touches only the
  pre-prod deployments. A profile with no `rollout` key can never deploy,
  which is what keeps `local` safe.
- `--rollout` against a profile that resolves to `push: false` is an error;
  nothing would have reached the registry for the rollout to pick up.
- `auto: true` rolls out on every build for that profile. `--no-rollout` still
  wins.

Hooks are argv, run from the resolved build root, with the usual `{{...}}`
interpolation plus `{{image.<build>}}`. They also receive the resolved
references in the environment, which is the interface the scripts actually use:

| Variable | Value |
| --- | --- |
| `AC_IMAGE_<BUILD>` | the build's primary tag, e.g. `AC_IMAGE_WEB` |
| `AC_IMAGES_<BUILD>` | every tag for that build, space separated |
| `AC_IMAGES` | every tag pushed in this run |
| `AC_BUILDS` | build names in this run |
| `AC_PROJECT`, `AC_PROFILE`, `AC_ACCOUNT`, `AC_REGISTRY`, `AC_TAG`, `AC_REGION`, `AC_ROOT` | resolved profile values |
| `AC_VERSION`, `AC_GIT_SHA`, `AC_GIT_SHORT_SHA`, `AC_GIT_BRANCH`, `AC_GIT_DIRTY`, `AC_TIMESTAMP` | source values |

A build name is upper-cased with every non-alphanumeric character replaced by
`_`, so a build called `api-workers` arrives as `AC_IMAGE_API_WORKERS`.

`AC_IMAGE_*` is populated for **every** build the manifest declares, not just
the ones being built, so a hook can pin a service that was not rebuilt in this
run. That is deliberate but sharp: a hook doing so should check `AC_BUILDS`
and confirm the tag actually exists in the registry, or it will pin a
deployment to an image nobody pushed.

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
- **`container cp` is unreliable in 1.1.0.** Copies INTO a container can
  silently no-op while exiting 0, and copies out of a container can hang
  forever; killing the hung `cp` can wedge the container so `stop` and `kill`
  stall too. Prefer `exec` with shell redirection, or `export`. `ac` passes
  `cp` through and cannot mask this.
- **A single container's exec channel can wedge under load**, taking every
  `exec`-based feature with it (readiness probes, `top`, `stats`), and a
  wedged container also ignores `container stop` and `container kill`. `ac`
  bounds its own probes with kill deadlines, and `stop`/`down`/`restart`
  escalate: bounded `container stop`, then SIGKILL, then terminating that
  container's own `container-runtime-linux` shim (matched by `--uuid`), so
  a wedged container still comes down. A bare `ac <p> exec` into a wedged
  container will still block until you Ctrl-C.
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

## Source layout

Grouped by responsibility, not by size. Nothing here is deep: one level of
directory, and every module is named after what it does.

| Path | What lives there |
| --- | --- |
| `main.rs` | Entry point and `rewrite_argv`, the shorthand that turns `ac shop start` into `ac project shop start`. Nothing else. |
| `dispatch.rs` | The match arms: `TopCommand` to a function, and `run_action` for the project verbs. |
| `core/ctx.rs` | `Ctx` (flags, paths, config) and `Runner`, the only place a subprocess is spawned. Every bounded-timeout variant lives here. |
| `core/state.rs` | `Snapshot` of `container ls -a`, and the daemon refcount. |
| `core/style.rs` | The one module that decides whether any ANSI is emitted. |
| `core/util.rs` | Shared formatting and `exit_ok`. |
| `cli/root.rs` | `Cli` and `TopCommand`. |
| `cli/project.rs` | `Action` and the project-scoped nested groups. |
| `cli/groups.rs` | The noun-group actions: image, volume, network, system, registry, daemon, builder. |
| `cli/run_opts.rs` | `RunOpts`, the flags `ac run` and `ac create` share. |
| `cli/reserved.rs` | `RESERVED` and `PROJECT_ACTIONS`. |
| `commands/docker/` | The manifest-free verbs: `target` resolves a name to a container, `opts` builds run argv, `lifecycle` is the container verbs, `images` the image and registry ones. |
| `commands/groups.rs` | The noun groups. |
| `commands/project.rs` | Project lifecycle: start, stop, down, readiness, login. |
| `commands/script.rs` | Manifest `scripts`: compose the string plus quoted args, run via `sh -c`. |
| `build/` | `vars` (interpolation, build root), `builder` (sizing), `plan` (argv per build), `reporter` (the live line), `run` (execution and summary), `rollout` (hooks). |
| `daemon/` | The ownership contract, with `supervisor` next to it. |
| `manifest/` | Manifest types and discovery, with `schema` next to it. |
| `completions.rs` | The completion tree, including the daemon-backed completers. |

Two boundaries worth keeping. `core` may not depend on `commands`, and the
`cli` modules hold no logic, only clap definitions and their doc comments.

## Conventions

**No comments in code.** Not in the Rust, not in the Makefile, not in
`tests/e2e.sh`. Names and structure carry the meaning; anything that genuinely
needs explaining belongs in this file instead. That is why the gotchas, the
build root rules, the interpolation table and the ownership contract are all
documented here at length rather than inline.

Two exceptions, both because they are functional rather than explanatory:

- `///` doc comments under `src/cli/`. clap turns these into the `--help` text,
  so deleting one deletes user facing output.
- `##` annotations on Makefile target lines. The `help` target parses them with
  awk to build its own listing.

Two non-obvious pieces of code, documented here instead of inline.

`src/completions.rs` leaks each project name with `Box::leak` because clap
wants `'static` names; the completion process emits candidates and exits
immediately, so the leak is deliberate and harmless. Project names that collide
with a reserved word are skipped, because `ac <that-name>` dispatches to the
command, not the project, so offering it would complete to something that
cannot run.

The same file shells out to `container` on every TAB to complete container
names, image references and registry hosts, which the manifest cannot supply.
That is why each of those completers is bounded rather than a plain call: a 1s
`container system status` probe gates a 2s list, both `.silent()`, both
returning an empty vector on any failure. A wedged or stopped daemon therefore
makes TAB empty and instant instead of hanging the shell, which is the one
failure mode that would make people turn completion off. `AC_COMPLETE_OFFLINE=1`
skips them outright. Do not reach for `Snapshot::query` or
`daemon::running_silent` here; both are unbounded.

Check for regressions:

```
grep -rnE '^\s*//' src --include='*.rs' | grep -v '^src/cli/'   # expect no output
grep -nE '^\s*#' Makefile tests/e2e.sh | grep -v '#!' # expect no output
```

## Docs that ship inside the binary

`docs/guide.md` and `docs/claude-snippet.md` are embedded with `include_str!`
and printed by `ac guide` / `ac guide claude`. Editing them changes user-facing
output, so treat them as part of the CLI surface. The guide is the
self-teaching entry point for agents; keep the docker-to-ac table in it
complete when adding commands.

`extras/` is a gitignored playground (an Express app with a multi-stage
Dockerfile) used by the e2e suite and for manually exercising builds; recreate
it from `tests/e2e.sh` if it is missing.

## The CLI reference under docs/cli/

`docs/cli/` is the exhaustive user-facing reference: one page per area, every
command, every flag with its short form and default, and what each runs
underneath. It is not embedded in the binary, unlike `docs/guide.md`. This file
stays the operating manual (why things are the way they are); `docs/cli/` is
the surface (what exists and what it does). **Adding or changing a command
means editing the matching page**, or the two drift:

| Page | Owns |
| --- | --- |
| `README.md` | The two invocation forms, `RESERVED`, docker-to-ac table |
| `global-flags.md` | `src/cli/root.rs` global flags, every env var, exit codes |
| `containers.md` | The manifest-free container verbs and all of `RunOpts` |
| `images-and-registries.md` | Image and registry verbs, `image`/`registry` groups |
| `project-commands.md` | Every `Action` in `src/cli/project.rs` |
| `builds.md`, `rollouts.md` | `src/build/` |
| `manifest.md` | The serde types in `src/manifest/` |
| `daemon-and-system.md` | `src/daemon/`, the noun groups, daemon gating |
| `completions.md` | `src/completions.rs` |
| `agents-and-json.md` | `--json` shapes and scripting patterns |

`.github/workflows/wiki-sync.yml` publishes `docs/` to the repo wiki on every
push to `main` via `scripts/sync-wiki.mjs`, which rewrites relative markdown
links into wiki slugs. The wiki is generated output: edit `docs/`, never the
wiki. Run `node scripts/sync-wiki.mjs --out /tmp/wiki` to preview locally.

The wiki is the public home of the reference, so `README.md` and `Cargo.toml`'s
`documentation` field link to `https://github.com/pulkitxm/ac/wiki/<slug>`
rather than to files under `docs/cli/`. Slugs come from `sync-wiki.mjs`:
`docs/cli/README.md` is `CLI`, and every other page is `CLI-<Title-Case-Name>`,
so **renaming a page under `docs/cli/` breaks the README links** and both must
move together. This file is deliberately not published to the wiki.

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
- **`stop` trusts observed state, not exit codes.** After stopping, the
  container list is re-read; a container that still runs makes the command
  fail loudly instead of reporting success.
- **`start --recreate` recreates running containers too**, stopping them
  first, instead of silently short-circuiting on "already running".
- **The supervisor debounce is implemented.** bash left it as a `TODO(human)`:
  it counted idle polls but never acted on them, so an ac-owned daemon was only
  ever reaped by the synchronous check inside `stop`/`down`, never by the
  watchdog.
- **One `container ls -a` per logical operation** instead of one per service, so
  the echoed output stays readable and status is a single consistent snapshot.
- **`--json` moves human log lines to stderr** so stdout is one parseable
  document.
