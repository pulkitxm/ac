# ac

A CLI for Apple's [`container`](https://github.com/apple/container) that
replaces both halves of docker on macOS: `ac run` / `ac build` / `ac logs` for
one-off containers and images, and `ac <project> start` for whole service
stacks, filling the gap left by the absence of `docker compose`.

```console
$ ac shop start
==> starting container daemon
  ok daemon started (owned by ac)
==> starting shop-postgres
  waiting for shop-postgres .. ready
  ok shop-postgres up  192.168.64.2/24
...
```

## Install

You need an Apple Silicon Mac on macOS 15 or newer, and a Rust toolchain to
build the binary.

1. Install Apple `container` (1.1.0 or newer). Download the signed `.pkg`
   installer from the
   [releases page](https://github.com/apple/container/releases), run it, then
   check:

   ```bash
   container --version
   ```

   You do not need to start anything: `ac` starts and stops the daemon itself,
   exactly when it is needed.

2. Build and install `ac`:

   ```bash
   git clone https://github.com/pulkitxm/ac.git
   cd ac
   make install
   ```

   This builds the release binary and symlinks it into `~/.local/bin` (override
   with `BIN_DIR=...` or `BIN_NAME=...`). If your Rust toolchain lives outside
   `~/.cargo`, point at it in an untracked `Makefile.local`:

   ```make
   CARGO_HOME  := /path/to/cargo
   RUSTUP_HOME := /path/to/rustup
   ```

3. Wire up your shell, in `~/.zshrc`:

   ```zsh
   export PATH="$HOME/.local/bin:$PATH"
   source <(COMPLETE=zsh ac)
   ```

   For bash, use `COMPLETE=bash` in `~/.bashrc`. Completion covers projects,
   actions, service names, flags, signal names, and — from the live daemon —
   container names, image references and registry hosts. The daemon-backed
   ones are bounded and fail silently, so TAB is never slower than a moment
   even with the daemon down; `AC_COMPLETE_OFFLINE=1` disables them.

## Quickstart

A project is one JSON manifest describing the services that make up a stack.
Drop it in `~/.config/ac/projects/<name>.json`:

```json
{
  "name": "myapp",
  "description": "whatever this stack is",
  "services": [
    {
      "name": "postgres",
      "image": "docker.io/library/postgres:16-alpine",
      "cpus": 2,
      "memory": "1g",
      "ports": ["5433:5432"],
      "env": { "POSTGRES_USER": "user" },
      "volumes": [{ "name": "pg-data", "target": "/var/lib/postgresql/data" }],
      "readyCmd": ["pg_isready", "-U", "user"],
      "readyTimeout": 90
    }
  ]
}
```

Then:

```bash
ac ls              # your project is discovered
ac myapp start     # daemon up if needed, volumes created, services started
ac myapp ls        # state, IPs, ports
ac myapp logs -f   # follow all services, prefixed and coloured
ac myapp down      # stop and remove containers; volumes and data survive
```

| Field | Meaning |
| --- | --- |
| `image` | Full OCI reference. Include the registry (`docker.io/library/...`). |
| `cpus`, `memory` | Sizes the container's **VM**, not a cgroup. Each container is its own VM. |
| `ports` | `host:container`, same as Docker. Optional, since every container also gets its own routable IP. |
| `env` | Key/value map. |
| `volumes` | Named volumes; the real volume is `<project>-<name>`, created on demand. |
| `readyCmd` | Polled via `container exec` until it exits 0. Apple `container` has no healthcheck primitive, so readiness is implemented here. |
| `readyTimeout` | Seconds before giving up (start continues anyway, with a warning). |

Services start in array order, each gated on the previous one's `readyCmd`.
Containers are named `<project>-<service>`. Unknown manifest fields are
rejected by name, so typos surface immediately. `ac schema` prints the full
JSON Schema, and the bundled `projects/shop.json` is a complete worked example
with builds, profiles and registries.

A manifest may also declare `scripts`, npm run style: a name mapped to one
shell string. `ac <project> <name> [args...]` runs the string with `sh -c`,
appending the extra arguments shell-quoted, so project-specific tooling (ssh
tunnels, port-forwards, db consoles) lives behind the same front door. The
script owns its own subcommands; `ac <project> scripts` lists what a project
declares, and shell completion offers the names next to the built-in actions.

## Daemon ownership

The part worth understanding, because it is the whole point of the tool.

| Situation on `ac <project> start` | What `ac` does |
| --- | --- |
| Daemon **already running** | Uses it. Never starts, restarts or stops it, including on `ac <project> stop`. |
| Daemon **not running** | Starts it, records ownership in `~/.local/state/ac/daemon.owned`, and spawns a supervisor. |

When `ac` owns the daemon, a detached supervisor process polls for running
containers. Once the last `ac`-managed container disappears (whether you ran
`ac <project> stop`, the containers exited on their own, or they crashed), the
supervisor stops the daemon and exits.

Ownership lives in a file rather than in memory, so a second `ac` invocation
from a different terminal makes the same decision. Shutdown refcounts across
**all** projects: stopping one project while another is still up leaves the
daemon running.

## Usage

Most actions take optional service names and default to every service, so
`ac shop restart` restarts the stack and `ac shop restart redis` restarts
one container. Services can be named either bare (`redis`) or by container name
(`shop-redis`), since the latter is what `ac shop ls` prints.

```
ac <project> start | up [svc...]   start services (up is an alias)
ac <project> stop [-t SECS] [svc...]  stop services, containers kept in place
ac <project> down [-v] [svc...]    stop and remove; -v also deletes volumes
ac <project> restart [svc...]      stop then start
ac <project> ls | ps | status      per-service state, IP, published ports
ac <project> logs [-f] [-n N] [svc]  logs; no service means all, interleaved
ac <project> run [--keep] <svc> [cmd...]  one-off container from the service
ac <project> create [svc...]       create containers without starting them
ac <project> exec <svc> <cmd...>   run a command in a service
ac <project> sh [svc]              interactive shell (bash if present, else sh)
ac <project> top [svc...]          processes inside each running service
ac <project> wait [--timeout N]    block until ready; exit code says so
ac <project> stats [--no-stream]   live resource usage
ac <project> inspect [svc...]      full container JSON
ac <project> kill [-s SIG] [svc..] send a signal, default KILL
ac <project> rm [svc...]           force remove containers, keeping volumes
ac <project> cp <src> <dst>        copy files; svc:/path for the container side
ac <project> pull [svc...]         pre-pull images
ac <project> build [name...]       build images, live per-step progress
ac <project> push [-P profile]     push already-built tags, no rebuild
ac <project> export <svc> [-o f]   stopped service's filesystem as a tar
ac <project> images | volumes      what the project declares, and its state
ac <project> port | ip | env       mappings, addresses, environment
ac <project> login                 authenticate to private registries
ac <project> config                the project manifest

ac ls                              list projects
ac status                          daemon + supervisor + every project
ac ps [-a] [-q]                    containers across every project, with
                                   project and service attribution
ac image ls|pull|push|rm|tag|inspect|prune|save|load   (ls shows sizes; -q names only; rmi works)
ac volume ls|create|rm|inspect|prune
ac network ls|create|rm|inspect|prune
ac system info|df|start|stop|prune|logs
ac registry login|logout|ls
ac daemon status | stop            who owns the daemon; stop only if ac's
ac builder status|start|stop|delete   the shared image builder
ac machine [args...]               container machine, passed through
ac guide [claude]                  built-in manual; claude prints a CLAUDE.md snippet
ac config | schema                 resolved configuration; manifest schema
```

No manifest needed, the plain docker CLI:

```
ac build -t app:dev .              build a Dockerfile in this directory
ac run -d -p 3000:3000 app:dev     run it, and print the URL
ac create|start|stop|restart|rm    container lifecycle, by container name
ac exec [-it] <c> <cmd...>         run a command inside one
ac sh <c>                          bash if the image has it, else sh
ac logs [-f] [-n N] <c>            container logs
ac inspect|port|stats|top <c>      what it is, what it publishes, what it uses
ac cp <src> <dst>                  either side may be <container>:/path
ac export <c> [-o file]            filesystem tarball (container must be stopped)
ac kill [-s SIG] <c...>            signal it
ac pull|push|tag|save|load         image verbs, docker spelling
ac login|logout <server>           registry credentials
```

Both surfaces mirror docker: the noun groups (`ac ps`, `ac image ls`,
`ac volume prune`) and the verbs (`ac run`, `ac build`, `ac logs`) map straight
onto the underlying `container` commands, with `--json` on every read.
`ac system start`/`stop` respect the ownership rule: ac never stops a daemon it
did not start. Containers made by `ac run` are labelled `ac.managed=1` so they
hold that daemon up for as long as they live.

Use `ac <project> <verb>` when a manifest declares the thing, because only that
form does readiness gating, named volumes and filtered registry login. Use the
bare verbs for everything else, and do not write a manifest just to run one
container.

## Builds

`ac <project> build` runs every build in the manifest in parallel. On a TTY
each build renders a single live line: current step position, the instruction
being run, per-step elapsed and total elapsed, all ticking in real time.
Finished steps print compactly as they complete, cached steps are marked, and
a failing build replays its last output lines so the cause is on screen.

```
⠸ web  [9/14] RUN pnpm install --frozen-lockfile  41.2s | total 1m03s
   + [web] [8/14] COPY package.json pnpm-lock.yaml ./  0.1s
   - [web] [7/14] WORKDIR /app  cached
```

`--progress plain` streams raw buildkit lines instead, `--sequential` builds
one image at a time, `--dry-run` prints the resolved plan without touching
anything, and `--json` emits a machine-readable summary per build. Every
setting resolves CLI flag > profile > build entry > project default.

## Agents

The CLI is written to be driven by coding agents as much as by people:

- `--json` on every read command puts one parseable document on stdout and
  moves human chatter to stderr.
- `ac guide` prints a complete manual, including a docker-to-ac table, so an
  agent can teach itself the tool at runtime. `ac guide claude` emits a short
  snippet to paste into another repository's CLAUDE.md.
- Every underlying `container` command is echoed to stderr before it runs, so
  any step can be copied and re-run by hand.
- `ac <project> wait` turns readiness into an exit code to gate on.

## Private registries (AWS ECR, GHCR, and friends)

Declare a `registries` block and `ac` authenticates before pulling, on every
`start` and `pull`. Credentials are never written into the manifest: you give a
`passwordCmd` argv that is executed and piped to `--password-stdin`.

```json
{
  "name": "myapp",
  "registries": [
    {
      "server": "123456789012.dkr.ecr.us-east-1.amazonaws.com",
      "username": "AWS",
      "passwordCmd": ["aws", "ecr", "get-login-password", "--region", "us-east-1"]
    }
  ],
  "services": [
    { "name": "api", "image": "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest" }
  ]
}
```

Re-running on every start matters for ECR, whose tokens expire after 12 hours.
`ac <project> login` runs the same step on its own. The pattern works for any
registry that takes a username and a token on stdin:

```json
{ "server": "ghcr.io", "username": "you", "passwordCmd": ["gh", "auth", "token"] }
```

## Configuration

`~/.config/ac/config.json`, created on first run:

```json
{
  "appRoot": "/path/to/app-root",
  "startTimeout": 90
}
```

`appRoot` is passed as `--app-root` when `ac` starts the daemon, and is seeded
from the running daemon on first run so `ac` keeps using your existing image
store.

## Notes on Apple Container

- One lightweight VM **per container**, each with its own kernel, so `memory`
  is VM sizing, and container counts cost real RAM.
- Every container gets a routable IP (`192.168.64.x`). You can reach it directly
  without publishing ports; `ac <project> ip` prints them.
- ICMP is blocked host to container, so `ping` fails even when TCP works.
- Named volumes are real ext4 block devices, not host directories, so every
  fresh volume contains a `lost+found`. Anything that insists on an empty
  directory will refuse to start. Postgres is the common case, which is why the
  example manifests set `PGDATA` to a subdirectory of the mount point:

  ```
  initdb: error: directory "/var/lib/postgresql/data" exists but is not empty
  initdb: detail: It contains a lost+found directory
  ```

  This does not happen on Docker, where named volumes are plain directories.
