# ac — Apple Container project runner

A small CLI for running project-scoped service stacks on Apple's
[`container`](https://github.com/apple/container), filling the gap left by the
absence of `docker compose`.

```console
$ ac demo start
==> starting container daemon
  ok daemon started (owned by ac)
==> starting demo-postgres
  waiting for demo-postgres .. ready
  ok demo-postgres up  192.168.64.2/24
...
```

## Daemon ownership

This is the part worth understanding, because it is the whole point of the tool.

| Situation on `ac <project> start` | What `ac` does |
| --- | --- |
| Daemon **already running** | Uses it. Never starts, restarts or stops it — including on `ac <project> stop`. |
| Daemon **not running** | Starts it, records ownership in `~/.local/state/ac/daemon.owned`, and spawns a supervisor. |

When `ac` owns the daemon, a detached supervisor process polls for running
containers. Once the last `ac`-managed container disappears — whether you ran
`ac <project> stop`, the containers exited on their own, or they crashed — the
supervisor stops the daemon and exits.

Ownership lives in a file rather than in memory, so a second `ac` invocation
from a different terminal makes the same decision.

Shutdown refcounts across **all** projects: stopping `demo` while another
project is still up leaves the daemon running.

## Install

```bash
git clone git@github.com:pulkitxm/ac.git ~/scripts/ac
cd ~/scripts/ac && ./install.sh
```

Then, in `~/.zshrc`:

```zsh
export PATH="$HOME/.local/bin:$PATH"
export AC_HOME="$HOME/scripts/ac"
fpath=("$AC_HOME/completions" $fpath)
autoload -Uz compinit && compinit
```

Requires `jq` and Apple `container` 1.1.0+.

## Usage

```
ac <project> start           bring every service up, in manifest order
ac <project> stop            stop and remove them
ac <project> restart
ac <project> status | ps     per-service state, IP, published ports
ac <project> logs <service>
ac <project> exec <service> <cmd...>
ac <project> ip [service]

ac ls                        list projects
ac status                    daemon + supervisor + every project
ac daemon status             who owns the daemon right now
ac daemon stop               stop it — only if ac started it
ac config                    resolved configuration
```

Tab completion covers projects, actions and service names.

## Adding a project

Drop a JSON file into `projects/`, or into `~/.config/ac/projects/` to keep it
private. User projects shadow repo ones with the same name.

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

| Field | Meaning |
| --- | --- |
| `image` | Full OCI reference. Include the registry (`docker.io/library/...`). |
| `cpus`, `memory` | Sizes the container's **VM**, not a cgroup. Each container is its own VM. |
| `ports` | `host:container`, same as Docker. Optional — every container also gets its own routable IP. |
| `env` | Key/value map. |
| `volumes` | Named volumes; the real volume is `<project>-<name>`, created on demand. |
| `readyCmd` | Polled via `container exec` until it exits 0. Apple `container` has no healthcheck primitive, so readiness is implemented here. |
| `readyTimeout` | Seconds before giving up (start continues anyway, with a warning). |

Services start in array order, each gated on the previous one's `readyCmd`.

Containers are named `<project>-<service>`, which is also how `ac` recognises
its own containers when deciding whether the daemon can be shut down.

## Configuration

`~/.config/ac/config.json`, created on first run:

```json
{
  "appRoot": "/Volumes/ContainerData/app-root",
  "sparseBundle": "/Volumes/External/container-data.sparsebundle",
  "imageMount": "/Volumes/ContainerData",
  "startTimeout": 90
}
```

- `appRoot` — passed as `--app-root` when `ac` starts the daemon. Seeded from
  the running daemon on first run so `ac` keeps using your existing image store.
- `sparseBundle` / `imageMount` — if set and not mounted, the bundle is attached
  with `hdiutil attach -owners on` before the daemon starts. This is needed when
  the app root lives on a volume mounted `noowners`, which otherwise makes
  `container-apiserver` abort with `XPC connection error: Connection invalid`.

## Notes on Apple Container

- One lightweight VM **per container**, each with its own kernel — so `memory`
  is VM sizing, and container counts cost real RAM.
- Every container gets a routable IP (`192.168.64.x`). You can reach it directly
  without publishing ports; `ac <project> ip` prints them.
- ICMP is blocked host→container, so `ping` fails even when TCP works.
