# The project manifest

A project is one JSON file. It declares the services that make up a local
stack, the images to build, the registries to log in to, and any custom
scripts. `ac <project> <action>` reads it; nothing else about a project is
stored anywhere.

This page is the field-by-field reference. For the commands that consume it see
[Project commands](project-commands.md); for the manifest-free verbs that need
no project at all see [Container commands](containers.md) and
[Images and registries](images-and-registries.md).

## Discovery and shadowing

`ac` looks for `<name>.json` in two directories, highest priority first:

1. `~/.config/ac/projects/<name>.json` (user)
2. `<ac home>/projects/<name>.json` (bundled with the repo)

The first file that exists wins, so a user file **shadows** a bundled one of
the same name. The repo stays cleanly updatable while remaining customisable.

The config directory follows `XDG_CONFIG_HOME` when it is set, so the user
directory is really `$XDG_CONFIG_HOME/ac/projects/`, defaulting to
`~/.config/ac/projects/`.

The "ac home" for bundled projects is resolved in `src/core/ctx.rs`, in this
order:

| Source | Value |
| --- | --- |
| `$AC_HOME` | used verbatim |
| the binary's own path | the nearest ancestor directory of the resolved executable that contains a `projects/` directory |
| fallback | `~/scripts/ac` |

Project **names** come from the file stem, not from anything inside the file.
`ac ls` lists the union of both directories, sorted, deduplicated.

Without `--json`, `ac ls` prints one name per line and nothing else.

```console
$ ac ls
sandbox
shop

$ ac --json ls
[
  {
    "name": "shop",
    "description": "shop local backing services",
    "file": "/Users/me/.config/ac/projects/shop.json",
    "services": ["postgres", "redis"],
    "builds": ["api"]
  }
]
```

A project that fails to parse still appears in `ac --json ls`, as
`{"name": "...", "error": "..."}`, so a broken manifest is visible rather than
silently missing.

## Naming conventions

The tool relies on two rules, and nothing configures them:

- the container for a service is `<project>-<service>`
- the real name of a named volume is `<project>-<volume>`

That is why services resolve from either spelling: `ac shop stop redis` and
`ac shop stop shop-redis` are the same thing. Naming a service that does not
exist fails and lists the valid ones.

## Validation

Every field is typed and **unknown fields are rejected**, so a typo produces an
error naming the bad field and its near neighbours rather than being silently
ignored:

```console
$ ac shop config
Error: invalid manifest /Users/me/.config/ac/projects/shop.json at line 12 column 9:
unknown field `portz`, expected one of `name`, `image`, `cpus`, `memory`, `ports`, ...
```

Beyond the type check, `src/manifest/mod.rs` enforces:

| Rule | Error |
| --- | --- |
| `name` must equal the file stem | "manifest ... declares name 'x' but the file is y.json" |
| no duplicate service names | "declares the service 's' more than once" |
| no duplicate build names | "declares the build 'b' more than once" |
| a script name must not collide with a project action | "collides with the ac action of the same name" |
| a script name must be a single word and must not start with `-` | "script names must be single words" |
| a script body must not be empty or whitespace | "with an empty command" |
| every `complete` word must be a single non-empty word | "completion words must be single words" |

The name rule exists because containers are named after the file, not after the
`name` field, so a mismatch would produce containers nobody can find.

## `ac schema`

Prints the manifest JSON Schema (draft 2020-12) to stdout. It is generated from
the same types that parse the manifest, so it never drifts.

```bash
ac schema > manifest.schema.json
```

Point an editor at it for completion and inline validation:

```json
{
  "$schema": "./manifest.schema.json",
  "name": "shop"
}
```

`ac schema` needs no daemon and no project.

## Top level fields

Only `name` is required. Every other field defaults to empty.

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `name` | string | yes | | Project name. Must match the file stem. |
| `description` | string | no | `""` | One line shown by `ac status` and by `ac --json ls`. |
| `root` | string | no | unset | Default directory builds run from. Overridden by `--root`, `$AC_ROOT`, and by the git worktree containing `$PWD` when that tree holds the manifest's first declared dockerfile. Also exported to scripts as `AC_PROJECT_ROOT`. |
| `region` | string | no | `us-east-1` (at use time) | Default value of the `{{region}}` placeholder. |
| `builder` | object | no | unset | Sizing for the shared buildkit builder. See [builder](#builder). |
| `profiles` | object | no | `{}` | Named build targets, keyed by profile name. See [profiles](#profiles). |
| `registries` | array | no | `[]` | Private registries to authenticate against. See [registries](#registries). |
| `builds` | array | no | `[]` | Image builds. See [builds](#builds). |
| `services` | array | no | `[]` | Containers this project runs. See [services](#services). |
| `scripts` | object | no | `{}` | Custom commands. See [scripts](#scripts). |

`profiles` and `scripts` keep **declaration order**, so `ac <project> scripts`
lists them in the order they appear in the file. `ac <project> profiles` sorts
its output. The JSON object fields (`env`, `buildArgs`, `labels`) also keep
their file order, so `ac <project> env` prints variables as written.

### builder

Sizing for the shared buildkit builder container. Both fields are optional.

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `cpus` | integer (min 1) | no | unset | CPUs for the builder VM. |
| `memory` | string | no | unset | Memory for the builder VM, for example `8g` or `4096m`. |

These values are read only when the builder container is **created**, so
changing them makes `ac` stop the builder first, discarding its layer cache.
`ac` says so loudly when it happens.

```json
"builder": { "cpus": 8, "memory": "8g" }
```

### profiles

A map of profile name to profile object, selected with `-P/--profile` on
`build`, `push`, `rollout` and `login`. Profile values override build entries
and project defaults.

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `platform` | string | no | unset | Build platform, for example `linux/arm64` or `linux/amd64`. |
| `push` | boolean | no | `false` | Push after building. |
| `tag` | string | no | unset | Value of `{{tag}}`. |
| `account` | string | no | unset | Value of `{{account}}`. |
| `region` | string | no | unset | Value of `{{region}}`, overriding the project default. |
| `registry` | string | no | unset | Value of `{{registry}}`: a host plus trailing slash, or empty for purely local profiles. Itself interpolated, so it may contain `{{account}}` and `{{region}}`. |
| `rollout` | object | no | unset | How this profile ships what it pushed. See [profiles.rollout](#profilesrollout). |

A profile with no `rollout` key can never deploy, which is what keeps a `local`
profile safe.

### profiles.rollout

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `description` | string | no | unset | Shown by `ac <project> rollout --dry-run`. |
| `preflight` | array of argv arrays | no | `[]` | Run **before anything is built**, before the daemon is ensured and before any registry login. A failure aborts in seconds. |
| `run` | array of argv arrays | no | `[]` | Run after every build and push in the invocation succeeded. |
| `auto` | boolean | no | `false` | Roll out on every `ac <project> build` for this profile without `--rollout`. `--no-rollout` still wins. |

Hooks are argv arrays (arrays of arrays), run from the resolved build root,
with `{{...}}` interpolation plus `{{image.<build>}}`, and receive the resolved
references in the environment (`AC_IMAGE_<BUILD>`, `AC_IMAGES`, `AC_BUILDS`, and
the resolved profile values). See [Builds and rollouts](builds.md).

### registries

An array of registries to authenticate against before pulling or pushing. `ac`
only contacts a registry an image actually comes from, which is what stops
`ac shop start` logging in to ECR merely to pull postgres from docker.io. An
explicit `ac <project> login` uses every declared registry.

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `server` | string | yes | | Registry host. Supports `{{...}}`. |
| `username` | string | no | `AWS` | Login username. |
| `passwordCmd` | array of strings | yes | | argv executed and piped to `container registry login --password-stdin`. |

`passwordCmd` is serde-renamed from `password_cmd`; the manifest spelling is
`passwordCmd`. Credentials are never stored in the manifest, which suits tokens
that expire (ECR tokens last 12 hours, so this re-runs on every start).

```json
"registries": [
  {
    "server": "{{account}}.dkr.ecr.{{region}}.amazonaws.com",
    "username": "AWS",
    "passwordCmd": ["aws", "ecr", "get-login-password", "--region", "{{region}}"]
  }
]
```

### builds

An array of image builds, run by `ac <project> build [name...]`.

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `name` | string | yes | | Build name, used on the command line. Must be unique. |
| `dockerfile` | string | yes | | Path relative to the resolved build root. |
| `context` | string | no | `.` | Build context, relative to the build root. |
| `image` | string | yes | | Image repository. Supports `{{...}}`, typically `{{registry}}my-app`. |
| `tags` | array of strings | no | `[]` | Tags appended to `image` as `<image>:<tag>`. Each supports `{{...}}`. Empty entries are dropped, and a build that resolves to no tags at all is an error at build time. |
| `target` | string | no | unset | Dockerfile stage to stop at. |
| `platform` | string | no | unset | Platform, when it differs from the profile's. |
| `buildArgs` | object of scalars | no | `{}` | Build arguments. Values may be string, number or boolean. Supports `{{...}}`. |
| `labels` | object of scalars | no | `{}` | Image labels. Supports `{{...}}`. |
| `secrets` | array of objects | no | `[]` | Build secrets. See below. |
| `preflight` | array of argv arrays | no | `[]` | Run from the build root before building. A failure aborts the build. |
| `postPush` | array of argv arrays | no | `[]` | Run from the build root after a successful push. A failure aborts and is reported as an error. |

`buildArgs` and `postPush` are the manifest spellings (renamed from
`build_args` and `post_push` in the Rust types).

`secrets[]`:

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `id` | string | yes | | Secret id, as referenced by `--mount=type=secret,id=...` in the Dockerfile. |
| `env` | string | no | unset | Host environment variable to read the secret from. |
| `src` | string | no | unset | Host file to read the secret from. |

```json
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
]
```

### services

An array of containers this project runs. They start in **array order**, each
gated on the previous one's `readyCmd`.

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `name` | string | yes | | Service name. The container is `<project>-<name>`. Must be unique. |
| `image` | string | yes | | Full OCI reference including the registry host, for example `docker.io/library/postgres:16-alpine`. |
| `cpus` | integer (min 1) | no | unset | Sizes the container's **VM**, not a cgroup. Each container is its own VM. |
| `memory` | string | no | unset | Memory for the container's VM, for example `1g`. |
| `ports` | array of strings | no | `[]` | `host:container`, same as Docker. Optional: every container also gets its own routable `192.168.64.x` address. |
| `env` | object of scalars | no | `{}` | Environment variables. Values may be string, number or boolean. |
| `volumes` | array of objects | no | `[]` | Named volumes. See [services.volumes](#servicesvolumes). |
| `args` | array of strings | no | `[]` | Extra argv appended after the image reference. |
| `readyCmd` | array of strings | no | `[]` | Polled with `container exec` until it exits 0. Apple `container` has no healthcheck primitive, so readiness is implemented by `ac`. Empty means `start` does not wait at all, and `ac <project> wait` treats the service as ready as soon as its container state is `running`. |
| `readyTimeout` | integer (seconds) | no | `90` | Seconds before giving up. `start` warns and continues anyway; `ac <project> wait` exits non-zero. |

`readyCmd` and `readyTimeout` are the manifest spellings (renamed from
`ready_cmd` and `ready_timeout`).

### services.volumes

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `name` | string | yes | | Logical volume name. The real volume is `<project>-<name>`, created on demand by `start`. |
| `target` | string | yes | | Mount point inside the container. |

Apple container volumes are real ext4 block devices, so a fresh one already
contains `lost+found`. Postgres refuses to initialise into a non-empty
directory, hence `PGDATA` pointing at a subdirectory in the worked example
below.

Volumes and their data survive `ac <project> down`, `rm` and
`start --recreate`. Only `ac <project> down -v` deletes them.

### scripts

A map of name to command, npm run style. `ac <project> <name> [args...]` hands
the string to `sh -c` with the extra arguments appended shell-quoted, and
propagates the exit code. `ac` never interprets the string: the script owns its
own subcommands, which is how project-specific tooling (ssh tunnels,
port-forwards, db consoles) sits behind `ac` without `ac` learning about it.

A value is either a **shell string** or an **object**:

| Field | Type | Required | Default | What it does |
| --- | --- | --- | --- | --- |
| `run` | string | yes | | The shell string handed to `sh -c`. |
| `complete` | array of strings | no | `[]` | Words TAB offers for the script's arguments, at every argument position. |

Anything that is neither a string nor an object is rejected with "a script must
be a shell string or an object with `run` and `complete`".

`ac` deliberately never executes a script to complete it (a completer that runs
user code could hang the shell or dial out), so the words are static data in
the manifest.

The script process sees:

| Variable | Value |
| --- | --- |
| `AC_PROJECT` | the project name |
| `AC_PROJECT_FILE` | the absolute path of the manifest |
| `AC_PROJECT_ROOT` | the manifest's `root`, only when `root` is set |

Name collisions are rejected at load time: a script may not be named after any
project action (`start`, `up`, `stop`, `down`, `restart`, `ls`, `ps`, `status`,
`logs`, `run`, `create`, `top`, `wait`, `push`, `export`, `exec`, `sh`, `shell`,
`stats`, `inspect`, `kill`, `rm`, `cp`, `pull`, `port`, `ip`, `env`, `build`,
`rollout`, `login`, `config`, `services`, `builds`, `profiles`, `images`,
`volumes`, `scripts`, `help`). The list lives in `src/cli/reserved.rs` as
`PROJECT_ACTIONS` and a test keeps it in step with the CLI.

```console
$ ac shop scripts
NAME     RUNS
forward  ~/.config/ac/scripts/shop-tunnels.sh
psql     psql -h 127.0.0.1 -p 5433 -U user postgres

$ ac shop forward status
$ ac shop psql -c 'select 1'
```

`ac --json <project> scripts` emits the map as written, so a string entry stays
a string and an object entry keeps its `run` and `complete`:

```json
{
  "forward": { "run": "~/.config/ac/scripts/shop-tunnels.sh", "complete": ["up", "status", "stop"] },
  "psql": "psql -h 127.0.0.1 -p 5433 -U user postgres"
}
```

## Interpolation

`{{...}}` placeholders are expanded in `image`, `tags`, `buildArgs`, `labels`,
hook arguments, and registry `server` and `passwordCmd`.

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

Details and precedence are in [Builds and rollouts](builds.md).

## Worked manifest

```json
{
  "name": "shop",
  "description": "shop local backing services",
  "root": "/Users/me/code/shop",
  "region": "us-east-1",

  "builder": { "cpus": 8, "memory": "8g" },

  "profiles": {
    "local": { "platform": "linux/arm64", "push": false, "tag": "dev-local", "registry": "" },
    "prod": {
      "platform": "linux/amd64",
      "push": true,
      "account": "123456789012",
      "tag": "latest",
      "registry": "{{account}}.dkr.ecr.{{region}}.amazonaws.com/",
      "rollout": {
        "description": "restart the app deployments",
        "preflight": [["./extras/ac-scripts/preflight.sh", "api"]],
        "run": [["./extras/ac-scripts/rollout.sh", "api"]],
        "auto": false
      }
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
    "forward": {
      "run": "~/.config/ac/scripts/shop-tunnels.sh",
      "complete": ["up", "restart", "stop", "status", "logs"]
    },
    "psql": "psql -h 127.0.0.1 -p 5433 -U user postgres"
  }
}
```

## Adding a project

No code changes are needed. Discovery is a directory listing.

```bash
# 1. write the file, starting from ac schema or the example above
$EDITOR ~/.config/ac/projects/shop.json

# 2. check it is discovered and parses
ac ls
ac shop config
ac shop images

# 3. bring it up and watch what it runs underneath
ac shop start
ac shop ls
```

An unknown field is an error naming the field, so typos surface at step 2.
`ac shop config` prints the manifest exactly as written (`ac --json shop config`
reparses it as JSON), and `ac shop images` resolves the interpolated image
references, so both are cheap sanity checks that need no daemon.

Put a manifest in `<ac home>/projects/` instead when it should ship with the
tool; a user file of the same name still wins.

When a project name collides with one of `ac`'s own commands (see `RESERVED` in
`src/cli/reserved.rs`), reach it with the escape hatch `ac -p <project>
<action>`. Shell completion skips such names because `ac <that-name>` would
dispatch to the command, not the project.

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [Project commands](project-commands.md): every action that reads this file.
- [Builds](builds.md) and [Rollouts](rollouts.md): the `builds`, `profiles` and
  `rollout` blocks in use.
- [Shell completion](completions.md): how manifest names and script `complete`
  words reach TAB.
