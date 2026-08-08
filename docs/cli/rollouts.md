# Rollouts

`ac` has no deployment logic and no Kubernetes awareness. A build profile
declares hooks, `ac` runs them and hands them the image references it resolved.
That is what turns build, push and ship into one command without teaching `ac`
about anyone's cluster.

Two commands drive it:

| Command | What it does |
| --- | --- |
| `ac <project> rollout` | Runs the profile's rollout hooks against images that are already pushed. Nothing is built. |
| `ac <project> build --rollout` | Builds, pushes, then runs the same hooks. See [Builds](builds.md). |

Both are project-scoped, so they need a manifest. See
[Project commands](project-commands.md) for the rest of the project verbs and
[Manifest reference](manifest.md) for the surrounding schema.

## The rollout block

A rollout hangs off a **profile**, not off the project. A profile with no
`rollout` key can never deploy, which is what keeps a `local` profile safe.

```json
{
  "name": "shop",
  "region": "us-east-1",
  "profiles": {
    "local": { "platform": "linux/arm64", "push": false, "tag": "dev-local", "registry": "" },
    "prod": {
      "platform": "linux/amd64",
      "push": true,
      "account": "123456789012",
      "tag": "latest",
      "registry": "{{account}}.dkr.ecr.{{region}}.amazonaws.com/",
      "rollout": {
        "description": "restart the app deployments and pin the workers",
        "preflight": [["./extras/ac-scripts/preflight.sh", "app", "workers"]],
        "run": [["./extras/ac-scripts/rollout.sh", "{{profile}}", "{{image.api}}"]],
        "auto": false
      }
    }
  },
  "builds": [
    {
      "name": "api",
      "dockerfile": "apps/api/Dockerfile",
      "context": ".",
      "image": "{{registry}}shop-api",
      "tags": ["{{tag}}", "{{version}}-{{git.shortSha}}{{git.dirtySuffix}}"]
    }
  ]
}
```

| Key | Type | Default | What it does |
| --- | --- | --- | --- |
| `description` | string | none | Free text. Printed by `ac <project> rollout --dry-run`. |
| `preflight` | array of argv arrays | `[]` | Hooks run **before anything is built**. |
| `run` | array of argv arrays | `[]` | Hooks run **after every build and push in the invocation succeeded**. |
| `auto` | boolean | `false` | Roll out on every `ac <project> build` for this profile, with no `--rollout`. `--no-rollout` still wins. |

Unknown keys inside `rollout` are rejected, as everywhere else in the manifest.
Empty argv arrays are skipped rather than run.

### Why the split into two lists

`preflight` runs before the daemon is ensured, before the shared builder is
sized, and before any registry login. An unreachable cluster or an expired
token therefore fails in seconds instead of after a ten minute build. That is
the entire reason the hook list is split in two.

`run` fires only once every build and every push in the invocation succeeded. A
non-zero exit from either list aborts the command and propagates the failure,
so a failed preflight means nothing was built and a failed build means the
rollout never runs.

### Blast radius is per profile

Because the block lives on the profile, `prod` may restart everything while
`pre-prod` touches only the pre-prod deployments, from the same manifest and
the same hook scripts. Choosing the profile is choosing the blast radius.

## `ac <project> rollout`

Rolls out images that are already in the registry, without rebuilding. Use it
to re-run a rollout that failed after a successful push, or to deploy an image
someone else built.

```text
ac <project> rollout [-P profile] [--root PATH] [--dry-run] [name...]
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-P`, `--profile <NAME>` | `$AC_PROFILE`, then `local` | Profile whose rollout to run. |
| `--root <PATH>` | build root resolution | Roll out from this tree. Overrides every other root rule, including `$AC_ROOT` and the manifest's `root`. |
| `--dry-run` | off | Resolve and print the hooks and their environment, running nothing. |
| `[name...]` | every declared build | Builds whose images this rollout covers. Sets `AC_BUILDS` and `AC_IMAGES`. |

The trailing `name...` arguments are build names from the manifest, not service
names. An unknown name is an error that lists the valid ones.

```console
$ ac shop rollout --profile prod
$ git rev-parse --show-toplevel
rollout profile: prod
build root: /Users/me/code/shop
rollout.preflight
rollout
rollout finished
```

```bash
ac shop rollout -P prod                 # every declared build
ac shop rollout api -P pre-prod         # only the api image
ac shop rollout -P prod --root ~/wt/hotfix
```

Notes on behaviour, from `src/build/rollout.rs`:

- The profile must exist. An unknown profile is an error listing the profiles
  the manifest declares.
- The profile must declare a `rollout`. If it does not, the error lists the
  profiles that do, or tells you to add the block when no profile has one.
- `ac <project> rollout` never starts the container daemon and never contacts a
  registry. It only runs your hooks.
- Unlike `build --rollout`, this command does **not** require the profile to
  push. It assumes the images are already where the hooks expect them.
- On success it prints `rollout finished` and exits 0. Any hook exiting
  non-zero aborts immediately with the failing argv in the message, and `ac`
  exits non-zero.

## `ac <project> build --rollout`

Same hooks, wrapped around a build.

| Flag | Default | What it does |
| --- | --- | --- |
| `--rollout` | neither flag: the profile's `rollout.auto` decides | Run the profile's rollout after every build and push succeeds. |
| `--no-rollout` | neither flag: the profile's `rollout.auto` decides | Never roll out, even when the profile sets `auto: true`. |

The two are a tri-state (`BuildArgs::rollout_override` in
`src/cli/project.rs`): neither flag means the profile's `auto` decides, and
the flags override each other so the last one on the command line wins.

Order of operations for `ac shop build --rollout -P prod`:

1. Resolve the profile, the target builds and the build root.
2. Decide whether a rollout is wanted, and fail early if it is impossible.
3. Run `rollout.preflight`.
4. Ensure the daemon, size the builder, log in to the registries the images
   come from.
5. Run each build's own `preflight`, `container build`, the pushes, and each
   build's `postPush`.
6. Print the build summary table (or the JSON array under `--json`).
7. Run `rollout.run`, then print `rollout finished`.

```bash
ac shop build --rollout -P prod        # build, push, then roll out
ac shop build --no-rollout -P prod     # never roll out, even with auto: true
ac shop build api --rollout -P prod    # one build; AC_BUILDS is "api"
```

### `--rollout` against a non-pushing profile is an error

If the resolved push setting is false (from `--no-push`, or from the profile's
`push`), nothing would reach the registry for the rollout to pick up, so `ac`
refuses before doing any work:

```console
$ ac shop build --rollout -P local
error: --rollout needs a profile that pushes, but 'local' resolves to push=false, so nothing would reach the registry for the rollout to pick up
```

`--rollout` against a profile that declares no `rollout` block is likewise an
error, and it names the profiles that do declare one.

## Dry run

`--dry-run` resolves everything (profile values, git and version variables,
image references, hook argv, hook environment) and prints it without running a
thing. `ac <project> build --dry-run --rollout` prints the build plans first
and then the same rollout block.

```console
$ ac shop rollout -P prod --dry-run
prod
  restart the app deployments and pin the workers
  root        /Users/me/code/shop
  builds      api web
  $ preflight: /Users/me/code/shop/extras/ac-scripts/preflight.sh app workers
  $ rollout: /Users/me/code/shop/extras/ac-scripts/rollout.sh prod 123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-api:latest
dry run, nothing was rolled out
```

With `--json` the same information is one object on stdout:

```json
{
  "profile": "prod",
  "root": "/Users/me/code/shop",
  "builds": ["api", "web"],
  "preflight": [["/Users/me/code/shop/extras/ac-scripts/preflight.sh", "app", "workers"]],
  "run": [["/Users/me/code/shop/extras/ac-scripts/rollout.sh", "prod", "123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-api:latest"]],
  "env": { "AC_PROJECT": "shop", "AC_PROFILE": "prod", "AC_IMAGE_API": "...", "...": "..." }
}
```

`preflight` and `run` are the fully interpolated argv, with the program path
already made absolute. `env` is a flat object of the exact variables the hooks
would see. `--json` implies `--quiet` and moves ac's human log lines to stderr,
so stdout stays a single parseable document.

## How hooks are executed

- A hook is **argv**, an array of strings, never a shell string. There is no
  shell in between, so no quoting or globbing happens on ac's side. Use
  `["sh", "-c", "..."]` when you want a shell.
- The working directory is the **resolved build root**, the same root a build
  would use, printed as `build root: ...` before the hooks run.
- Every argument goes through `{{...}}` interpolation.
- `argv[0]` containing a `/` is resolved to an absolute path against the build
  root, so `./extras/ac-scripts/rollout.sh` works regardless of where you
  invoked `ac` from. A bare program name (`kubectl`, `aws`) is left alone and
  found on `PATH`.
- Hooks inherit stdio, so their output goes straight to your terminal rather
  than being captured into a build line.
- The first hook that exits non-zero aborts the command; later hooks in the
  same list do not run, and neither does the other list.

## Interpolation available to hooks

Everything from the build variable table, plus one placeholder that only makes
sense here:

| Placeholder | Value |
| --- | --- |
| `{{image.<build>}}` | the **primary** tag of that build, i.e. its first declared tag, fully qualified with the registry |
| `{{profile}}` | the profile name |
| `{{account}}` | `profiles.<p>.account` |
| `{{tag}}` | `profiles.<p>.tag` |
| `{{region}}` | `profiles.<p>.region`, then `.region`, then `us-east-1` |
| `{{registry}}` | `profiles.<p>.registry`, itself interpolated |
| `{{version}}` | `version` from `package.json` at the build root, else `0.0.0` |
| `{{git.sha}}` | full HEAD sha |
| `{{git.shortSha}}` | short HEAD sha |
| `{{git.branch}}` | current branch |
| `{{git.dirtySuffix}}` | `-local-<timestamp>` when the tree is dirty, else empty |
| `{{timestamp}}` | `YYYYMMDDHHMMSS`, fixed once per run |

`{{image.<build>}}` is defined for every build the manifest declares that has
at least one non-empty tag, not only the builds in this run. See the sharp edge
below.

## Environment handed to hooks

This is the interface the scripts actually use. Every variable below is set on
both `preflight` and `run` hooks, for both `ac <project> rollout` and
`ac <project> build --rollout`.

| Variable | Value |
| --- | --- |
| `AC_PROJECT` | project name |
| `AC_PROFILE` | resolved profile name |
| `AC_ACCOUNT` | `profiles.<p>.account`, empty when unset |
| `AC_REGION` | resolved region (`profiles.<p>.region`, then `.region`, then `us-east-1`) |
| `AC_REGISTRY` | resolved registry prefix, a host plus trailing slash, empty for local profiles |
| `AC_TAG` | `profiles.<p>.tag`, empty when unset |
| `AC_VERSION` | `version` from `package.json` at the build root, else `0.0.0` |
| `AC_ROOT` | the resolved build root, absolute |
| `AC_GIT_SHA` | full HEAD sha, empty outside a git tree |
| `AC_GIT_SHORT_SHA` | short HEAD sha, empty outside a git tree |
| `AC_GIT_BRANCH` | current branch, empty outside a git tree |
| `AC_GIT_DIRTY` | `1` when the tree is dirty, `0` otherwise |
| `AC_TIMESTAMP` | `YYYYMMDDHHMMSS`, fixed once per run |
| `AC_BUILDS` | build names in this run, space separated |
| `AC_IMAGES` | every tag of the builds **in this run**, space separated |
| `AC_IMAGE_<BUILD>` | that build's primary (first) tag |
| `AC_IMAGES_<BUILD>` | every tag for that build, space separated |
| `AC_QUIET` | `1` when `--quiet` or `AC_QUIET=1` is in effect, `0` otherwise |

`AC_PROFILE` and `AC_ROOT` are also **inputs**: `ac` reads `$AC_PROFILE` when
no `-P/--profile` is given, and `$AC_ROOT` as the second-highest priority build
root rule. Exporting them in a shell therefore changes what a later `ac`
invocation does.

### Name mangling

`<BUILD>` is the build name upper-cased with every non-alphanumeric character
replaced by `_`. A build called `api-workers` arrives as
`AC_IMAGE_API_WORKERS` and `AC_IMAGES_API_WORKERS`. A build called `web.v2`
arrives as `AC_IMAGE_WEB_V2`. Two build names that mangle to the same key would
collide, so keep them distinct.

A build that declares no tags gets no `AC_IMAGE_*` or `AC_IMAGES_*` variable at
all, since there is nothing to name.

### Sharp edge: `AC_IMAGE_*` covers every declared build

`AC_IMAGE_<BUILD>` and `AC_IMAGES_<BUILD>` are populated for **every** build the
manifest declares, not just the ones in this run. That is deliberate: a hook can
pin a service that was not rebuilt this time, which is what makes
`ac shop build api --rollout` able to deploy a manifest that also mentions
`web`.

It is also sharp. Nothing verifies that those other tags exist in the registry.
A hook that pins every `AC_IMAGE_*` it can see will happily pin a deployment to
an image nobody ever pushed. The two guards:

- `AC_BUILDS` tells the hook which builds this run actually covered.
- `AC_IMAGES` contains tags only for those builds.

So a careful hook iterates `AC_BUILDS`, or checks the registry before pinning
anything outside it.

## Worked hook script

`extras/ac-scripts/rollout.sh`, run from the build root, taking deployment
names as arguments:

```bash
#!/usr/bin/env bash
set -euo pipefail

echo "project   $AC_PROJECT"
echo "profile   $AC_PROFILE"
echo "root      $AC_ROOT"
echo "builds    $AC_BUILDS"
echo "images    $AC_IMAGES"

if [ "$AC_GIT_DIRTY" = "1" ]; then
  echo "refusing to roll out a dirty tree" >&2
  exit 1
fi

for build in $AC_BUILDS; do
  key="AC_IMAGE_$(printf '%s' "$build" | tr '[:lower:]' '[:upper:]' | tr -c 'A-Z0-9' '_')"
  image="${!key}"
  [ -n "$image" ] || { echo "no image for build $build" >&2; exit 1; }
  kubectl --context "$AC_PROFILE" set image "deploy/shop-$build" "$build=$image"
done

for dep in "$@"; do
  kubectl --context "$AC_PROFILE" rollout status "deploy/$dep" --timeout=5m
done
```

The matching preflight, which is the whole reason the split exists:

```bash
#!/usr/bin/env bash
set -euo pipefail

kubectl --context "$AC_PROFILE" cluster-info >/dev/null
aws sts get-caller-identity --query Account --output text | grep -qx "$AC_ACCOUNT"
for dep in "$@"; do
  kubectl --context "$AC_PROFILE" get "deploy/$dep" >/dev/null
done
```

Wire them up and the whole cycle is one command:

```console
$ ac shop build --rollout -P prod
build root: /Users/me/code/shop
rollout.preflight
building 2 images in parallel (--sequential to disable)
BUILD  STATUS  TIME   STEPS     TAGS
api    ok      1m12s  14 (9c)   123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-api:latest
web    ok        58s  11 (7c)   123456789012.dkr.ecr.us-east-1.amazonaws.com/shop-web:latest
all builds finished
rolling out profile 'prod'
rollout finished
```

## Exit behaviour

| Situation | Result |
| --- | --- |
| Unknown profile | error naming the profiles the manifest declares, non-zero |
| Profile declares no `rollout` (on `rollout`, or with `--rollout`) | error naming the profiles that do, non-zero |
| Unknown build name | error listing the valid build names, non-zero |
| `--rollout` with a resolved `push=false` | error, non-zero, nothing built |
| A `preflight` hook fails | abort before any build, non-zero |
| A build or push fails | `run` hooks never execute, non-zero |
| A `run` hook fails | abort at that hook, non-zero |
| Everything succeeds | `rollout finished`, exit 0 |
| `--dry-run` | exit 0, nothing executed |

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [Builds](builds.md): the build and push these hooks fire after.
- [The project manifest](manifest.md): where the `rollout` block sits in the
  schema.
- [Project commands](project-commands.md): the rest of `ac <project> ...`.
