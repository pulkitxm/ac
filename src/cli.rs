use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "ac",
    version,
    about = "Apple Container project runner: docker-compose style stacks for `container`",
    long_about = "\
ac runs project-scoped service stacks on Apple `container`, filling the gap left
by the absence of `docker compose`.

The usual form is `ac <project> <action> [services...]`, for example
`ac noveum start` or `ac noveum restart redis`. `ac <project>` on its own is the
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
    disable_help_subcommand = true,
    propagate_version = true
)]
pub struct Cli {
    /// Emit machine readable JSON instead of a human table. Implies --quiet.
    #[arg(long, global = true)]
    pub json: bool,

    /// Do not echo the underlying `container` commands. Same as AC_QUIET=1.
    #[arg(short, long, global = true)]
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

    /// Every image in the local image store.
    ///
    /// Runs: container image ls (--format json with --json)
    Images,

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

    /// Generate a shell completion script.
    ///
    /// Example: ac completions zsh > ~/.zsh/completions/_ac
    Completions {
        /// Shell to generate for.
        shell: CompletionShell,
    },

    /// Print the ac version.
    Version,

    /// Run an action against a project.
    ///
    /// This is what `ac <project> <action>` expands to. Write it out in full
    /// (or use `-p <project>`) when a project name collides with one of the
    /// commands above.
    ///
    /// Example: ac project noveum restart redis
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

#[derive(Subcommand, Debug)]
pub enum DaemonAction {
    /// Show whether the daemon is running and who owns it.
    ///
    /// "owned by ac" means ac started it and may stop it. "external" means it
    /// was already running and ac will never touch it.
    Status,
    /// Stop the daemon, but only if ac started it.
    ///
    /// Kills the supervisor first, then runs `container system stop`. Does
    /// nothing when the daemon was already running before ac was involved.
    Stop,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Elvish,
    PowerShell,
}

#[derive(Subcommand, Debug)]
pub enum Action {
    /// Start services, creating them if needed.
    ///
    /// Ensures the daemon is up (starting and owning it when it was down),
    /// authenticates to any registry the images actually come from, creates
    /// missing named volumes, then for each service runs `container start` when
    /// a stopped container already exists, or `container run -d` otherwise.
    /// Waits on each service's readyCmd before moving to the next.
    ///
    /// Examples:
    ///   ac noveum start
    ///   ac noveum start redis clickhouse
    ///   ac noveum start --recreate postgres
    Start {
        /// Delete and recreate containers instead of restarting them in place.
        /// Named volumes and their data survive.
        #[arg(long)]
        recreate: bool,
        /// Services to act on. Empty means all of them. Either `redis` or
        /// `noveum-redis` is accepted.
        services: Vec<String>,
    },

    /// Stop services, keeping the containers for a fast restart.
    ///
    /// Runs `container stop <name>` per service. The container keeps its
    /// filesystem, so `ac <project> start` brings it back in place. Afterwards
    /// ac refcounts running containers across ALL projects and releases the
    /// daemon only if it owns it and nothing is left.
    ///
    /// Example: ac noveum stop redis
    Stop {
        /// Services to act on. Empty means all of them.
        services: Vec<String>,
    },

    /// Stop AND remove containers. Named volumes and their data survive.
    ///
    /// Runs `container stop` then `container rm` per service, then the same
    /// cross-project daemon refcount check as `stop`.
    ///
    /// Example: ac noveum down
    Down {
        /// Services to act on. Empty means all of them.
        services: Vec<String>,
    },

    /// Stop then start services.
    ///
    /// Exactly `ac <project> stop <svc>` followed by `ac <project> start <svc>`.
    ///
    /// Example: ac noveum restart redis
    Restart {
        /// Recreate rather than restart in place on the way back up.
        #[arg(long)]
        recreate: bool,
        /// Services to act on. Empty means all of them.
        services: Vec<String>,
    },

    /// Per-service state, IP and published ports.
    ///
    /// Reads one `container ls -a --format json` and joins it against the
    /// manifest, so services that were never created show as `absent`.
    ///
    /// Example: ac noveum ls --json
    #[command(alias = "ps", alias = "status")]
    Ls,

    /// Show or follow logs.
    ///
    /// With a service name this is `container logs [flags] <project>-<svc>`.
    /// With no service it fans out across every service, prefixing and
    /// colouring each line by service the way `docker compose logs` does, and
    /// Ctrl-C tears down the whole group.
    ///
    /// Examples:
    ///   ac noveum logs -f
    ///   ac noveum logs -n 100 postgres
    Logs {
        /// Follow log output.
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show from the end of the logs.
        #[arg(short = 'n', long = "tail", value_name = "N")]
        tail: Option<u64>,
        /// Show the VM boot log instead of the container's stdio.
        #[arg(long)]
        boot: bool,
        /// Service to read. Empty means every service, interleaved.
        service: Option<String>,
    },

    /// Run a command inside a service.
    ///
    /// Runs `container exec -i [-t] <project>-<svc> <command...>`. The `-t` is
    /// only added when stdin AND stdout are terminals, because Apple
    /// `container` fails with ENODEV otherwise.
    ///
    /// Example: ac noveum exec postgres psql -U user -c 'select 1'
    Exec {
        /// Service to run in.
        service: String,
        /// Command and arguments.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },

    /// Open an interactive shell inside a service.
    ///
    /// Runs bash when the image has it, otherwise sh.
    ///
    /// Example: ac noveum sh redis
    #[command(alias = "shell")]
    Sh {
        /// Service to enter. Defaults to the first service in the manifest.
        service: Option<String>,
    },

    /// Live resource usage.
    ///
    /// Runs: container stats <containers...>
    Stats {
        /// Services to include. Empty means all of them.
        services: Vec<String>,
    },

    /// Full container JSON, straight from the daemon.
    ///
    /// Runs: container inspect <containers...>
    Inspect {
        /// Services to include. Empty means all of them.
        services: Vec<String>,
    },

    /// Send a signal to services.
    ///
    /// Runs: container kill --signal <SIG> <containers...>
    Kill {
        /// Signal name, without the SIG prefix.
        #[arg(short = 's', long, default_value = "KILL")]
        signal: String,
        /// Services to signal. Empty means all of them.
        services: Vec<String>,
    },

    /// Force remove containers, leaving named volumes intact.
    ///
    /// Runs `container rm --force`, then the cross-project daemon refcount
    /// check.
    Rm {
        /// Services to remove. Empty means all of them.
        services: Vec<String>,
    },

    /// Copy files to or from a service.
    ///
    /// `svc:/path` is rewritten to `<project>-<svc>:/path` on either side;
    /// anything else is treated as a host path.
    ///
    /// Example: ac noveum cp ./dump.sql postgres:/tmp/dump.sql
    Cp {
        /// Source, host path or svc:/path.
        src: String,
        /// Destination, host path or svc:/path.
        dst: String,
    },

    /// Pre-pull images so a later start is fast.
    ///
    /// Runs `container image pull` per service, after any needed registry
    /// login.
    Pull {
        /// Services to pull for. Empty means all of them.
        services: Vec<String>,
    },

    /// Inspect and manage the images this project uses.
    ///
    /// With no subcommand this lists them, so `ac noveum images` keeps
    /// working. The subcommands act on the local image store.
    ///
    /// Examples:
    ///   ac noveum images
    ///   ac noveum images rm redis
    ///   ac noveum images prune
    Images {
        #[command(subcommand)]
        action: Option<ImagesAction>,
    },

    /// Inspect and manage the named volumes this project declares.
    ///
    /// Volumes hold the data that survives `down` and `rm`, so removing one is
    /// the only destructive operation in ac. With no subcommand this lists
    /// them.
    ///
    /// Examples:
    ///   ac noveum volumes
    ///   ac noveum volumes rm postgres-data
    ///   ac noveum volumes prune
    Volumes {
        #[command(subcommand)]
        action: Option<VolumesAction>,
    },

    /// Published port mappings declared in the manifest.
    Port {
        /// Services to show. Empty means all of them.
        services: Vec<String>,
    },

    /// Container IPs, as reported by the daemon.
    ///
    /// Apple `container` gives every container a routable 192.168.64.x address,
    /// so services are reachable without publishing ports. ICMP is blocked, so
    /// ping fails even when TCP works.
    Ip {
        /// Services to show. Empty means all of them.
        services: Vec<String>,
    },

    /// Environment variables a service is started with, from the manifest.
    Env {
        /// Service to show.
        service: String,
    },

    /// Build the project's images.
    ///
    /// Resolves every setting through CLI flag > profile > build entry >
    /// project default, runs each build's preflight hooks, then
    /// `container build`. When the profile pushes, each tag is pushed and the
    /// postPush hooks run. A failing preflight or postPush aborts immediately.
    ///
    /// Examples:
    ///   ac noveum build
    ///   ac noveum build web --profile dev
    ///   ac noveum build --platform linux/amd64 --no-cache --sequential
    Build(BuildArgs),

    /// Authenticate to the project's private registries.
    ///
    /// Runs each registry's passwordCmd and pipes it to
    /// `container registry login --password-stdin`. Credentials are never
    /// stored in the manifest. Called on your behalf by start, pull and build.
    Login {
        /// Profile whose {{account}} and {{region}} fill in the server template.
        #[arg(short = 'P', long)]
        profile: Option<String>,
    },

    /// Print the project manifest as written.
    Config,

    /// List the service names this project declares.
    ///
    /// Reads the manifest only, so it works with the daemon stopped. Shell
    /// completion uses it, and it is the reliable way for a script or agent to
    /// discover what can be passed to start, stop, logs, exec and friends.
    ///
    /// Examples:
    ///   ac noveum services
    ///   ac noveum services --json
    Services,

    /// List the build names this project declares.
    ///
    /// Reads the manifest only, so it works with the daemon stopped. These are
    /// the names accepted by `ac <project> build`.
    ///
    /// Examples:
    ///   ac noveum builds
    Builds,

    /// List the build profile names this project declares.
    ///
    /// Reads the manifest only, so it works with the daemon stopped. These are
    /// the values accepted by `ac <project> build --profile`.
    ///
    /// Examples:
    ///   ac noveum profiles
    Profiles,
}

#[derive(Args, Debug)]
pub struct BuildArgs {
    /// Profile to build for. Defaults to $AC_PROFILE, then `local`.
    #[arg(short = 'P', long)]
    pub profile: Option<String>,

    /// Build from this tree. Overrides every other root rule.
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// Target platform, e.g. linux/amd64 or linux/arm64.
    #[arg(long)]
    pub platform: Option<String>,

    /// Push the resulting tags, overriding the profile.
    #[arg(long, overrides_with = "no_push")]
    pub push: bool,

    /// Do not push, overriding the profile.
    #[arg(long, overrides_with = "push")]
    pub no_push: bool,

    /// Ignore the layer cache.
    #[arg(long)]
    pub no_cache: bool,

    /// Build output style. `plain` shows honest line by line output.
    #[arg(long, value_name = "auto|plain|tty")]
    pub progress: Option<String>,

    /// Dockerfile stage to stop at.
    #[arg(long)]
    pub target: Option<String>,

    /// Resize the shared buildkit builder. The builder only reads this when it
    /// is created, so changing it stops and recreates the builder, discarding
    /// its layer cache.
    #[arg(long)]
    pub builder_cpus: Option<u32>,

    /// Memory for the shared buildkit builder, e.g. 8g. Same caveat as
    /// --builder-cpus.
    #[arg(long)]
    pub builder_memory: Option<String>,

    /// Build one image at a time instead of in parallel.
    #[arg(long)]
    pub sequential: bool,

    /// Resolve and print what would be built, without building or pushing.
    ///
    /// Touches nothing: no daemon, no builder, no registry login. Pair with
    /// --json to inspect the resolved plan from a script.
    #[arg(long)]
    pub dry_run: bool,

    /// Builds to run, by name. Empty means every build in the manifest.
    pub names: Vec<String>,
}

impl BuildArgs {
    /// `--push` and `--no-push` collapse into a tri-state: `None` means the
    /// profile decides.
    pub fn push_override(&self) -> Option<bool> {
        if self.push {
            Some(true)
        } else if self.no_push {
            Some(false)
        } else {
            None
        }
    }
}

/// Words that can never be interpreted as a project name in the
/// `ac <project> <action>` shorthand. Use `ac -p <name>` when a project really
/// is called one of these.
pub const RESERVED: &[&str] = &[
    "ls",
    "projects",
    "status",
    "daemon",
    "images",
    "df",
    "prune",
    "config",
    "schema",
    "completions",
    "version",
    "project",
    "help",
    "__supervise",
];

#[derive(Debug, Subcommand)]
pub enum ImagesAction {
    /// List the images this project's services and builds declare.
    ///
    /// Reads the manifest, so it works with the daemon stopped.
    Ls,

    /// Remove this project's images from the local store.
    ///
    /// Resolves each name through the manifest, so `redis` means whatever
    /// image the redis service declares. Empty means every image the project
    /// declares. Runs `container image rm` per image.
    ///
    /// Examples:
    ///   ac noveum images rm redis
    ///   ac noveum images rm
    Rm {
        /// Services or builds whose images to remove. Empty means all.
        names: Vec<String>,
    },

    /// Remove images this project declares that no container is using.
    ///
    /// Runs `container image prune`, then reports what the project still has.
    Prune,
}

#[derive(Debug, Subcommand)]
pub enum VolumesAction {
    /// List the volumes this project declares, and whether they exist yet.
    ///
    /// Reads the manifest for the declared set and the daemon for what is
    /// actually present.
    Ls,

    /// Delete this project's volumes. THIS DESTROYS THE DATA IN THEM.
    ///
    /// Names are the manifest names, so `postgres-data` means the volume the
    /// manifest calls postgres-data, stored as `<project>-postgres-data`.
    /// Empty means every volume the project declares.
    ///
    /// Examples:
    ///   ac noveum volumes rm postgres-data
    Rm {
        /// Volumes to delete. Empty means all of this project's volumes.
        names: Vec<String>,
    },

    /// Show the daemon's full JSON for this project's volumes.
    Inspect {
        /// Volumes to inspect. Empty means all of this project's volumes.
        names: Vec<String>,
    },

    /// Remove volumes no container references, across the whole daemon.
    Prune,
}
