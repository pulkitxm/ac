# Shell completion

`ac` ships two completion mechanisms and they are not the same thing.

1. **The dynamic hook**, `source <(COMPLETE=<shell> ac)`. The shell asks the
   real `ac` binary for candidates on every TAB. This is the one to use.
2. **The static script**, `ac completions <shell>`. A one-shot generated
   script for the built-in commands only. No projects, no service names, no
   container names.

Both are driven from the same clap definition, so flags and subcommands look
identical. Everything that has to be looked up at TAB time (your manifests,
the daemon's containers and images) exists only in the dynamic hook.

## The dynamic hook (recommended)

`main.rs` calls `clap_complete::CompleteEnv` before anything else, so a run
with `COMPLETE` set in the environment prints the shell integration and exits
without touching the daemon or parsing your arguments.

| Shell | Line to add |
| --- | --- |
| zsh | `source <(COMPLETE=zsh ac)` in `~/.zshrc` |
| bash | `source <(COMPLETE=bash ac)` in `~/.bashrc` |
| fish | `source (COMPLETE=fish ac | psub)` in `~/.config/fish/config.fish` |
| elvish | `eval (E:COMPLETE=elvish ac | slurp)` in `~/.config/elvish/rc.elv` |
| powershell | `COMPLETE=powershell ac | Invoke-Expression` in your `$PROFILE` |

```bash
echo 'source <(COMPLETE=zsh ac)' >> ~/.zshrc
exec zsh
```

`make completions` prints the zsh and bash lines for you. Nothing is written
to disk: the hook is a few lines that call the binary, so completion is always
in step with the installed `ac` and with whatever manifests exist right now.
Installing a new manifest needs no regeneration and no shell restart.

If you installed under another name (`make install BIN_NAME=ac-dev`), use that
name in the hook: `source <(COMPLETE=zsh ac-dev)`.

## The static script

```console
$ ac completions zsh > ~/.zsh/completions/_ac
```

| Argument | Values |
| --- | --- |
| `<shell>` | `bash`, `zsh`, `fish`, `elvish`, `powershell` (alias `power-shell`) |

The shell argument is required and is the only argument; there are no flags.
Output goes to stdout.

What you give up compared with the hook:

- no project names, so `ac sh<TAB>` does not offer `shop`
- no per-project actions or manifest script names
- no service, build, profile or volume names
- no container names, image references or registry hosts
- it goes stale the moment the binary changes, since it is a snapshot

Use it only where sourcing a subprocess at shell startup is not acceptable,
or when packaging `ac` for a distribution that wants a static file.

## What the dynamic hook completes

Everything below comes from `src/completions.rs`, which rewrites the clap
command tree at completion time: it takes the `project` subcommand as a
template and grafts a copy of it under every discovered project name, so
`ac shop <TAB>` offers the same actions as `ac project shop <TAB>`.

| Position | Candidates | Source |
| --- | --- | --- |
| `ac <TAB>` | built-in commands plus every project name | `~/.config/ac/projects/*.json` and `<repo>/projects/*.json` |
| `ac project <TAB>` | project names | same |
| `ac <project> <TAB>` | project actions plus manifest script names | the CLI plus `scripts` in the manifest |
| service arguments (`start`, `stop`, `down`, `restart`, `rm`, `logs`, `exec`, `sh`, `top`, `wait`, ...) | both spellings, `redis` and `shop-redis` | manifest `services[].name` |
| `-P/--profile` | profile names | manifest `profiles` |
| `ac <project> build <TAB>`, `ac <project> push <TAB>` | build names | manifest `builds[].name` |
| `ac <project> volumes rm/inspect <TAB>` | volume names | manifest `services[].volumes[].name` |
| any other project `names` argument | service names and build names, unioned | manifest |
| container arguments on the global verbs (`start`, `stop`, `restart`, `rm`, `exec`, `sh`, `logs`, `inspect`, `kill`, `export`, `stats`, `top`, `port`) | live container names | `container ls -a -q` |
| image arguments (`run`, `create`, `pull`, `push`, `tag` source, `save`, `rmi`, `image rm`, `image inspect`, ...) | live image references | `container image ls -q` |
| `-s/--signal` on `kill` and `stop` | `KILL TERM INT HUP QUIT USR1 USR2 STOP CONT` | static list |
| `login`/`logout` server argument | registry hosts you are logged in to | `container registry ls -q` |
| `ac cp <TAB>` | container names suffixed `:/`, plus local paths | `container ls -a -q` plus the filesystem |
| `-f/--file`, `-i/--input` | files | the filesystem |
| build context | directories | the filesystem |
| `-o/--output` | any path | the filesystem |

`ac -p <TAB>` is **not** in that list. `-p` never reaches clap: `rewrite_argv`
in `src/main.rs` strips it before parsing, so the completion engine has no
argument to attach candidates to. Use `ac project <TAB>` when you want the
completer to offer project names explicitly.

Two gaps worth knowing, both because the completer keys off argument names and
these arguments are called `names`: `ac volume rm <TAB>` and
`ac network rm <TAB>` offer nothing. Their project-scoped equivalents
(`ac <project> volumes rm`) do complete.

Flags and their values complete everywhere, since they come straight from the
clap definitions. See [Project commands](project-commands.md),
[Container commands](containers.md) and
[Daemon and system commands](daemon-and-system.md) for what each one does.

`ac tag <TAB>` and `ac image tag <TAB>` are a deliberate exception: their
`target` argument is a new reference you are inventing, not one that exists,
so it is not completed from the image store. The exception is keyed on those
two paths alone, so any other argument named `target` does get image
references, including `ac build --target <TAB>`, where the value wanted is
really a Dockerfile build stage.

```console
$ ac sh<TAB>
shop     sh
$ ac shop st<TAB>
start   stats   stop
$ ac shop logs <TAB>
postgres  redis  shop-postgres  shop-redis
$ ac kill -s <TAB>
KILL  TERM  INT  HUP  QUIT  USR1  USR2  STOP  CONT
```

Hidden aliases are not offered as candidates, so `ac shop status` and
`ac shop ps` work but only `ls` completes; the same goes for `shell` (an alias
of `sh`) and `projects` (an alias of `ls`). Visible aliases do complete, so
`ac shop up` and `ac images` both appear.

### Manifest script names and their `complete` words

Any name in the manifest's `scripts` map completes as a project subcommand,
next to the built-in actions. A script whose name collides with a project
action is filtered out of the completion tree (manifest validation rejects such
a name anyway, so this only bites on an old manifest).

A plain string script offers nothing for its arguments. To get argument
completion, write the object form and list the words:

```json
"scripts": {
  "psql": "psql -h 127.0.0.1 -p 5433 -U user postgres",
  "tunnels": {
    "run": "~/.config/ac/scripts/shop-tunnels.sh",
    "complete": ["up", "down", "status"]
  }
}
```

```console
$ ac shop tunnels <TAB>
up  down  status
```

The words are offered at **every** argument position, not just the first, and
they are static data. `ac` never executes a script to work out its completions:
a completer that ran user code could hang the shell or dial out on every TAB.
See the manifest reference for the full `scripts` schema.

### Why some project names never appear

A project whose name is a reserved word (`run`, `build`, `start`, `stop`,
`logs`, `machine`, and the rest of `RESERVED` in `src/cli/reserved.rs`) is
skipped by the completer. `ac <that-name>` dispatches to the command, not the
project, so offering the project there would complete to something that cannot
run. Reach it explicitly instead:

```console
$ ac -p build start
```

`ac project <TAB>` does offer it, because in that position it is unambiguously
a project: the candidates there come from the same manifest listing, with no
reserved-word filter. `ac -p <TAB>` completes nothing at all, since `-p` is
stripped before clap sees it.

## Completers that hit the live daemon

Container names, image references and registry hosts cannot come from a
manifest, so those three completers shell out to `container` on every TAB.
They are bounded so a stopped or wedged daemon cannot hang your shell:

1. `container system status` runs first as a probe, silenced, with a **1 second**
   timeout. Anything other than success returns no candidates at all.
2. Only then does the real listing run, silenced, with a **2 second** timeout.
   Any failure yields an empty list.

The result is that TAB against a down daemon is empty and instant rather than
blocking. Nothing is echoed, and no daemon is ever started for a completion.

### AC_COMPLETE_OFFLINE

```bash
export AC_COMPLETE_OFFLINE=1
```

Set to any value to skip the daemon-backed completers outright: no probe, no
listing, no subprocess. Manifest-derived candidates (projects, services,
builds, profiles, volumes, scripts) and all flags keep working, because they
are read from JSON files on disk. Useful on a slow machine, or when you never
want TAB to touch the runtime.

### Where manifests are read from

The completer resolves projects exactly like the CLI does: `~/.config/ac/projects/`
first, then `<repo>/projects/`, honouring `XDG_CONFIG_HOME` and `AC_HOME`. A
user file shadows a bundled one of the same name. An unreadable or invalid
manifest yields no candidates for that project rather than an error, so a
broken JSON file makes TAB quiet, not noisy.

## Troubleshooting: TAB offers nothing

Work down this list.

1. **Is the hook actually loaded?** `echo $_comps[ac]` in zsh, or
   `complete -p ac` in bash, should print something. If not, the `source` line
   is missing or is in a file your shell does not read for interactive sessions.
2. **Is `ac` on the PATH the shell used at startup?** The hook calls `ac` by
   name. `which ac` should find the binary you installed.
3. **Ask the binary directly.** This is the same call the shell makes:
   ```bash
   COMPLETE=zsh ac -- ac shop ''
   ```
   Candidates on stdout means `ac` is fine and the problem is in the shell
   integration. Nothing means the problem is below.
4. **Only container and image names are missing?** That is the daemon. Check
   `ac daemon status`; if it is down, or slower than the 1 second probe budget,
   those three completers return empty by design. Manifest candidates should
   still appear.
5. **Is `AC_COMPLETE_OFFLINE` set?** `echo $AC_COMPLETE_OFFLINE`. If it is set,
   container, image and registry candidates are suppressed everywhere.
6. **Only project names are missing?** Run `ac ls`. If it lists nothing, the
   completer sees nothing either. Check the manifest lives in
   `~/.config/ac/projects/<name>.json` and that `ac <name> config` parses.
7. **One project is missing while others complete?** Either its manifest fails
   to parse (run `ac <name> config` to see the error) or its name is a reserved
   word, in which case use `ac -p <name>`.
8. **Using the static script?** `ac completions zsh` cannot offer projects,
   services or containers at all. Switch to the `COMPLETE=` hook.
9. **Stale after an upgrade?** The hook needs no regeneration, but a shell that
   cached the old function may. `exec zsh` (or `exec bash`) reloads it. A file
   written from `ac completions` must be regenerated by hand.

## See also

- [CLI reference](README.md): the index, the two invocation forms and the
  docker-to-ac translation table.
- [Global flags and invocation-wide behaviour](global-flags.md):
  `AC_COMPLETE_OFFLINE` alongside every other environment variable `ac` reads.
- [Project commands](project-commands.md): the actions and manifest scripts the
  dynamic hook offers.
- [Daemon, system and host-level commands](daemon-and-system.md):
  `ac completions <shell>` in context with the rest of the host-level commands.
