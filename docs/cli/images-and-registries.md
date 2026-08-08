# Images and registries

Everything `ac` can do to images and registry credentials without a manifest
being involved. Two spellings reach the same code: the docker-style verbs
(`ac build`, `ac pull`, `ac push`, `ac tag`, `ac save`, `ac load`, `ac rmi`,
`ac images`, `ac login`, `ac logout`) and the noun groups (`ac image ...`,
`ac registry ...`). Each one is a thin pass to Apple's `container` binary, with
ac's daemon ownership contract, command echo and `--json` handling layered on.

For a project's declared builds, with profiles, interpolated tags, registry
login filtered to the images actually involved, and rollout hooks, see
[Project builds](builds.md). For containers see
[Container commands](containers.md), and for the daemon itself see
[Daemon and system](daemon-and-system.md).

Every underlying command is echoed to stderr, dimmed and prefixed with `$ `,
before it runs. Suppress with `--quiet` or `AC_QUIET=1`. `--json` implies
`--quiet` and moves human log lines to stderr so stdout stays one parseable
document.

## Daemon gating

The rule splits by whether the command reads or mutates.

| Commands | Gating |
| --- | --- |
| `ac image ls`, `ac image inspect`, `ac registry ls`, `ac save`, `ac logout` | Require a running daemon and fail with a hint. ac never starts a daemon for a read. |
| `ac build`, `ac pull`, `ac push`, `ac tag`, `ac load`, `ac login`, and every mutating `ac image ...` / `ac registry ...` subcommand | Start the daemon if it is not running, then re-check the refcount afterwards, so a daemon started for a one-off command is released again. |

The group forms that go through the shared passthrough (`ac image pull`,
`push`, `rm`, `tag`, `prune`, `save`, `load`, and `ac registry logout`) also
spawn the supervisor before running, which is a no-op unless ac owns the
daemon. The top-level verbs (`ac pull`, `ac push`, `ac tag`, `ac load`,
`ac login`) and `ac build` do not: they ensure the daemon and settle it again
without a watchdog, because they leave nothing running behind them.

There is one asymmetry worth knowing, and it is in the source rather than in
any help text: top-level `ac save` calls `daemon::require` and `ac logout`
calls `daemon::require` (`src/commands/docker/images.rs`), while the group
forms `ac image save` and `ac registry logout` go through the mutating
passthrough in `src/commands/groups.rs` and will start a daemon. If you want
"fail rather than start a daemon", use the top-level spelling.

## ac build

```text
ac build [flags] [CONTEXT]
```

Runs `container build [flags] <context>`. The context argument defaults to `.`.

**This is the plain docker-style build.** It has no profiles, no `{{...}}`
interpolation, no rollout hooks and no build-root resolution: the context is
whatever you pass (or the current directory), and the tags are exactly the
strings you type. For any of those, use `ac <project> build`, documented in
[Project builds](builds.md).

| Flag | Default | What it does |
| --- | --- | --- |
| `-t`, `--tag <NAME>` | none | Name for the built image. Repeatable; each becomes a `--tag`. |
| `-f`, `--file <PATH>` | container's default (`Dockerfile` in the context) | Path to the Dockerfile. |
| `--target <STAGE>` | none | Target build stage in a multi-stage Dockerfile. |
| `--platform <os/arch[/variant]>` | daemon default | Platform to build for. |
| `-a`, `--arch <ARCH>` | daemon default | Architecture to build for. `--platform` wins. |
| `--os <OS>` | daemon default | OS to build for. `--platform` wins. |
| `--build-arg <KEY=VALUE>` | none | Build-time variable. Repeatable. |
| `-l`, `--label <KEY=VALUE>` | none | Image label. Repeatable. |
| `--secret <SPEC>` | none | Build secret, `id=<key>[,env=VAR\|,src=PATH]`. Repeatable. |
| `--no-cache` | off | Do not use the layer cache. |
| `--pull` | off | Always attempt to pull a newer base image. |
| `--progress <auto\|plain\|tty>` | container's default | Progress output style, passed straight through. |
| `-o`, `--output <SPEC>` | none | Output configuration, `type=<oci\|tar\|local>[,dest=]`. |
| `-c`, `--cpus <N>` | builder's existing size | CPUs for the builder container. |
| `-m`, `--memory <SIZE>` | builder's existing size | Memory for the builder container, e.g. `8g`. |
| `-q`, `--build-quiet` | off | Suppress build output, docker `build -q` style. Passed as `--quiet` to `container build`. |
| `CONTEXT` | `.` | Build context directory. |

Note that `-q` here is `--build-quiet`, not ac's global `--quiet`. The global
flag has no short form precisely because `-q` means different things on
different commands.

`-c`/`-m` are handled before the build runs. If the builder container already
exists at a different size, ac stops and recreates it, because
`container builder` only reads cpu and memory at creation time. That discards
the layer cache and ac warns loudly when it happens. Passing neither flag
leaves the builder completely alone.

On success, and only when at least one `-t` was given, ac prints one
`built <tag>` line per tag plus a dimmed hint showing how to run the first tag.

```console
$ ac build -t my-app:dev .
$ ac build -t my-app:dev -f docker/Dockerfile --target runner .
$ ac build -t my-app:dev --platform linux/amd64 --no-cache --build-arg NODE_ENV=production .
$ ac build -t my-app:dev -c 8 -m 8g .
```

## ac image

```text
ac image [-v] [-q] [SUBCOMMAND]
ac images ...
```

`images` is a visible alias for `image`. With no subcommand this lists images,
so the old `ac images` keeps working, and the group-level `-v` and `-q` are
forwarded to `ls`.

### ac image ls

Aliases: `list`. Also reached as bare `ac image` or `ac images`.

| Flag | Default | What it does |
| --- | --- | --- |
| `-v`, `--verbose` | off | Pass straight through to `container image ls --verbose`, printing every platform variant as the runtime formats it. Hidden from `--help`, but real. |
| `-q` | off | Only print image names, docker `images -q` style. Runs `container image ls -q`. `-q` wins over `-v`. |

The default human output is ac's own table, built from
`container image ls --format json`: columns `NAME`, `TAG`, `ARCH`, `SIZE`
(right aligned), `CREATED`, sorted, one row per tag. Multi-platform images are
sized for the variant matching this machine's architecture, falling back to the
first variant, which is why the footer says every variant is behind
`ac image ls -v`.

`--json` emits the runtime's own document from `container image ls --format
json`, not ac's table shape. `-q --json` emits a flat array of name strings.

```console
$ ac image ls
$ ac images -q
$ ac image ls -v
$ ac --json image ls
```

### ac image pull

Runs `container image pull [--platform P] <reference>`. Same as top-level
`ac pull`.

| Argument / flag | Default | What it does |
| --- | --- | --- |
| `<REFERENCE>` | required | Full OCI reference, including the registry host. |
| `--platform <os/arch[/variant]>` | daemon default | Platform to pull. |

```console
$ ac image pull docker.io/library/alpine:3.20
$ ac pull docker.io/library/postgres:16-alpine --platform linux/arm64
```

### ac image push

Runs `container image push [--platform P] <reference>`. Same as top-level
`ac push`.

| Argument / flag | Default | What it does |
| --- | --- | --- |
| `<REFERENCE>` | required | Full OCI reference, including the registry host. |
| `--platform <os/arch[/variant]>` | all | Platform to push when the local image is multi-platform. |

```console
$ ac push 123456789012.dkr.ecr.us-east-1.amazonaws.com/my-app:dev
```

### ac image rm

Aliases: `delete`, `remove`. Also `ac rmi <references...>`, which is a hidden
top-level command mapping onto exactly this with `force` unset.

Runs `container image rm <references...>`. Volumes and containers are
untouched; to remove containers use `ac rm` (see
[Container commands](containers.md)).

| Argument / flag | Default | What it does |
| --- | --- | --- |
| `<REFERENCES...>` | required, at least one | Image references to remove. |
| `-f`, `--force` | off | Accepted for docker muscle memory and ignored; removal never prompts, and the flag is not forwarded. Hidden from `--help`. |

```console
$ ac image rm my-app:dev
$ ac rmi my-app:dev my-app:old
```

### ac image tag

Runs `container image tag <source> <target>`. Same as top-level `ac tag`.

| Argument | Default | What it does |
| --- | --- | --- |
| `<SOURCE>` | required | Existing reference. |
| `<TARGET>` | required | New reference to create. |

```console
$ ac image tag my-app:dev-local 123456789012.dkr.ecr.us-east-1.amazonaws.com/my-app:dev
```

### ac image inspect

Runs `container image inspect <references...>`. Output is pretty-printed JSON;
with `--json` the runtime's document is emitted verbatim on stdout. Requires a
running daemon.

| Argument | Default | What it does |
| --- | --- | --- |
| `<REFERENCES...>` | required, at least one | Image references to inspect. |

```console
$ ac image inspect my-app:dev
$ ac --json image inspect my-app:dev | jq '.[0].variants | length'
```

### ac image prune

Runs `container image prune [--all]`.

| Flag | Default | What it does |
| --- | --- | --- |
| `-a`, `--all` | off | Remove every unused image, not just dangling ones. |

```console
$ ac image prune
$ ac image prune --all
```

### ac image save

Runs `container image save -o <output> [--platform P] <references...>`.

| Argument / flag | Default | What it does |
| --- | --- | --- |
| `<REFERENCES...>` | required, at least one | Image references to save. |
| `-o`, `--output <PATH>` | required | Path for the OCI tar archive. |
| `--platform <os/arch[/variant]>` | all | Platform to save for multi-platform images. |

```console
$ ac image save -o backup.tar my-app:dev-local
$ ac image save -o two.tar --platform linux/arm64 my-app:dev my-app:old
```

### ac image load

Runs `container image load -i <input>`.

| Flag | Default | What it does |
| --- | --- | --- |
| `-i`, `--input <PATH>` | required | OCI tar archive to read. |

```console
$ ac image load -i backup.tar
```

## The top-level image verbs

These exist so docker muscle memory works. They are narrower than the group
forms: single reference, no `--platform` on save.

### ac pull

```text
ac pull <REFERENCE> [--platform <os/arch[/variant]>]
```

Same behaviour as `ac image pull`, one reference only.

```console
$ ac pull docker.io/library/redis:7-alpine
```

### ac push

```text
ac push <REFERENCE> [--platform <os/arch[/variant]>]
```

Same behaviour as `ac image push`, one reference only.

```console
$ ac push ghcr.io/me/my-app:latest
```

### ac tag

```text
ac tag <SOURCE> <TARGET>
```

Same as `ac image tag`.

```console
$ ac tag my-app:dev ghcr.io/me/my-app:latest
```

### ac save

```text
ac save <REFERENCE> -o <PATH>
```

| Argument / flag | Default | What it does |
| --- | --- | --- |
| `<REFERENCE>` | required | One image reference to save. |
| `-o`, `--output <PATH>` | required | Path for the tar archive. |

Runs `container image save -o <path> <reference>`. Unlike `ac image save` this
requires a daemon that is already running and will not start one, and takes no
`--platform`.

```console
$ ac save my-app:dev -o my-app.tar
```

### ac load

```text
ac load -i <PATH>
```

| Flag | Default | What it does |
| --- | --- | --- |
| `-i`, `--input <PATH>` | required | Archive to read. |

Runs `container image load -i <path>`.

```console
$ ac load -i my-app.tar
```

### ac images, ac rmi

`ac images` is the visible alias of `ac image` and accepts its `-v` and `-q`.
`ac rmi <references...>` is a hidden alias for `ac image rm` and requires at
least one reference.

```console
$ ac images
$ ac rmi my-app:dev
```

## Registries

Apple's `container registry login` has **no** `--password` flag; it prompts, or
reads `--password-stdin`. ac keeps that shape on `ac registry login` and adds a
convenience `--password` on the docker-style `ac login`, which it pipes into
`--password-stdin` itself.

### ac registry ls

Runs `container registry ls`, and `container registry ls --format json` under
`--json`. A plain read, so it requires a running daemon. Bare `ac registry`
with no subcommand does this.

```console
$ ac registry ls
$ ac --json registry ls
```

### ac registry login

Runs `container registry login [-u <user>] [--password-stdin] <server>`.

| Argument / flag | Default | What it does |
| --- | --- | --- |
| `<SERVER>` | required | Registry server host. |
| `-u`, `--username <NAME>` | none | Registry user name. Passed as `-u`. |
| `--password-stdin` | off | Read the password from stdin instead of prompting. |

On success this prints `authenticated to <server>`; on failure it returns an
error rather than a bare non-zero exit.

```console
$ ac registry login -u me ghcr.io
$ aws ecr get-login-password --region us-east-1 \
    | ac registry login -u AWS --password-stdin 123456789012.dkr.ecr.us-east-1.amazonaws.com
```

### ac registry logout

Runs `container registry logout <server>`.

| Argument | Default | What it does |
| --- | --- | --- |
| `<SERVER>` | required | Registry server host. |

```console
$ ac registry logout ghcr.io
```

### ac login

The docker-style spelling, with one extra flag.

| Argument / flag | Default | What it does |
| --- | --- | --- |
| `<SERVER>` | required | Registry host. |
| `-u`, `--username <NAME>` | none | Username. Passed as `--username`. |
| `-p`, `--password <PASS>` | none | Password. ac adds `--password-stdin` to the underlying command and writes the value (plus a newline) to the child's stdin, because `container registry login` accepts no `--password`. |
| `--password-stdin` | off | Read the password from stdin. |

Sharp edge: if both `-p` and `--password-stdin` are given, `--password-stdin`
wins and the `-p` value is ignored (`src/commands/docker/images.rs` filters the
password out when `password_stdin` is set), so the command will consume your
stdin instead. Pick one.

Passing a password on the command line puts it in your shell history and in the
process table. `--password-stdin` is the safer form.

```console
$ ac login ghcr.io -u me -p "$GITHUB_TOKEN"
$ printf '%s' "$GITHUB_TOKEN" | ac login ghcr.io -u me --password-stdin
```

### ac logout

```text
ac logout <SERVER>
```

Runs `container registry logout <server>`. Requires a running daemon and will
not start one.

```console
$ ac logout ghcr.io
```

## Project-scoped equivalents

When the images belong to a manifest, the project form does more work and is
usually what you want. `ac <project> login` runs each declared registry's
`passwordCmd` into `--password-stdin`; `ac <project> pull` pulls the services'
images after any needed login; `ac <project> images` lists the images the
manifest declares; `ac <project> push` pushes the tags a build would produce
without building them. Registry login there is filtered to the registries the
images actually come from, which is what stops a start logging in to ECR merely
to pull postgres from docker.io. See [Project commands](project-commands.md)
and [Project builds](builds.md).

## Exit behaviour

None of the commands on this page reproduce the underlying process's exact exit
code. A non-zero `container` status becomes an ac error (`command exited
<status>`) printed on stderr with an `err` prefix, and `ac` exits `1`.
`ac registry login` phrases its own failure instead (`login to <server>
failed`). Under `--json`, stdout may be empty on failure, so gate on the exit
code, not on parsing output.

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [Builds](builds.md): the project-scoped build with profiles, interpolation
  and filtered registry login.
- [Container commands](containers.md): the verbs that run what you built.
- [The project manifest](manifest.md): the `builds` and `registries` blocks.
- [Daemon, system and host-level commands](daemon-and-system.md): `ac system df`
  and `ac system prune` for reclaiming image storage.
