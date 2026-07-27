# ac

A small CLI for running project-scoped service stacks on Apple's
[`container`](https://github.com/apple/container), filling the gap left by the
absence of `docker compose`.

```console
$ ac noveum start
==> starting container daemon
  ok daemon started (owned by ac)
==> starting noveum-postgres
  waiting for noveum-postgres .. ready
  ok noveum-postgres up  192.168.64.2/24
...
```

## Daemon ownership

This is the part worth understanding, because it is the whole point of the tool.

| Situation on `ac <project> start` | What `ac` does |
| --- | --- |
| Daemon **already running** | Uses it. Never starts, restarts or stops it, including on `ac <project> stop`. |
| Daemon **not running** | Starts it, records ownership in `~/.local/state/ac/daemon.owned`, and spawns a supervisor. |

When `ac` owns the daemon, a detached supervisor process polls for running
containers. Once the last `ac`-managed container disappears (whether you ran
`ac <project> stop`, the containers exited on their own, or they crashed), the
supervisor stops the daemon and exits.

Ownership lives in a file rather than in memory, so a second `ac` invocation
from a different terminal makes the same decision.

Shutdown refcounts across **all** projects: stopping `noveum` while another
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

Most actions take optional service names and default to every service, so
`ac noveum restart` restarts the stack and `ac noveum restart redis` restarts
one container. Services can be named either bare (`redis`) or by container name
(`noveum-redis`), since the latter is what `ac noveum ls` prints.

```
ac <project> start [svc...]        start services
ac <project> stop [svc...]         stop and remove services
ac <project> restart [svc...]      stop then start
ac <project> ls | ps | status      per-service state, IP, published ports
ac <project> logs [-f] [-n N] [svc]  logs; no service means all, interleaved
ac <project> exec <svc> <cmd...>   run a command in a service
ac <project> sh [svc]              interactive shell (bash if present, else sh)
ac <project> stats [svc...]        live resource usage
ac <project> inspect [svc...]      full container JSON
ac <project> kill [-s SIG] [svc..] send a signal, default KILL
ac <project> rm [svc...]           force remove containers, keeping volumes
ac <project> cp <src> <dst>        copy files; svc:/path for the container side
ac <project> pull [svc...]         pre-pull images
ac <project> images                images this project uses
ac <project> port [svc]            published port mappings
ac <project> ip [svc]              container IPs
ac <project> env <svc>             environment from the manifest
ac <project> login                 authenticate to private registries
ac <project> config                the project manifest

ac ls                              list projects
ac status                          daemon + supervisor + every project
ac daemon status                   who owns the daemon right now
ac daemon stop                     stop it, only if ac started it
ac images                          every image in the local store
ac df                              disk usage
ac prune                           remove stopped containers, unused images
ac config                          resolved configuration
```

`ac <project> logs -f` with no service follows every container at once, each
line prefixed and coloured by service, the way `docker compose logs -f` does.
Apple `container logs` only handles one container, so the fan-out happens in
`ac` and Ctrl-C tears down the whole group.

Tab completion covers projects, actions, service names (both forms), flags and
signal names, in zsh and bash.

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
{ "server": "ghcr.io", "username": "pulkitxm", "passwordCmd": ["gh", "auth", "token"] }
```

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
| `ports` | `host:container`, same as Docker. Optional, since every container also gets its own routable IP. |
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
  "sparseBundle": "/Volumes/Sandisk SSD/container-data.sparsebundle",
  "imageMount": "/Volumes/ContainerData",
  "startTimeout": 90
}
```

- `appRoot`: passed as `--app-root` when `ac` starts the daemon. Seeded from
  the running daemon on first run so `ac` keeps using your existing image store.
- `sparseBundle` / `imageMount`: if set and not mounted, the bundle is attached
  with `hdiutil attach -owners on` before the daemon starts. This is needed when
  the app root lives on a volume mounted `noowners`, which otherwise makes
  `container-apiserver` abort with `XPC connection error: Connection invalid`.

## Notes on Apple Container

- One lightweight VM **per container**, each with its own kernel, so `memory`
  is VM sizing, and container counts cost real RAM.
- Every container gets a routable IP (`192.168.64.x`). You can reach it directly
  without publishing ports; `ac <project> ip` prints them.
- ICMP is blocked host to container, so `ping` fails even when TCP works.
- Named volumes are real ext4 block devices, not host directories, so every
  fresh volume contains a `lost+found`. Anything that insists on an empty
  directory will refuse to start. Postgres is the common case, which is why the
  noveum manifest sets `PGDATA` to a subdirectory of the mount point:

  ```
  initdb: error: directory "/var/lib/postgresql/data" exists but is not empty
  initdb: detail: It contains a lost+found directory
  ```

  This does not happen on Docker, where named volumes are plain directories.
