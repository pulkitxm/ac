# Builds

`ac <project> build` turns the `builds` array of a project manifest into
`container build` invocations, and `ac <project> push` pushes the tags those
builds would produce without building anything. Both are project commands, so
they need a manifest; the manifest-free `ac build` is a thin pass to
`container build` with no profiles, no interpolation and no rollout.

See [Manifest schema](manifest.md) for the `builds`, `profiles`, `registries`
and `builder` blocks these commands read, and [Rollouts](rollouts.md) for what
`--rollout` runs.

## ac \<project\> build

```text
ac <project> build [flags] [names...]
```

`names` are build names from the manifest's `builds[].name`. Empty means every
build the manifest declares. An unknown name is an error that lists the valid
ones, and a project with no `builds` is an error rather than a silent no-op.
`ac <project> builds` prints the accepted names, `ac <project> profiles` prints
the accepted `--profile` values, and both read only the manifest so they work
with the daemon stopped.

| Flag | Default | What it does |
| --- | --- | --- |
| `-P`, `--profile <NAME>` | `$AC_PROFILE`, then `local` | Profile whose platform, tag, account, registry, push and rollout settings apply. An unknown profile is an error listing the declared ones. |
| `--root <PATH>` | (see below) | Build from this tree, overriding every other root rule. A path that does not exist is an error. |
| `--platform <PLATFORM>` | profile, then build entry, then `linux/arm64` | Target platform, for example `linux/amd64`. Passed as `--platform`. |
| `--push` | profile's `push` | Push the resulting tags whatever the profile says. |
| `--no-push` | profile's `push` | Never push, whatever the profile says. `--push` and `--no-push` override each other, so the last one on the command line takes effect. |
| `--no-cache` | off | Ignore the layer cache. Also turned on by setting the `NO_CACHE` environment variable to anything. |
| `--progress <auto\|plain\|tty>` | unset, which behaves as `auto` | Build output style. Only these three values parse. See [Progress modes](#progress-modes). |
| `--target <STAGE>` | build entry's `target` | Dockerfile stage to stop at. |
| `--builder-cpus <N>` | manifest `builder.cpus` | CPUs for the shared buildkit builder. Changing it stops and recreates the builder, discarding its cache. |
| `--builder-memory <SIZE>` | manifest `builder.memory` | Memory for the shared builder, for example `8g`. Same caveat as `--builder-cpus`. |
| `--sequential` | off | Build one image at a time instead of in parallel. |
| `--rollout` | profile's `rollout.auto` | Run the profile's rollout after every build and push succeeds. The rollout's own preflight hooks run first, before anything is built. |
| `--no-rollout` | profile's `rollout.auto` | Never roll out, even when the profile sets `rollout.auto`. Overrides `--rollout`. |
| `--dry-run` | off | Resolve and print what would be built, running nothing. No daemon, no builder, no registry login. |

```bash
ac shop build
ac shop build api --profile prod
ac shop build --platform linux/amd64 --no-cache --sequential
ac shop build --dry-run --json
ac shop build --rollout -P prod
```

`--rollout` against a profile that resolves to `push: false` is an error:
nothing would reach the registry for the rollout to pick up. `--rollout` also
requires the profile to declare a `rollout` block, and the error names the
profiles that do.

### What one build runs

Per build, in order:

1. The build's `preflight` hooks, argv arrays run from the resolved root. A
   failure aborts that build immediately.
2. `container build` with the resolved arguments (see below).
3. When pushing, `container image push <tag>` once per tag.
4. When pushing, the build's `postPush` hooks. They never run after a failed
   push.

The `container build` argv is assembled in this order: `--platform`, `-f
<absolute dockerfile>`, `--progress` (when ac is driving it), `--target`,
`--no-cache`, `--cpus`, `--memory`, one `--build-arg K=V` per `buildArgs`
entry, one `--secret id=<id>[,env=VAR][,src=PATH]` per `secrets` entry, one
`--label K=V` per `labels` entry, one `-t <ref>` per resolved tag, then the
absolute build context. Apple `container build` has no `--cache-from`, so
neither does ac.

A build whose `tags` list is empty is an error. So is a tag template that
renders to nothing or to something starting with `-`, which is what happens
when `{{git.*}}` placeholders are empty because the build root is not a git
repository; the error says exactly that.

## Precedence

Every setting resolves through four levels, highest first:

1. CLI flag (`--platform`, `--target`, `--builder-cpus`, `--builder-memory`,
   `--push` / `--no-push`, ...)
2. profile (`profiles.<name>`)
3. build entry (`builds[]`)
4. project default (`builder`, `region`) or ac's own fallback

Not every setting exists at every level, so the practical ladders are:

| Setting | Ladder |
| --- | --- |
| platform | `--platform` > `profiles.<p>.platform` > `builds[].platform` > `linux/arm64` |
| push | `--push` / `--no-push` > `profiles.<p>.push` > `false` |
| target | `--target` > `builds[].target` > none |
| builder cpus | `--builder-cpus` > `builder.cpus` > builder left as is |
| builder memory | `--builder-memory` > `builder.memory` > builder left as is |
| region | `profiles.<p>.region` > `region` > `us-east-1` |
| profile | `--profile` > `$AC_PROFILE` > `local` |
| rollout | `--rollout` / `--no-rollout` > `profiles.<p>.rollout.auto` > off |

## Build root resolution

The resolved root is where hooks run, where relative `dockerfile` and `context`
paths are resolved, and where git and `package.json` are read for
interpolation. It is printed as `build root: ...` before anything runs (under
`--dry-run` it is a `root` field of the plan instead).

Rules, highest priority first, from `src/build/vars.rs`:

1. `--root <path>`.
2. `$AC_ROOT`, when non-empty.
3. When `$PWD` is inside a git worktree: that worktree's top level, if it
   contains the manifest's **first** declared `dockerfile`. When the manifest
   declares no builds (or the first has an empty dockerfile), the top level is
   used if its directory name matches the basename of the manifest's `root`.
4. When `$PWD` is not in a git repo at all: `$PWD`, if the manifest declares at
   least one build and `$PWD` contains **every** dockerfile it declares.
5. `root` from the manifest. A path that does not exist produces a warning and
   falls through.
6. `$PWD`.

Rule 3 is why git worktrees work. Running a build from inside a worktree builds
*that* tree rather than the path baked into the manifest, so one manifest
serves every worktree. Requiring the dockerfile to actually be present is what
stops an unrelated repo hijacking the build.

```console
$ cd ~/code/shop-feature-branch && ac shop build api
==> build root: /Users/me/code/shop-feature-branch
```

## Interpolation

`{{...}}` placeholders are expanded in `image`, each entry of `tags`, each
`buildArgs` entry, each `labels` entry, every hook argument (`preflight`,
`postPush`, and the rollout hooks), and a registry's `server` and
`passwordCmd`.

| Placeholder | Value |
| --- | --- |
| `{{profile}}` | the profile name being built |
| `{{account}}` | `profiles.<p>.account`, empty when unset |
| `{{tag}}` | `profiles.<p>.tag`, empty when unset |
| `{{region}}` | `profiles.<p>.region`, then `region`, then `us-east-1` |
| `{{registry}}` | `profiles.<p>.registry`, itself expanded for `{{account}}` and `{{region}}` |
| `{{version}}` | `version` from `package.json` at the build root, else `0.0.0` |
| `{{git.sha}}` | full HEAD sha, empty outside a git repo |
| `{{git.shortSha}}` | short HEAD sha |
| `{{git.branch}}` | current branch |
| `{{git.dirtySuffix}}` | `-local-<timestamp>` when `git status --porcelain` is non-empty, else empty |
| `{{timestamp}}` | `YYYYMMDDHHMMSS`, fixed once per build run |
| `{{image.<build>}}` | the first resolved tag of that build, for every build that has at least one non-empty tag. Intended for hooks, though it is substituted anywhere interpolation runs |

The final reference is `<interpolated image>:<interpolated tag>`, so
`"image": "{{registry}}shop-api"` with `"tags": ["{{tag}}"]` yields
`shop-api:dev-local` locally and
`123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-api:latest` under a prod
profile whose `registry` ends in a slash.

`{{git.dirtySuffix}}` exists so a dirty local tree can never overwrite the image
CI built for that commit.

## Registry login

Login is **filtered**: a registry is contacted only when an image in this run
actually comes from it. That is what stops `ac shop build` authenticating to
ECR when the only thing it needs is docker.io.

- On `build`, login runs only when the run resolves to pushing, against the
  images being built.
- On `push`, login always runs, against the images whose tags are being pushed.
- `ac <project> login` passes no image filter, so every declared registry is
  used.

A registry entry is skipped when its interpolated `server` is empty, starts
with `.`, or still contains `{{` (an unresolved placeholder). Each surviving
entry runs its `passwordCmd` and pipes the output to
`container registry login --username <username> --password-stdin <server>`. A
failure warns and continues rather than aborting, on the grounds that the pull
or push will fail with a clearer message. `username` defaults to `AWS`.

## Parallel and sequential

Multiple builds run in parallel by default. ac says so:

```console
$ ac shop build
==> building 3 images in parallel (--sequential to disable)
```

Parallelism applies when there is more than one build, `--sequential` is not
set, and the output mode is not the inherited buildkit display. `--sequential`
forces one at a time, and is also what makes `--progress tty` usable with more
than one build.

## Progress modes

`--progress` picks between three renderers. What ac passes down to
`container build` differs per mode, because ac has to parse the buildkit plain
stream to drive its own display.

| Mode | When it is chosen | What it prints | What `container build` gets |
| --- | --- | --- | --- |
| live (`auto`, the default) | `--progress auto` or omitted, and colour is on (a TTY, no `NO_COLOR`, no `--no-color`, no `--json`) | One live line per build, refreshed on a 100ms ticker: `[i/n] <instruction>  <step time> \| total <total>`, or `<phase>  <total>` between steps. Finished steps print as `+ [i/n] ... 3.4s`, cached ones as dim `- [i/n] ... cached`, failures as `x ... <error>`. | `--progress plain` |
| stream (`plain`) | `--progress plain`, or `auto` with colour off (piped output, `--json`, `NO_COLOR`, `--no-color`) | Raw buildkit lines, each prefixed with the build name padded to the widest name and a `\|` separator. | `--progress plain` |
| inherit (`tty`) | `--progress tty` **and** either a single build or `--sequential` | Nothing of ac's own: stdio is inherited and buildkit draws its own display. | `--progress tty` |

`--progress tty` with several builds and no `--sequential` falls back to the
live or stream renderer, because two buildkit displays cannot share a terminal.

In the live mode ac keeps the last 200 raw output lines per build and replays
the last 40 of them when that build fails, so a failure is diagnosable without
re-running with `--progress plain`.

Every underlying command is still echoed to stderr as `$ container build ...`
before it runs, unless `--quiet` or `AC_QUIET=1`.

## The summary

Every build run ends with a table:

```console
$ ac shop build
BUILD  STATUS  TIME    STEPS     TAGS
api    ok      1m04s   12 (7c)   shop-api:dev-local
web    ok      41.2s   9 (9c)    shop-web:dev-local
ok all builds finished
```

`STEPS` is `<done> (<cached>c)`, or `-` when no step was observed (which is
what the inherited `tty` mode always reports, since ac never parsed the
stream). Times under a minute print as `12.3s`, longer ones as `1m04s`.

If any build failed, the table still prints, the failing names are listed, and
the command exits non-zero.

Under `--json` the table is replaced by an array, one object per build:

```json
[
  {
    "build": "api",
    "ok": true,
    "seconds": 64.3,
    "steps": { "done": 12, "cached": 7 },
    "tags": ["shop-api:dev-local"],
    "pushed": false,
    "error": null
  }
]
```

`error` is the failure message string when `ok` is false, and `null` otherwise.
`--json` implies `--quiet` and moves human log lines to stderr, so stdout stays
a single parseable document.

## Dry run

`--dry-run` resolves everything and prints it without touching the daemon, the
builder or any registry:

```console
$ ac shop build api --profile prod --dry-run
api
  profile     prod
  root        /Users/me/code/shop
  dockerfile  apps/api/Dockerfile
  platform    linux/amd64
  tag         123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-api:latest
  push        true
  $ container build --platform linux/amd64 -f /Users/me/code/shop/apps/api/Dockerfile -t ... /Users/me/code/shop

dry run, nothing was built or pushed
```

With `--json` the same data comes back as an array:

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
    "command": ["build", "--platform", "linux/amd64", "-f", "...", "-t", "...", "..."]
  }
]
```

`command` is the argv passed to `container`, without the leading `container`.
When a rollout is also requested, the resolved rollout hooks and their
environment are printed after the builds; see [Rollouts](rollouts.md).

## Builder sizing

`--builder-cpus` and `--builder-memory` (or `builder.cpus` and
`builder.memory` in the manifest) size the shared buildkit builder container.
Apple `container` reads those values **only when the builder is created**, so
passing them to a build while the builder is already running is silently
ignored.

ac therefore reads `container builder status --format json`, compares the
running builder's cpus and memory against what is wanted, and when they differ
warns loudly and runs `container builder stop` before building. That discards
the builder's layer cache, so the next build is a cold one.

```console
$ ac shop build --builder-memory 12g
warn resizing buildkit builder from 8 cpus / 8192 MB to unchanged cpus / 12288 MB.
     The builder only reads these values when it is created, so it is being
     stopped first and its layer cache is discarded.
```

Memory is parsed with `g`/`gb` (binary, so `8g` is 8192 MB) and `m`/`mb`
suffixes; a bare number is megabytes. Nothing happens at all when neither cpus
nor memory is specified: the builder is left exactly as it is. Manage it
directly with `ac builder <status|start|stop|delete>`.

## ac \<project\> push

```text
ac <project> push [-P profile] [names...]
```

Resolves the same tags `build` would produce for the profile, ensures the
daemon, logs in to the registries those images come from, then runs
`container image push` once per tag. Nothing is built, and `postPush` hooks do
**not** run (they belong to a build). Use [`ac <project> rollout`](rollouts.md)
to deploy what is already pushed.

| Flag | Default | What it does |
| --- | --- | --- |
| `-P`, `--profile <NAME>` | `$AC_PROFILE`, then `local` | Profile whose registry, account and tag template resolve the tags. An unknown profile is an error listing the declared ones. |

Positional `names` are build names; empty means every build. Note that `push`
takes no `--root`: the build root still resolves through the rules above, minus
rule 1, which matters because git placeholders in tag templates are read from
it.

```bash
ac shop push --profile pre-prod
ac shop push api -P prod
ac --json shop push -P prod
```

Failures are collected rather than aborting on the first one; the command exits
non-zero at the end and names the builds that failed. Under `--json` stdout is
an array, one object per build:

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

`tags` is everything that was attempted, `pushed` is the subset that succeeded,
so a partial failure is visible per tag.

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [Project commands](project-commands.md) for the rest of `ac <project> ...`.
- [Rollouts](rollouts.md) for `--rollout`, `--no-rollout` and
  `ac <project> rollout`.
- [Manifest schema](manifest.md) for `builds`, `profiles`, `registries` and
  `builder`.
- [`ac build`](images-and-registries.md) (no project) for a one-off
  `container build` with no profiles,
  interpolation or rollout. Its own flag set is `-t/--tag`, `-f/--file`,
  `--target`, `--platform`, `-a/--arch`, `--os`, `--build-arg`, `-l/--label`,
  `--secret`, `--no-cache`, `--pull`, `--progress`, `-o/--output`, `-c/--cpus`,
  `-m/--memory`, `-q/--build-quiet`, and a positional build context defaulting
  to `.`; note that `-q` there means "suppress build output", not the global
  `--quiet`.
