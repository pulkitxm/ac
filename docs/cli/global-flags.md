# Global flags and invocation-wide behaviour

Everything on this page applies to every `ac` invocation, whichever form you
use: `ac <project> <action>`, `ac <verb> <container>`, or a noun group like
`ac image ls`. Three of them are declared once in `src/cli/root.rs` as clap
global arguments, so they are accepted at any depth of the command tree, with
one important exception described in
[Trailing arguments swallow global flags](#trailing-arguments-swallow-global-flags).

For the commands themselves see [Project commands](project-commands.md),
[Container commands](containers.md),
[Image and registry commands](images-and-registries.md) and
[Daemon and system commands](daemon-and-system.md).

## The flags

| Flag | Default | What it does |
| --- | --- | --- |
| `--json` | off | Emit machine readable JSON instead of a human table. Implies `--quiet`, moves human log lines to stderr, and disables colour. |
| `--quiet` | off | Do not echo the underlying `container` commands. Same as `AC_QUIET=1`. No short form. |
| `--no-color` | off | Disable ANSI colour, whatever the terminal says. |
| `-h`, `--help` | | Help for the command or subcommand it follows. Every subcommand has its own. |
| `-V`, `--version` | | Print the ac version. `propagate_version` is on, so `-V` works on subcommands too. `ac version` prints the same thing. |

`--json`, `--quiet` and `--no-color` are the only three clap globals. `-h` and
`-V` come from clap itself.

`-p` / `--project <NAME>` looks like a fourth global but is not a clap argument
at all: `rewrite_argv` in `src/main.rs` consumes it before parsing and turns
`ac -p <name> <action>` into `ac project <name> <action>`. That is why it has
to appear immediately after the global flags and before anything else, and why
`--project=<NAME>` works while `-p=<NAME>` does not. See
[The `-p` escape hatch and argv rewriting](#the--p-escape-hatch-and-argv-rewriting).

There are no other invocation-wide flags. Everything else (`-a`, `-q`, `-t`,
`-f`, ...) belongs to a specific command and is documented with that command.

```console
$ ac --json ps
$ ac --quiet shop start
$ ac --no-color shop status
$ ac -p status start          # project literally named "status"
$ ac --project=build config   # project literally named "build"
```

## What `--json` implies

`--json` is not only an output format. In `src/core/ctx.rs` it sets three
things at once:

- **Quiet is forced on.** `quiet = quiet || json || AC_QUIET`. The `$ container
  ...` echo lines are suppressed.
- **Colour is forced off.** `color` requires `!json`, so no ANSI ever reaches a
  JSON stream.
- **Human log lines move to stderr.** `ctx.log`, `ctx.info`, `ctx.ok` and
  `ctx.dim` print to stdout normally, and to stderr when `--json` is set.
  `ctx.warn` and `ctx.err` always go to stderr.

The result is the contract worth relying on in scripts: **stdout is one
parseable JSON document and nothing else**. On failure stdout may be empty, so
branch on the exit code, not on the presence of output.

```console
$ ac --json ps | jq -r '.[] | select(.state=="running") | .container'
$ ac --json shop status 2>/dev/null | jq '.services'
```

A few commands change behaviour under `--json` rather than just reformatting:

- `ac stats` and `ac <project> stats` imply `--no-stream`, take a single
  sample, and are killed after 20 seconds if the runtime wedges.
- `ac image ls` stops building its own table and passes the daemon's
  `container image ls --format json` through unchanged. With `-q` as well it
  emits a plain JSON array of image names.
- Commands that are pure passthroughs to `container` with no JSON of their own
  (for example `ac machine ...`) print whatever `container` prints.

Not every command has a JSON shape. The rule is that **reads** have one
(`status`, `ps`, `ls`, `config`, `images`, `port`, `ip`, `env`, `stats`,
`inspect`, `scripts`, `df`, and the build summary), and pure passthrough verbs
do not.

## `--format json` is rewritten, other values are an error

For docker muscle memory, `--format json` and `--format=json` anywhere on the
line are rewritten to `--json` before clap ever sees them (`map_format_json` in
`src/main.rs`). Any other `--format` value is a hard error, because ac has no
other output format:

```console
$ ac ps --format json      # same as: ac ps --json
$ ac image ls --format=json
$ ac ps --format table
err --format table is not supported; ac emits JSON only, use --json
```

The rewrite deliberately stops at a **passthrough zone**. Once the parser has
seen `exec`, `run`, `cp`, `copy` or `machine` used as a verb (in the first
word, or in the second word when the first is a project name), everything after
it is left exactly as written, so a `--format json` intended for the command
running inside the container survives:

```console
$ ac demo exec web mycli --format json    # --format json goes to mycli
$ ac stop run --format json               # "run" here is a container name, so this becomes --json
```

## The three meanings of `-q`

`-q` is not a global flag, on purpose. It means three different things
depending on where it appears, and the global quiet switch is spelled out in
full to keep them apart.

| Where | Meaning |
| --- | --- |
| `ac ps -q` | Print container names only, docker `ps -q` style. There is no long form. |
| `ac image ls -q` (and the hidden `-q` on bare `ac image` / `ac images`) | Print image names only. |
| `ac build -q` (`--build-quiet`) | Suppress build output, passed on as `container build --quiet`. |
| anywhere else | Not accepted. Use `--quiet` or `AC_QUIET=1`. |

```console
$ ac ps -q | xargs -n1 ac stop
$ ac image ls -q
$ ac build -q -t app:dev .
$ ac --quiet shop start        # not: ac -q shop start
```

## Trailing arguments swallow global flags

Four top-level commands forward everything after their target verbatim: `run`,
`create`, `exec` and `machine`, and so do the project-scoped `ac <project> run`
and `ac <project> exec`. They are declared `trailing_var_arg` with
`allow_hyphen_values`, so a global flag written after the target is part of the
forwarded argv, not a flag for ac. Put global flags **first**.

(`cp` is not in this list: it takes two ordinary positionals. It is in the
`--format` passthrough zone below, but a `--json` written after its arguments
is still parsed by clap as ac's own.)

```console
$ ac --json machine ls        # correct: --json is ac's
$ ac machine --json ls        # wrong: --json is handed to `container machine`

$ ac --quiet run --rm alpine sh -c 'echo hi'
$ ac run --rm alpine sh -c 'echo hi' --quiet    # --quiet goes to sh

$ ac --json exec web ps aux
$ ac exec web ps aux --json                     # --json goes to ps
```

Everywhere else the flags really are global, so both of these work:

```console
$ ac --json shop ls
$ ac shop ls --json
```

The argv rewriter only hoists global flags that appear **before** the project
name; the rest are parsed by clap as global arguments on the subcommand.

## The `-p` escape hatch and argv rewriting

`ac <project> <action>` is shorthand. `src/main.rs` rewrites it to
`ac project <project> <action>` before parsing. The rules:

- A first word starting with `-`, or listed in `RESERVED`
  (`src/cli/reserved.rs`), is left alone and parses as one of ac's own
  commands.
- Otherwise it must name a discoverable project. If it does not, ac fails with
  a message listing the known projects and commands, and, when the word is a
  project action such as `start`, a hint pointing at
  `ac <project> start ...`.
- `ac <project>` with no action becomes `ac <project> status`.
- `-p <name>` / `--project=<name>` skips both checks entirely: the reserved-word
  check, which is the only way to reach a project whose name collides with a
  command, and the "is this a discoverable project" check, so an unknown name
  passed this way fails later, when the manifest is looked up.
  `ac -p weird` also defaults to `status`.

```console
$ ac shop start          # rewritten to: ac project shop start
$ ac shop                # rewritten to: ac project shop status
$ ac -p logs down        # project named "logs"
$ ac project shop restart redis   # the fully written out form
```

`ac -p` with no name following it is an error (`-p requires a project name`).

## Colour and TTY detection

Colour is enabled only when **all** of these hold (`src/core/ctx.rs`):

- `--no-color` was not passed
- `--json` was not passed
- `NO_COLOR` is not set in the environment (any value, including empty)
- stdout is a terminal

The decision is made once at startup and installed as a global override, so
`src/core/style.rs` is the only place ANSI is produced and every code path
honours it. Progress rendering follows the same switch: on a TTY each build
draws one live line, and off a TTY (or under `--json`, or with
`--progress plain`) it streams plain prefixed lines instead.

TTY detection is separate from colour in two places, and both check stdin and
stdout together because Apple `container` fails with ENODEV otherwise:

- `ac exec` and `ac <project> exec` pass `-i` always and add `-t` only when
  stdin **and** stdout are terminals. Docker's `-it` is accepted and ignored.
- `ac run` / `ac <project> run` request a TTY under the same condition.

## The command echo

Before running any `container` command, ac prints it to stderr, dimmed and
prefixed with `$ `, so any step can be copied and re-run by hand:

```console
$ ac shop stop redis
$ container ls -a --format json
$ container stop --time 10 shop-redis
ok stopped shop-redis
```

The echo goes to **stderr**, never stdout, so it never pollutes piped output.
It covers non-`container` helpers too (`hdiutil`, `date`, `sh -c` for manifest
scripts). Suppress it with `--quiet`, `--json` or `AC_QUIET=1`. Some repeated
commands are echoed once per invocation rather than once per call.

## Environment variables ac reads

| Variable | Default | What it does |
| --- | --- | --- |
| `AC_QUIET` | unset | Any value suppresses the command echo, exactly like `--quiet`. |
| `NO_COLOR` | unset | Any value disables colour, exactly like `--no-color`. |
| `AC_ROOT` | unset | Build root override. Beaten only by `--root`; beats git worktree detection and the manifest's `root`. Must exist, or the build fails. |
| `AC_PROFILE` | `local` | Default build profile for `ac <project> build`, `push`, `rollout` and `login` when `-P/--profile` is not given, and the profile used to expand `{{...}}` in service image references. An empty value is ignored. |
| `NO_CACHE` | unset | Any value adds `--no-cache` to every project build, as if `--no-cache` had been passed. |
| `AC_POLL_INTERVAL` | `5` | Seconds between supervisor polls. Read from the environment the supervisor was spawned with. |
| `AC_IDLE_GRACE` | `4` | Consecutive idle polls required before the supervisor stops an ac-owned daemon. |
| `AC_COMPLETE_OFFLINE` | unset | Any value makes shell completion skip the daemon-backed completers (container names, image references, registry hosts), so TAB is instant and empty rather than waiting on `container`. |
| `AC_HOME` | derived | Where ac looks for the bundled `projects/` directory. Otherwise it walks up from the resolved binary path looking for a `projects` directory, then falls back to `~/scripts/ac`. |
| `XDG_CONFIG_HOME` | `~/.config` | Parent of the config directory `ac/` (`config.json`, `projects/`). |
| `XDG_STATE_HOME` | `~/.local/state` | Parent of the state directory `ac/` (`daemon.owned`, `supervisor.pid`, `supervisor.log`). |
| `HOME` | required | Must be set; ac errors with `HOME is not set` otherwise. |
| `COMPLETE` | unset | Set by the shell hook `make completions` prints (`COMPLETE=zsh ac` and friends) to make ac emit completion candidates and exit. |

`AC_QUIET` and `NO_COLOR` are the only two that alter every command. The rest
are scoped to builds, the supervisor, completion, or path discovery.

```console
$ AC_QUIET=1 ac --json ps
$ NO_COLOR=1 ac shop status
$ AC_PROFILE=prod ac shop build
$ AC_POLL_INTERVAL=1 AC_IDLE_GRACE=3 ac shop start
$ AC_COMPLETE_OFFLINE=1 ac shop <TAB>
```

## Environment variables ac sets

Manifest scripts (`ac <project> <script>`, run through `sh -c`) receive:

| Variable | Value |
| --- | --- |
| `AC_PROJECT` | the project name |
| `AC_PROJECT_FILE` | absolute path to the manifest that was resolved |
| `AC_PROJECT_ROOT` | the manifest's `root`, only when it is set |

Rollout and build hooks receive a much larger set (`AC_IMAGE_<BUILD>`,
`AC_IMAGES_<BUILD>`, `AC_IMAGES`, `AC_BUILDS`, `AC_PROFILE`, `AC_ACCOUNT`,
`AC_REGISTRY`, `AC_TAG`, `AC_REGION`, `AC_ROOT`, `AC_VERSION`, `AC_GIT_SHA`,
`AC_GIT_SHORT_SHA`, `AC_GIT_BRANCH`, `AC_GIT_DIRTY`, `AC_TIMESTAMP`); see
[Builds and rollouts](builds.md).

## Exit codes

- **0** on success.
- **1** on any ac-level error: an unknown project or command, a manifest that
  fails to parse, a `--format` value that is not `json`, a missing daemon where
  one was required, a stop that did not take effect. The message goes to stderr
  prefixed with a red `err`.
- **2** from clap for a usage error: an unknown flag, a missing required
  argument, a bad value. `--help` and `--version` exit 0.
- **The child's own code**, passed straight through, for commands that mostly
  exist to run something else: `ac run`, `ac create`, `ac exec`, `ac sh`,
  `ac logs`, the same project-scoped verbs, and manifest scripts. `ac shop test`
  exiting 3 means the script exited 3.

Everywhere else a non-zero `container` exit is turned into an ac error, so the
process exits 1 rather than mirroring the runtime's code.

Two behaviours worth relying on in scripts:

- `ac <project> wait` exits non-zero on timeout, so it can gate a deployment
  step on readiness.
- `stop` and `down` verify against a fresh `container ls -a` rather than
  trusting the exit code, because `container run` and `container stop` both
  lie occasionally. A container still running after a stop is a failure even
  when the runtime reported success.

```bash
if ac shop wait --timeout 60; then
  ac --json shop ps | jq -r '.[].container'
else
  echo "stack did not come up" >&2
  exit 1
fi
```

## Daemon gating, in one paragraph

It is invocation-wide behaviour, so it belongs here even though the detail
lives with each command. **Commands that need an existing daemon** (`ps`,
`logs`, `inspect`, `port`, `stats`, `top`, `export`, `cp`, `exec`, `sh`, `df`,
`image ls`, `volume ls`, `save`, `logout`, and also `stop`, `rm` and `kill`)
call `daemon::require` and fail with exactly this message rather than starting
a daemon behind your back:

```console
err container daemon is not running; start it with `ac system start` or any `ac <project> start`
```

`stop`, `rm` and `kill` additionally re-check the refcount afterwards, so
stopping the last ac container releases an ac-owned daemon. **Mutations that
leave nothing behind** (`build`, `pull`, `push`, `tag`, `load`, `login`,
`system prune`, `builder start`, the mutating `machine` subcommands) call
`daemon::ensure` and re-check the refcount afterwards, releasing the daemon
again if ac started it. **Mutations that leave something running** (`run`,
`create`, `start`, `restart`, `system start`, `<project> start`, and the
`volume` / `network` mutating passthroughs) additionally spawn the supervisor.
ac never stops a daemon it did not start.

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [ac for scripts, CI and agents](agents-and-json.md): the JSON shape each
  command emits under `--json`.
- [Daemon, system and host-level commands](daemon-and-system.md): the ownership
  contract behind the daemon gating summarised above.
- [Project commands](project-commands.md) and
  [Container commands](containers.md): the commands these flags apply to.
