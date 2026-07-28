use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::cli::{
    Action, BuilderAction, DaemonAction, ImageAction, NetworkAction, RegistryAction, RunOpts,
    SystemAction, VolumeAction,
};

#[derive(Parser, Debug)]
#[command(
    name = "ac",
    version,
    about = "Apple Container project runner: docker-compose style stacks for `container`",
    long_about = "\
ac runs project-scoped service stacks on Apple `container`, filling the gap left
by the absence of `docker compose`.

The usual form is `ac <project> <action> [services...]`, for example
`ac shop start` or `ac shop restart redis`. `ac <project>` on its own is the
same as `ac <project> status`. When a project name collides with one of ac's own
commands, use the explicit form `ac -p <project> <action>`.

DAEMON OWNERSHIP
  If the container daemon is already running when ac needs it, ac uses it and
  never starts, restarts or stops it. If it is not running, ac starts it,
  records ownership in ~/.local/state/ac/daemon.owned, and spawns a supervisor
  that stops the daemon once the last ac-managed container across ALL projects
  has gone away.

Every underlying `container` command is echoed to stderr, dimmed and prefixed
with `$ `, before it runs. Set AC_QUIET=1 or pass --quiet to suppress that.",
    propagate_version = true
)]
pub struct Cli {
    /// Emit machine readable JSON instead of a human table. Implies --quiet.
    #[arg(long, global = true)]
    pub json: bool,

    /// Do not echo the underlying `container` commands. Same as AC_QUIET=1.
    /// There is deliberately no short form: `-q` means "names only" on the
    /// listing commands (ps, image ls) and "suppress build output" on
    /// `ac build`.
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Disable ANSI colour. Colour is off automatically when stdout is not a
    /// terminal, or when NO_COLOR is set.
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: TopCommand,
}

/// The `Project` variant is much larger than the rest because it carries a
/// whole `Action`. Boxing it would buy nothing: exactly one of these is parsed
/// per process.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub enum TopCommand {
    /// List the projects ac can see.
    ///
    /// Manifests are JSON files in ~/.config/ac/projects (user, wins) and
    /// <repo>/projects (bundled).
    ///
    /// Example: ac ls --json
    #[command(alias = "projects")]
    Ls,

    /// Daemon state, supervisor state, and the status of every project.
    ///
    /// Runs `container system status` plus one `container ls -a --format json`.
    ///
    /// Example: ac status --json
    Status,

    /// Inspect or stop the container daemon.
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },

    /// Containers across every project, docker ps style.
    ///
    /// Shows running containers; -a includes stopped and created ones. Rows
    /// are joined against the project manifests, so ac-managed containers
    /// carry their project and service names. With --json this emits
    /// [{container, project, service, state, ip, image}].
    ///
    /// Examples:
    ///   ac ps
    ///   ac ps -a --json
    Ps {
        /// Include containers that are not running.
        #[arg(short, long)]
        all: bool,
        /// Only print container names, docker ps -q style.
        #[arg(short = 'q')]
        ids: bool,
    },

    /// Manage the local image store, docker image style.
    ///
    /// With no subcommand this lists every image, so the old `ac images`
    /// keeps working.
    #[command(visible_alias = "images")]
    Image {
        /// Accepted for docker muscle memory; sizes are already shown.
        #[arg(short = 'v', long, hide = true)]
        verbose: bool,
        /// Only print image names, docker images -q style.
        #[arg(short = 'q', hide = true)]
        ids: bool,
        #[command(subcommand)]
        action: Option<ImageAction>,
    },

    /// Manage named volumes across the whole daemon, docker volume style.
    ///
    /// For the volumes one project declares, use `ac <project> volumes`.
    #[command(visible_alias = "volumes")]
    Volume {
        #[command(subcommand)]
        action: Option<VolumeAction>,
    },

    /// Manage container networks, docker network style.
    ///
    /// Every container joins `default` unless told otherwise. Networks other
    /// than the default require macOS 26 or newer.
    #[command(visible_alias = "networks")]
    Network {
        #[command(subcommand)]
        action: Option<NetworkAction>,
    },

    /// Daemon lifecycle and disk usage, docker system style.
    ///
    /// start and stop respect the ownership contract: ac never stops a
    /// daemon it did not start, and never restarts one that is already up.
    System {
        #[command(subcommand)]
        action: Option<SystemAction>,
    },

    /// Registry logins outside any project, docker login style.
    ///
    /// Project-scoped credentials belong in the manifest instead; those are
    /// used automatically by start, pull and build.
    Registry {
        #[command(subcommand)]
        action: Option<RegistryAction>,
    },

    /// Remove images, docker rmi style. Same as `ac image rm`.
    #[command(hide = true)]
    Rmi {
        /// References to remove.
        #[arg(required = true)]
        references: Vec<String>,
    },

    /// Disk usage for images, containers and volumes.
    ///
    /// Runs: container system df (--format json with --json)
    Df,

    /// Remove stopped containers and unused images.
    ///
    /// Runs `container prune` then `container image prune`, and finally
    /// re-checks whether the daemon can be released.
    Prune,

    /// Print the resolved ac configuration (~/.config/ac/config.json).
    ///
    /// Keys: appRoot, sparseBundle, imageMount, startTimeout.
    Config,

    /// Print the JSON Schema for a project manifest.
    ///
    /// Use this to author a project file without guessing at field names.
    ///
    /// Example: ac schema > manifest.schema.json
    Schema,

    /// Print the built-in manual, written for agents and humans alike.
    ///
    /// With no topic this is the full guide: a docker-to-ac command table,
    /// the daemon ownership rules, build behaviour and manifest authoring.
    /// `ac guide claude` prints a concise snippet to paste into another
    /// repository's CLAUDE.md so agents working there drive ac correctly.
    ///
    /// Examples:
    ///   ac guide
    ///   ac guide claude >> ../my-app/CLAUDE.md
    Guide {
        /// What to print. Omit for the full manual.
        topic: Option<GuideTopic>,
    },

    /// Generate a shell completion script.
    ///
    /// Example: ac completions zsh > ~/.zsh/completions/_ac
    Completions {
        /// Shell to generate for.
        shell: CompletionShell,
    },

    /// Print the ac version.
    Version,

    /// Run a container from an image, docker run style. No manifest needed.
    ///
    /// Runs: container run [options] <image> [command...]
    ///
    /// The container is labelled `ac.managed=1`, so if ac had to start the
    /// daemon for it, the supervisor counts it and will not stop the daemon
    /// while it is alive. `-t` is only passed when stdin AND stdout are
    /// terminals, because Apple `container` fails with ENODEV otherwise.
    ///
    /// Examples:
    ///   ac run -d --name web -p 3000:3000 my-app:dev
    ///   ac run --rm -it docker.io/library/alpine:3.20 sh
    Run {
        #[command(flatten)]
        opts: RunOpts,
        /// Remove the container when it exits.
        #[arg(long = "rm")]
        rm: bool,
        /// Image reference to run.
        image: String,
        /// Command and arguments, overriding the image default.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },

    /// Create a container without starting it, docker create style.
    ///
    /// Runs: container create [options] <image> [command...]
    /// Start it later with `ac start <name>`.
    Create {
        #[command(flatten)]
        opts: RunOpts,
        /// Remove the container when it exits.
        #[arg(long = "rm")]
        rm: bool,
        /// Image reference to create from.
        image: String,
        /// Command and arguments, overriding the image default.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },

    /// Build an image from a Dockerfile, docker build style. No manifest needed.
    ///
    /// Runs: container build [options] <context>
    ///
    /// For a project's declared builds, with profiles, interpolated tags and
    /// rollout hooks, use `ac <project> build` instead.
    ///
    /// Examples:
    ///   ac build -t my-app:dev .
    ///   ac build -t my-app:dev -f docker/Dockerfile --target runner .
    Build {
        /// Name for the built image, repeatable.
        #[arg(short = 't', long = "tag", value_name = "NAME")]
        tags: Vec<String>,
        /// Path to the Dockerfile.
        #[arg(short = 'f', long = "file", value_name = "PATH")]
        file: Option<String>,
        /// Target build stage.
        #[arg(long)]
        target: Option<String>,
        /// Platform to build for, os/arch[/variant].
        #[arg(long)]
        platform: Option<String>,
        /// Architecture to build for. --platform wins.
        #[arg(short = 'a', long)]
        arch: Option<String>,
        /// OS to build for. --platform wins.
        #[arg(long)]
        os: Option<String>,
        /// Build-time variable, repeatable.
        #[arg(long = "build-arg", value_name = "KEY=VALUE")]
        build_args: Vec<String>,
        /// Image label, repeatable.
        #[arg(short = 'l', long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
        /// Build secret, repeatable (id=<key>[,env=VAR|,src=PATH]).
        #[arg(long = "secret", value_name = "SPEC")]
        secrets: Vec<String>,
        /// Do not use the layer cache.
        #[arg(long)]
        no_cache: bool,
        /// Always attempt to pull a newer base image.
        #[arg(long)]
        pull: bool,
        /// Progress output style.
        #[arg(long, value_name = "auto|plain|tty")]
        progress: Option<String>,
        /// Output configuration, type=<oci|tar|local>[,dest=].
        #[arg(short = 'o', long, value_name = "SPEC")]
        output: Option<String>,
        /// CPUs for the builder container. Resizing discards its cache.
        #[arg(short = 'c', long = "cpus")]
        cpus: Option<u32>,
        /// Memory for the builder container. Resizing discards its cache.
        #[arg(short = 'm', long = "memory")]
        memory: Option<String>,
        /// Suppress build output, docker build -q style.
        #[arg(short = 'q', long = "build-quiet")]
        build_quiet: bool,
        /// Build context directory.
        #[arg(default_value = ".")]
        context: String,
    },

    /// Start one or more stopped containers, docker start style.
    ///
    /// Runs: container start <container...>
    /// For a whole project stack use `ac <project> start`.
    Start {
        /// Attach stdout and stderr.
        #[arg(short = 'a', long)]
        attach: bool,
        /// Attach stdin.
        #[arg(short = 'i', long)]
        interactive: bool,
        /// Containers to start.
        #[arg(required = true)]
        containers: Vec<String>,
    },

    /// Stop one or more running containers, docker stop style.
    ///
    /// Escalates the way project stop does: a bounded `container stop`, then
    /// SIGKILL, then the container's own runtime shim, so a wedged container
    /// still comes down. For a whole project use `ac <project> stop`.
    Stop {
        /// Seconds to wait before killing the container.
        #[arg(short = 't', long = "time", value_name = "SECS")]
        time: Option<u32>,
        /// Signal to send instead of the default stop sequence. Passing this
        /// bypasses ac's stop escalation and calls `container stop --signal`.
        #[arg(short = 's', long)]
        signal: Option<String>,
        /// Stop every running container.
        #[arg(short = 'a', long)]
        all: bool,
        /// Containers to stop.
        containers: Vec<String>,
    },

    /// Restart containers: stop then start, docker restart style.
    Restart {
        /// Seconds to wait before killing the container.
        #[arg(short = 't', long = "time", value_name = "SECS")]
        time: Option<u32>,
        /// Containers to restart.
        #[arg(required = true)]
        containers: Vec<String>,
    },

    /// Remove containers, docker rm style.
    ///
    /// Runs: container rm [--force] <container...>
    /// Images are `ac image rm` (or `ac rmi`).
    #[command(alias = "delete")]
    Rm {
        /// Remove even if the container is running.
        #[arg(short, long)]
        force: bool,
        /// Remove every container.
        #[arg(short = 'a', long)]
        all: bool,
        /// Containers to remove.
        containers: Vec<String>,
    },

    /// Run a command in a running container, docker exec style.
    ///
    /// Runs: container exec -i [-t] <container> <command...>
    /// `-t` is added only when stdin AND stdout are terminals.
    ///
    /// Example: ac exec -it web sh
    Exec {
        /// Keep stdin open. Always on; accepted for docker muscle memory.
        #[arg(short = 'i', long)]
        interactive: bool,
        /// Request a TTY. Honoured only when stdin and stdout are terminals.
        #[arg(short = 't', long)]
        tty: bool,
        /// Run detached.
        #[arg(short = 'd', long)]
        detach: bool,
        /// Environment entry, repeatable.
        #[arg(short = 'e', long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// Working directory inside the container.
        #[arg(short = 'w', long = "workdir")]
        workdir: Option<String>,
        /// User to run as, name|uid[:gid].
        #[arg(short = 'u', long)]
        user: Option<String>,
        /// Container to run in.
        container: String,
        /// Command and arguments.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },

    /// Open a shell in a running container.
    ///
    /// Runs bash when the image has it, otherwise sh.
    ///
    /// Example: ac sh web
    #[command(alias = "shell")]
    Sh {
        /// Container to enter.
        container: String,
    },

    /// Fetch container logs, docker logs style.
    ///
    /// Runs: container logs [-f] [-n N] [--boot] <container>
    Logs {
        /// Follow log output.
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show from the end.
        #[arg(short = 'n', long = "tail", value_name = "N")]
        tail: Option<u64>,
        /// Show the VM boot log instead of the container's stdio.
        #[arg(long)]
        boot: bool,
        /// Container to read.
        container: String,
    },

    /// Display detailed information about containers, docker inspect style.
    ///
    /// Runs: container inspect <container...>
    /// For images use `ac image inspect`.
    Inspect {
        /// Containers to inspect.
        #[arg(required = true)]
        containers: Vec<String>,
    },

    /// Send a signal to containers, docker kill style.
    ///
    /// Runs: container kill --signal <SIG> <container...>
    Kill {
        /// Signal to send.
        #[arg(short = 's', long, default_value = "KILL")]
        signal: String,
        /// Signal every running container.
        #[arg(short = 'a', long)]
        all: bool,
        /// Containers to signal.
        containers: Vec<String>,
    },

    /// Copy files between a container and the host, docker cp style.
    ///
    /// Runs: container cp <src> <dst>, where either side may be
    /// <container>:/path.
    ///
    /// Apple container 1.1.0 has known cp bugs: copies INTO a container can
    /// silently no-op while exiting 0, and copies out can hang. Prefer
    /// `ac exec` with shell redirection when it matters.
    #[command(alias = "copy")]
    Cp {
        /// Source path, local or <container>:/path.
        src: String,
        /// Destination path, local or <container>:/path.
        dst: String,
    },

    /// Export a container's filesystem as a tar archive.
    ///
    /// Runs: container export -o <output> <container>. Apple container
    /// refuses to export a RUNNING container, so stop it first.
    Export {
        /// Container to export.
        container: String,
        /// Output path. Defaults to <container>.tar.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Live resource usage, docker stats style.
    ///
    /// --json implies --no-stream and emits one snapshot.
    Stats {
        /// Take one sample and exit instead of streaming.
        #[arg(long)]
        no_stream: bool,
        /// Containers to include. Empty means every running one.
        containers: Vec<String>,
    },

    /// Processes running inside containers, docker top style.
    ///
    /// Runs `ps aux` (falling back to plain `ps`) through `container exec`.
    Top {
        /// Containers to show. Empty means every running one.
        containers: Vec<String>,
    },

    /// Published port mappings for a container, docker port style.
    Port {
        /// Container to show.
        container: String,
    },

    /// Pull an image from a registry, docker pull style. Same as
    /// `ac image pull`.
    Pull {
        /// Image reference to pull.
        reference: String,
        /// Platform to pull, os/arch[/variant].
        #[arg(long)]
        platform: Option<String>,
    },

    /// Push an image to a registry, docker push style. Same as
    /// `ac image push`.
    Push {
        /// Image reference to push.
        reference: String,
        /// Platform to push, os/arch[/variant].
        #[arg(long)]
        platform: Option<String>,
    },

    /// Tag an image, docker tag style. Same as `ac image tag`.
    Tag {
        /// Existing image reference.
        source: String,
        /// New reference.
        target: String,
    },

    /// Save an image to a tar archive, docker save style.
    Save {
        /// Image reference to save.
        reference: String,
        /// Output path.
        #[arg(short, long, required = true)]
        output: PathBuf,
    },

    /// Load images from a tar archive, docker load style.
    Load {
        /// Archive to read.
        #[arg(short, long, required = true)]
        input: PathBuf,
    },

    /// Log in to a registry, docker login style. Same as `ac registry login`.
    Login {
        /// Registry host.
        server: String,
        /// Username.
        #[arg(short, long)]
        username: Option<String>,
        /// Password. Prefer --password-stdin.
        #[arg(short, long)]
        password: Option<String>,
        /// Read the password from stdin.
        #[arg(long)]
        password_stdin: bool,
    },

    /// Log out of a registry, docker logout style.
    Logout {
        /// Registry host.
        server: String,
    },

    /// Manage the image builder container, `container builder` style.
    ///
    /// Sizing applies only at creation: `ac build -c/-m` stops and recreates
    /// the builder when a resize is needed, discarding its layer cache.
    Builder {
        #[command(subcommand)]
        action: Option<BuilderAction>,
    },

    /// Manage container machines, `container machine` style.
    ///
    /// Passed through unchanged; ac adds nothing here beyond ensuring the
    /// daemon for mutating subcommands.
    #[command(visible_alias = "machines")]
    Machine {
        /// Subcommand and arguments, passed to `container machine` verbatim.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run an action against a project.
    ///
    /// This is what `ac <project> <action>` expands to. Write it out in full
    /// (or use `-p <project>`) when a project name collides with one of the
    /// commands above.
    ///
    /// Example: ac project shop restart redis
    Project {
        /// Project name, matching a manifest file name without the extension.
        name: String,
        #[command(subcommand)]
        action: Action,
    },

    /// The detached supervisor loop. Not for direct use.
    #[command(name = "__supervise", hide = true)]
    Supervise,
}

/// Every flag `container run` and `container create` share. Both docker verbs
/// take exactly the same set, so they are declared once and flattened into
/// both.

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum GuideTopic {
    /// A concise CLAUDE.md snippet for making another repo ac-aware.
    Claude,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Elvish,
    #[value(name = "powershell", alias = "power-shell")]
    PowerShell,
}
