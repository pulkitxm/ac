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

   For bash, use `COMPLETE=bash` in `~/.bashrc`. See
   [Completions](docs/cli/completions.md) for the other shells and for what gets
   completed.

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

Services start in array order, each gated on the previous one's `readyCmd`.
Containers are named `<project>-<service>`. Unknown manifest fields are
rejected by name, so typos surface immediately. Every field is documented in
the [Manifest reference](docs/cli/manifest.md); `ac schema` prints the JSON
Schema, and the bundled `projects/shop.json` is a complete worked example with
builds, profiles and registries.

## Documentation

The full CLI reference lives in [`docs/cli/`](docs/cli/README.md), and is also
published to the [wiki](https://github.com/pulkitxm/ac/wiki). `ac guide` prints
a manual from inside the binary, and `ac <anything> --help` is written to be
read cold.

| Page | What it covers |
| --- | --- |
| [CLI reference](docs/cli/README.md) | The two invocation forms, reserved words, and a docker-to-ac translation table |
| [Global flags](docs/cli/global-flags.md) | `--json`, `--quiet`, `--no-color`, `-p`, every environment variable, exit codes |
| [Containers](docs/cli/containers.md) | `run`, `create`, `start`, `stop`, `exec`, `logs`, `cp`, and the rest of the manifest-free verbs |
| [Images and registries](docs/cli/images-and-registries.md) | `build`, `pull`, `push`, `tag`, `login`, and the `image` / `registry` groups |
| [Project commands](docs/cli/project-commands.md) | Every `ac <project> <action>`, with flags and readiness semantics |
| [Builds](docs/cli/builds.md) | Profiles, precedence, build root resolution, interpolation, progress modes |
| [Rollouts](docs/cli/rollouts.md) | Post-push hooks and the environment handed to them |
| [Manifest](docs/cli/manifest.md) | Field-by-field schema reference, discovery, and `scripts` |
| [Daemon and system](docs/cli/daemon-and-system.md) | Ownership, the supervisor, `ps`, `status`, `system`, `volume`, `network`, `builder` |
| [Completions](docs/cli/completions.md) | Shell setup, what completes, and how the daemon-backed completers are bounded |
| [Agents and JSON](docs/cli/agents-and-json.md) | Driving `ac` from scripts, CI and coding agents |

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
daemon running. The full contract is in
[Daemon and system](docs/cli/daemon-and-system.md).

## Two surfaces

`ac <project> <action> [services...]` acts on services resolved through a
manifest, and is the only form that does ordered startup gated on `readyCmd`,
named volume creation and filtered registry login. `ac <action> <container>`
acts on one container or image by its real name, with no manifest involved:

```bash
ac build -t app:dev .              # a Dockerfile in this directory
ac run -d -p 3000:3000 app:dev     # run it, and print the URL
ac logs -f app-dev                 # follow it
```

Do not write a manifest just to run one container. Both surfaces mirror docker,
noun groups (`ac ps`, `ac image ls`, `ac volume prune`) and verbs (`ac run`,
`ac build`, `ac logs`) alike, with `--json` on every read. See the
[CLI reference](docs/cli/README.md) for the complete list and the docker
translation table.

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

Every setting resolves CLI flag > profile > build entry > project default.
Profiles, build root resolution, `{{...}}` interpolation and the progress modes
are covered in [Builds](docs/cli/builds.md); shipping what you built is in
[Rollouts](docs/cli/rollouts.md).

## Private registries (AWS ECR, GHCR, and friends)

Declare a `registries` block and `ac` authenticates before pulling, on every
`start` and `pull`. Credentials are never written into the manifest: you give a
`passwordCmd` argv that is executed and piped to `--password-stdin`.

```json
{
  "server": "123456789012.dkr.ecr.us-east-1.amazonaws.com",
  "username": "AWS",
  "passwordCmd": ["aws", "ecr", "get-login-password", "--region", "us-east-1"]
}
```

Re-running on every start matters for ECR, whose tokens expire after 12 hours.
Login is filtered to the registries the images actually come from, so pulling
postgres from docker.io never touches ECR. The pattern works for any registry
that takes a username and a token on stdin:

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
