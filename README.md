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

You need an Apple Silicon Mac on macOS 15 or newer, and a Rust toolchain.

1. Install Apple `container` (1.1.0 or newer). Download the signed `.pkg`
   installer from the
   [releases page](https://github.com/apple/container/releases), run it, then
   check:

   ```bash
   container --version
   ```

   You do not need to start anything: `ac` starts and stops the daemon itself,
   exactly when it is needed.

2. Install `ac`:

   ```bash
   cargo install ac-cli
   ```

   The crate is `ac-cli`; the binary it installs is `ac`. Nothing gates the
   build to macOS, so `cargo install` succeeds anywhere, but the binary drives
   Apple `container` and is useless without it.

   To build from a checkout instead, clone the repo and run `make install`,
   which symlinks the release binary into `~/.local/bin`. See
   [CLAUDE.md](CLAUDE.md) for `BIN_DIR`, `BIN_NAME` and pointing at a toolchain
   outside `~/.cargo`.

3. Wire up your shell, in `~/.zshrc`:

   ```zsh
   export PATH="$HOME/.local/bin:$PATH"
   source <(COMPLETE=zsh ac)
   ```

   Other shells, and what gets completed, are in
   [Completions](docs/cli/completions.md).

## Quickstart

A project is one JSON manifest describing the services that make up a stack.
Drop it in `~/.config/ac/projects/<name>.json`:

```json
{
  "name": "myapp",
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

```bash
ac ls              # your project is discovered
ac myapp start     # daemon up if needed, volumes created, services started
ac myapp ls        # state, IPs, ports
ac myapp logs -f   # follow all services, prefixed and coloured
ac myapp down      # stop and remove containers; volumes and data survive
```

Services start in array order, each gated on the previous one's `readyCmd`,
which stands in for the healthcheck primitive Apple `container` does not have.
Containers are named `<project>-<service>`, and unknown manifest fields are
rejected by name so typos surface immediately. Every field, including builds,
profiles, registries and scripts, is in the
[Manifest reference](docs/cli/manifest.md).

## Documentation

The full CLI reference is in [`docs/cli/`](docs/cli/README.md), and is
published to the [wiki](https://github.com/pulkitxm/ac/wiki). `ac guide` prints
a manual from inside the binary, and every `--help` is written to be read cold.

| Page | What it covers |
| --- | --- |
| [CLI reference](docs/cli/README.md) | The two invocation forms, reserved words, docker-to-ac translation table |
| [Global flags](docs/cli/global-flags.md) | `--json`, `--quiet`, `--no-color`, `-p`, every environment variable, exit codes |
| [Containers](docs/cli/containers.md) | `run`, `create`, `start`, `stop`, `exec`, `logs`, `cp`, and the rest of the manifest-free verbs |
| [Images and registries](docs/cli/images-and-registries.md) | `build`, `pull`, `push`, `tag`, `login`, and the `image` / `registry` groups |
| [Project commands](docs/cli/project-commands.md) | Every `ac <project> <action>`, with flags and readiness semantics |
| [Builds](docs/cli/builds.md) | Profiles, precedence, build root resolution, interpolation, live progress |
| [Rollouts](docs/cli/rollouts.md) | Post-push hooks and the environment handed to them |
| [Manifest](docs/cli/manifest.md) | Field-by-field schema, discovery, private registries, `scripts` |
| [Daemon and system](docs/cli/daemon-and-system.md) | Ownership, the supervisor, `ps`, `status`, `system`, `volume`, `network`, `builder`, `ac config` |
| [Completions](docs/cli/completions.md) | Shell setup and what completes |
| [Agents and JSON](docs/cli/agents-and-json.md) | Driving `ac` from scripts, CI and coding agents |

## Daemon ownership

The part worth understanding, because it is the whole point of the tool.

| Situation on `ac <project> start` | What `ac` does |
| --- | --- |
| Daemon **already running** | Uses it. Never starts, restarts or stops it, including on `ac <project> stop`. |
| Daemon **not running** | Starts it, records ownership in `~/.local/state/ac/daemon.owned`, and spawns a supervisor. |

When `ac` owns the daemon, a detached supervisor polls for running containers
and stops the daemon once the last `ac`-managed container disappears, whether
you ran `ac <project> stop`, the containers exited on their own, or they
crashed. Ownership lives in a file rather than in memory, so a second `ac`
invocation from another terminal makes the same decision, and the refcount
spans **all** projects. Full contract in
[Daemon and system](docs/cli/daemon-and-system.md).

## Two surfaces

`ac <project> <action> [services...]` acts on services resolved through a
manifest, and is the only form that does ordered startup gated on `readyCmd`,
named volume creation and filtered registry login. `ac <action> <container>`
acts on one container or image by its real name, no manifest involved:

```bash
ac build -t app:dev .              # a Dockerfile in this directory
ac run -d -p 3000:3000 app:dev     # run it, and print the URL
ac logs -f app-dev                 # follow it
```

Do not write a manifest just to run one container. Both surfaces mirror docker,
the noun groups (`ac ps`, `ac image ls`, `ac volume prune`) and the verbs alike,
with `--json` on every read. The
[CLI reference](docs/cli/README.md) has the complete list.

## Notes on Apple Container

- One lightweight VM **per container**, each with its own kernel, so `memory`
  is VM sizing and container counts cost real RAM.
- Every container gets a routable IP (`192.168.64.x`), reachable without
  publishing ports (`ac <project> ip` prints them). ICMP is blocked, so `ping`
  fails even when TCP works.
- Named volumes are real ext4 block devices, so a fresh one already contains a
  `lost+found`. Anything insisting on an empty directory refuses to start,
  which is why the example manifests point `PGDATA` at a subdirectory. This
  does not happen on Docker, where volumes are plain directories.

The rest of the sharp edges, and what `ac` does about each, are in
[CLAUDE.md](CLAUDE.md).
