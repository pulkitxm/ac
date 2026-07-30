use std::path::PathBuf;

use clap::{Args, Subcommand};

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
    ///   ac shop start
    ///   ac shop up redis clickhouse
    ///   ac shop start --recreate postgres
    #[command(visible_alias = "up")]
    Start {
        /// Delete and recreate containers instead of restarting them in place.
        /// Named volumes and their data survive.
        #[arg(long)]
        recreate: bool,
        /// Accepted for docker compose muscle memory; every container is
        /// detached anyway.
        #[arg(short = 'd', long, hide = true)]
        detach: bool,
        /// Services to act on. Empty means all of them. Either `redis` or
        /// `shop-redis` is accepted.
        services: Vec<String>,
    },

    /// Stop services, keeping the containers for a fast restart.
    ///
    /// Runs `container stop <name>` per service. The container keeps its
    /// filesystem, so `ac <project> start` brings it back in place. Afterwards
    /// ac refcounts running containers across ALL projects and releases the
    /// daemon only if it owns it and nothing is left.
    ///
    /// Example: ac shop stop redis
    Stop {
        /// Seconds to wait before the container is killed, docker stop -t
        /// style.
        #[arg(short = 't', long = "time", value_name = "SECS")]
        time: Option<u32>,
        /// Services to act on. Empty means all of them.
        services: Vec<String>,
    },

    /// Stop AND remove containers. Named volumes and their data survive.
    ///
    /// Runs `container stop` then `container rm` per service, then the same
    /// cross-project daemon refcount check as `stop`.
    ///
    /// Example: ac shop down
    Down {
        /// ALSO DELETE the services' named volumes and their data, docker
        /// compose down -v style. Without this flag volumes always survive.
        #[arg(short = 'v', long)]
        volumes: bool,
        /// Seconds to wait before the container is killed, docker stop -t
        /// style.
        #[arg(short = 't', long = "time", value_name = "SECS")]
        time: Option<u32>,
        /// Services to act on. Empty means all of them.
        services: Vec<String>,
    },

    /// Stop then start services.
    ///
    /// Exactly `ac <project> stop <svc>` followed by `ac <project> start <svc>`.
    ///
    /// Example: ac shop restart redis
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
    /// Example: ac shop ls --json
    #[command(alias = "ps", alias = "status")]
    Ls {
        /// Accepted for docker muscle memory; every service is always shown,
        /// including absent ones.
        #[arg(short = 'a', long = "all", hide = true)]
        all: bool,
    },

    /// Show or follow logs.
    ///
    /// With a service name this is `container logs [flags] <project>-<svc>`.
    /// With no service it fans out across every service, prefixing and
    /// colouring each line by service the way `docker compose logs` does, and
    /// Ctrl-C tears down the whole group.
    ///
    /// Examples:
    ///   ac shop logs -f
    ///   ac shop logs -n 100 postgres
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

    /// Run a one-off container from a service definition, compose run style.
    ///
    /// Starts a fresh container from the service's image, env and volumes,
    /// named <project>-<svc>-run-<timestamp>, and removes it when the command
    /// exits. Published ports are NOT bound, so it never conflicts with the
    /// long-running service. Use `exec` instead to enter the container that
    /// is already running.
    ///
    /// Examples:
    ///   ac shop run postgres psql -U user -h shop-postgres
    ///   ac shop run redis sh
    ///   ac acplay run web --keep node --version
    Run {
        /// Keep the container after the command exits instead of removing it.
        #[arg(long)]
        keep: bool,
        /// Accepted for docker muscle memory; one-off containers are removed
        /// on exit by default (use --keep to retain).
        #[arg(long = "rm", hide = true)]
        rm_noop: bool,
        /// Accepted for docker muscle memory; interactivity is automatic.
        #[arg(short = 'i', hide = true)]
        interactive: bool,
        /// Accepted for docker muscle memory; a TTY is allocated when
        /// stdin and stdout are terminals.
        #[arg(short = 't', hide = true)]
        tty: bool,
        /// Extra KEY=VALUE environment entries, overriding the manifest.
        #[arg(short = 'e', long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// Do not attach the service's named volumes. Use this when the
        /// long-running service is up: a named volume is a block device and
        /// cannot be attached to two containers at once.
        #[arg(long)]
        no_volumes: bool,
        /// Service whose definition to run.
        service: String,
        /// Command to run. Empty means the image's default command.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },

    /// Create containers without starting them, compose create style.
    ///
    /// Runs `container create` with exactly the arguments `start` would use,
    /// so a later `start` brings them up in place. Creates missing named
    /// volumes first.
    Create {
        /// Recreate containers that already exist. Volumes and data survive.
        #[arg(long)]
        recreate: bool,
        /// Services to create. Empty means all of them.
        services: Vec<String>,
    },

    /// Processes running inside services, docker top style.
    ///
    /// Runs `ps aux` (falling back to plain `ps`) through `container exec`
    /// in each running service.
    Top {
        /// Services to show. Empty means every running one.
        services: Vec<String>,
    },

    /// Block until services are ready, then exit 0.
    ///
    /// A service with a readyCmd is polled through `container exec` until it
    /// exits 0; one without is waited on until its container is running.
    /// Exits non-zero on timeout, so scripts and agents can gate on it.
    ///
    /// Examples:
    ///   ac shop wait
    ///   ac shop wait postgres --timeout 30 && psql ...
    Wait {
        /// Seconds to wait per service before giving up. Defaults to each
        /// service's readyTimeout.
        #[arg(long)]
        timeout: Option<u64>,
        /// Services to wait for. Empty means all of them.
        services: Vec<String>,
    },

    /// Push already-built image tags, without rebuilding.
    ///
    /// Resolves the same tags `build` would produce for the profile, logs in
    /// to the registries those images come from, and runs
    /// `container image push` per tag. postPush hooks do NOT run here.
    ///
    /// Examples:
    ///   ac shop push --profile pre-prod
    ///   ac shop push web -P dev
    Push {
        /// Profile whose registry, account and tag template to use.
        #[arg(short = 'P', long)]
        profile: Option<String>,
        /// Builds whose tags to push. Empty means every build.
        names: Vec<String>,
    },

    /// Export a service's filesystem as a tar archive.
    ///
    /// Runs: container export -o <output> <project>-<svc>. Unlike docker,
    /// Apple container can only export a STOPPED container, so stop the
    /// service first.
    Export {
        /// Service to export.
        service: String,
        /// Output path. Defaults to <project>-<service>.tar in the current
        /// directory.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Run a command inside a service.
    ///
    /// Runs `container exec -i [-t] <project>-<svc> <command...>`. The `-t` is
    /// only added when stdin AND stdout are terminals, because Apple
    /// `container` fails with ENODEV otherwise.
    ///
    /// Example: ac shop exec postgres psql -U user -c 'select 1'
    Exec {
        /// Accepted for docker muscle memory; interactivity is detected
        /// automatically.
        #[arg(short = 'i', hide = true)]
        interactive: bool,
        /// Accepted for docker muscle memory; a TTY is allocated
        /// automatically when stdin and stdout are terminals.
        #[arg(short = 't', hide = true)]
        tty: bool,
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
    /// Example: ac shop sh redis
    #[command(alias = "shell")]
    Sh {
        /// Accepted for docker muscle memory; interactivity is automatic.
        #[arg(short = 'i', hide = true)]
        interactive: bool,
        /// Accepted for docker muscle memory; a TTY is allocated when
        /// stdin and stdout are terminals.
        #[arg(short = 't', hide = true)]
        tty: bool,
        /// Service to enter. Defaults to the first service in the manifest.
        service: Option<String>,
    },

    /// Live resource usage.
    ///
    /// Runs: container stats <containers...>
    ///
    /// --json implies --no-stream and emits one snapshot.
    Stats {
        /// Take one sample and exit instead of streaming, docker style.
        #[arg(long)]
        no_stream: bool,
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
    /// Example: ac shop cp ./dump.sql postgres:/tmp/dump.sql
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
    /// With no subcommand this lists them, so `ac shop images` keeps
    /// working. The subcommands act on the local image store.
    ///
    /// Examples:
    ///   ac shop images
    ///   ac shop images rm redis
    ///   ac shop images prune
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
    ///   ac shop volumes
    ///   ac shop volumes rm postgres-data
    ///   ac shop volumes prune
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
    ///   ac shop build
    ///   ac shop build web --profile dev
    ///   ac shop build --platform linux/amd64 --no-cache --sequential
    Build(BuildArgs),

    /// Roll out the images a profile already pushed, without rebuilding.
    ///
    /// Runs the profile's `rollout.preflight` hooks and then its `rollout.run`
    /// hooks, from the resolved build root. Each hook is argv with `{{...}}`
    /// interpolation, and receives the resolved image references in the
    /// environment (AC_IMAGE_<BUILD>, AC_IMAGES, AC_PROFILE, AC_TAG, ...), so
    /// the rollout logic itself lives in your repo rather than in ac.
    ///
    /// Use it to re-run a rollout that failed after a successful push, or to
    /// deploy an image someone else built.
    ///
    /// Examples:
    ///   ac shop rollout --profile prod
    ///   ac shop rollout web --profile pre-prod
    ///   ac shop rollout --profile prod --dry-run --json
    Rollout(RolloutArgs),

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
    ///   ac shop services
    ///   ac shop services --json
    Services,

    /// List the build names this project declares.
    ///
    /// Reads the manifest only, so it works with the daemon stopped. These are
    /// the names accepted by `ac <project> build`.
    ///
    /// Examples:
    ///   ac shop builds
    Builds,

    /// List the build profile names this project declares.
    ///
    /// Reads the manifest only, so it works with the daemon stopped. These are
    /// the values accepted by `ac <project> build --profile`.
    ///
    /// Examples:
    ///   ac shop profiles
    Profiles,

    /// List the custom scripts this project declares.
    ///
    /// A manifest may carry a `scripts` map of name to shell string. Running
    /// `ac <project> <name> [args...]` hands that string to `sh -c`, with any
    /// extra arguments appended shell-quoted (npm run style), so the script
    /// decides what its arguments mean. The script sees AC_PROJECT,
    /// AC_PROJECT_FILE and, when the manifest sets `root`, AC_PROJECT_ROOT.
    ///
    /// Reads the manifest only, so it works with the daemon stopped.
    ///
    /// Examples:
    ///   ac shop scripts
    ///   ac noveum forward status
    Scripts,

    #[command(external_subcommand)]
    Script(Vec<String>),
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

    /// Build output style. `plain` streams raw lines, `tty` hands the
    /// display to buildkit, `auto` picks the live renderer on a terminal.
    #[arg(long, value_parser = ["auto", "plain", "tty"])]
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

    /// Run the profile's rollout after every build and push succeeds.
    ///
    /// The rollout's own preflight hooks run FIRST, before anything is built,
    /// so an unreachable cluster fails in seconds rather than after a long
    /// build. Requires a profile that declares `rollout` and pushes.
    #[arg(long, overrides_with = "no_rollout")]
    pub rollout: bool,

    /// Never roll out, even if the profile sets `rollout.auto`.
    #[arg(long, overrides_with = "rollout")]
    pub no_rollout: bool,

    /// Resolve and print what would be built, without building or pushing.
    ///
    /// Touches nothing: no daemon, no builder, no registry login. Pair with
    /// --json to inspect the resolved plan from a script.
    #[arg(long)]
    pub dry_run: bool,

    /// Builds to run, by name. Empty means every build in the manifest.
    pub names: Vec<String>,
}

#[derive(Args, Debug)]
pub struct RolloutArgs {
    /// Profile whose rollout to run. Defaults to $AC_PROFILE, then `local`.
    #[arg(short = 'P', long)]
    pub profile: Option<String>,

    /// Roll out from this tree. Overrides every other root rule.
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// Resolve and print the hooks and their environment, running nothing.
    #[arg(long)]
    pub dry_run: bool,

    /// Builds whose images this rollout covers. Empty means every build.
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

    /// `--rollout` and `--no-rollout` collapse into a tri-state: `None` means
    /// the profile's `rollout.auto` decides.
    pub fn rollout_override(&self) -> Option<bool> {
        if self.rollout {
            Some(true)
        } else if self.no_rollout {
            Some(false)
        } else {
            None
        }
    }
}

/// Words that can never be interpreted as a project name in the
/// `ac <project> <action>` shorthand. Use `ac -p <name>` when a project really
/// is called one of these.

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
    ///   ac shop images rm redis
    ///   ac shop images rm
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
    ///   ac shop volumes rm postgres-data
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
