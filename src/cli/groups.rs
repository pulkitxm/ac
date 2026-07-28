use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum BuilderAction {
    /// Show the builder container's status.
    ///
    /// Runs: container builder status
    Status,
    /// Start the builder container.
    ///
    /// Runs: container builder start
    Start {
        /// CPUs for the builder. Applied only at creation.
        #[arg(short = 'c', long)]
        cpus: Option<u32>,
        /// Memory for the builder. Applied only at creation.
        #[arg(short = 'm', long)]
        memory: Option<String>,
    },
    /// Stop the builder container.
    ///
    /// Runs: container builder stop
    Stop,
    /// Delete the builder container, discarding its layer cache.
    ///
    /// Runs: container builder delete
    #[command(alias = "rm")]
    Delete {
        /// Delete the builder even if it is running.
        #[arg(short, long)]
        force: bool,
    },
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

#[derive(Debug, Subcommand)]
pub enum ImageAction {
    /// List every image in the local store, with sizes, docker images style.
    ///
    /// Runs: container image ls --verbose (--format json with --json)
    #[command(alias = "list")]
    Ls {
        /// Accepted for docker muscle memory; sizes are already shown.
        #[arg(short = 'v', long, hide = true)]
        verbose: bool,
        /// Only print image names, docker images -q style.
        #[arg(short = 'q')]
        ids: bool,
    },

    /// Pull an image by full OCI reference.
    ///
    /// Runs: container image pull <reference>
    ///
    /// Example: ac image pull docker.io/library/alpine:3.20
    Pull {
        /// Full reference, including the registry host.
        reference: String,
        /// Platform, e.g. linux/amd64. Defaults to the daemon's default.
        #[arg(long)]
        platform: Option<String>,
    },

    /// Push an image by full OCI reference.
    ///
    /// Runs: container image push <reference>
    Push {
        /// Full reference, including the registry host.
        reference: String,
        /// Platform to push when the local image is multi-platform.
        #[arg(long)]
        platform: Option<String>,
    },

    /// Remove images from the local store.
    ///
    /// Runs: container image rm <references...>
    #[command(alias = "delete", alias = "remove")]
    Rm {
        /// Accepted for docker muscle memory; removal never prompts anyway.
        #[arg(short = 'f', long, hide = true)]
        force: bool,
        /// References to remove.
        #[arg(required = true)]
        references: Vec<String>,
    },

    /// Give an existing image an additional reference.
    ///
    /// Runs: container image tag <source> <target>
    ///
    /// Example: ac image tag myapp:dev-local 123.dkr.ecr.us-east-1.amazonaws.com/myapp:dev
    Tag {
        /// Existing reference.
        source: String,
        /// New reference to create.
        target: String,
    },

    /// Full image JSON, straight from the daemon.
    ///
    /// Runs: container image inspect <references...>
    Inspect {
        /// References to inspect.
        #[arg(required = true)]
        references: Vec<String>,
    },

    /// Remove dangling images, or every unused one with --all.
    ///
    /// Runs: container image prune [--all]
    Prune {
        /// Remove all unused images, not just dangling ones.
        #[arg(short, long)]
        all: bool,
    },

    /// Save images to an OCI tar archive.
    ///
    /// Runs: container image save -o <output> <references...>
    ///
    /// Example: ac image save -o backup.tar myapp:dev-local
    Save {
        /// References to save.
        #[arg(required = true)]
        references: Vec<String>,
        /// Path for the archive.
        #[arg(short, long)]
        output: PathBuf,
        /// Platform to save for multi-platform images.
        #[arg(long)]
        platform: Option<String>,
    },

    /// Load images from an OCI tar archive.
    ///
    /// Runs: container image load -i <input>
    Load {
        /// Archive to load.
        #[arg(short, long)]
        input: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum VolumeAction {
    /// List every volume the daemon knows about.
    ///
    /// Runs: container volume ls (--format json with --json)
    #[command(alias = "list")]
    Ls,

    /// Create a named volume.
    ///
    /// Runs: container volume create <name>
    ///
    /// Volumes are ext4 block devices; a fresh one already contains
    /// lost+found.
    Create {
        /// Volume name.
        name: String,
    },

    /// Delete volumes. THIS DESTROYS THE DATA IN THEM.
    ///
    /// Runs: container volume rm <names...>
    #[command(alias = "delete", alias = "remove")]
    Rm {
        /// Volumes to delete.
        #[arg(required = true)]
        names: Vec<String>,
    },

    /// Full volume JSON, straight from the daemon.
    ///
    /// Runs: container volume inspect <names...>
    Inspect {
        /// Volumes to inspect.
        #[arg(required = true)]
        names: Vec<String>,
    },

    /// Remove volumes no container references.
    ///
    /// Runs: container volume prune
    Prune,
}

#[derive(Debug, Subcommand)]
pub enum NetworkAction {
    /// List networks.
    ///
    /// Runs: container network ls (--format json with --json)
    #[command(alias = "list")]
    Ls,

    /// Create a network.
    ///
    /// Runs: container network create <name>. Requires macOS 26 or newer.
    Create {
        /// Network name.
        name: String,
        /// Restrict to host-only networking.
        #[arg(long)]
        internal: bool,
        /// IPv4 subnet, e.g. 192.168.66.0/24.
        #[arg(long)]
        subnet: Option<String>,
    },

    /// Delete networks.
    ///
    /// Runs: container network rm <names...>
    #[command(alias = "delete", alias = "remove")]
    Rm {
        /// Networks to delete.
        #[arg(required = true)]
        names: Vec<String>,
    },

    /// Full network JSON, straight from the daemon.
    ///
    /// Runs: container network inspect <names...>
    Inspect {
        /// Networks to inspect.
        #[arg(required = true)]
        names: Vec<String>,
    },

    /// Remove networks with no container connections.
    ///
    /// Runs: container network prune
    Prune,
}

#[derive(Debug, Subcommand)]
pub enum SystemAction {
    /// Daemon state, ownership, and the supervisor, plus appRoot.
    ///
    /// The same information as `ac daemon status`.
    Info,

    /// Disk usage for images, containers and volumes.
    ///
    /// Runs: container system df (--format json with --json)
    Df,

    /// Start the daemon if it is not already running.
    ///
    /// A daemon started here is owned by ac and will be stopped once the
    /// last ac-managed container is gone. A daemon that was already running
    /// is left exactly as it is.
    Start,

    /// Stop the daemon, but only if ac started it.
    ///
    /// Does nothing when the daemon was already running before ac was
    /// involved, by design. Same as `ac daemon stop`.
    Stop,

    /// Remove stopped containers and unused images.
    ///
    /// Runs `container prune` then `container image prune`, then re-checks
    /// whether the daemon can be released. Same as `ac prune`.
    Prune {
        /// Also remove every unused image, docker system prune -a style.
        #[arg(short, long)]
        all: bool,
    },

    /// Logs from the `container` system services themselves.
    ///
    /// Runs: container system logs [-f] [--last <period>]
    Logs {
        /// Follow log output.
        #[arg(short, long)]
        follow: bool,
        /// How far back to fetch, e.g. 10m, 2h, 1d.
        #[arg(long)]
        last: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum RegistryAction {
    /// Log in to a registry, prompting for the password.
    ///
    /// Runs: container registry login [-u <user>] [--password-stdin] <server>
    ///
    /// Examples:
    ///   ac registry login -u me ghcr.io
    ///   aws ecr get-login-password | ac registry login -u AWS --password-stdin <acct>.dkr.ecr.us-east-1.amazonaws.com
    Login {
        /// Registry server host.
        server: String,
        /// Registry user name.
        #[arg(short, long)]
        username: Option<String>,
        /// Read the password from stdin instead of prompting.
        #[arg(long)]
        password_stdin: bool,
    },

    /// Log out from a registry.
    ///
    /// Runs: container registry logout <server>
    Logout {
        /// Registry server host.
        server: String,
    },

    /// List stored registry logins.
    ///
    /// Runs: container registry ls
    Ls,
}
