use clap::Args;

#[derive(Args, Debug, Default)]
pub struct RunOpts {
    /// Run the container in the background and print its name.
    #[arg(short = 'd', long)]
    pub detach: bool,
    /// Name for the container.
    #[arg(long)]
    pub name: Option<String>,
    /// Environment entry, KEY=VALUE or bare KEY to inherit from the host.
    #[arg(short = 'e', long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,
    /// File of KEY=VALUE environment entries.
    #[arg(long = "env-file", value_name = "PATH")]
    pub env_file: Vec<String>,
    /// Publish a port, [host-ip:]host-port:container-port[/protocol].
    #[arg(short = 'p', long = "publish", value_name = "SPEC")]
    pub publish: Vec<String>,
    /// Publish a socket, host_path:container_path.
    #[arg(long = "publish-socket", value_name = "SPEC")]
    pub publish_socket: Vec<String>,
    /// Bind mount a volume, source:target.
    #[arg(short = 'v', long = "volume", value_name = "SPEC")]
    pub volume: Vec<String>,
    /// Add a mount, type=<>,source=<>,target=<>,readonly.
    #[arg(long = "mount", value_name = "SPEC")]
    pub mount: Vec<String>,
    /// Add a tmpfs mount at the given path.
    #[arg(long = "tmpfs", value_name = "PATH")]
    pub tmpfs: Vec<String>,
    /// Size of /dev/shm, e.g. 64M.
    #[arg(long = "shm-size", value_name = "SIZE")]
    pub shm_size: Option<String>,
    /// Container label, repeatable.
    #[arg(short = 'l', long = "label", value_name = "KEY=VALUE")]
    pub label: Vec<String>,
    /// CPUs to allocate. This sizes the container's VM, not a cgroup.
    #[arg(short = 'c', long = "cpus")]
    pub cpus: Option<u32>,
    /// Memory to allocate, with optional K, M, G, T or P suffix.
    #[arg(short = 'm', long = "memory")]
    pub memory: Option<String>,
    /// Keep stdin open.
    #[arg(short = 'i', long)]
    pub interactive: bool,
    /// Request a TTY. Honoured only when stdin and stdout are terminals.
    #[arg(short = 't', long)]
    pub tty: bool,
    /// User for the process, name|uid[:gid].
    #[arg(short = 'u', long)]
    pub user: Option<String>,
    /// User ID for the process.
    #[arg(long)]
    pub uid: Option<String>,
    /// Group ID for the process.
    #[arg(long)]
    pub gid: Option<String>,
    /// Initial working directory inside the container.
    #[arg(short = 'w', long = "workdir", visible_alias = "cwd")]
    pub workdir: Option<String>,
    /// Resource limit, <type>=<soft>[:<hard>].
    #[arg(long = "ulimit", value_name = "LIMIT")]
    pub ulimit: Vec<String>,
    /// Override the image entrypoint.
    #[arg(long)]
    pub entrypoint: Option<String>,
    /// Attach to a network, <name>[,mac=..][,mtu=..].
    #[arg(long)]
    pub network: Option<String>,
    /// Platform for a multi-platform image, os/arch[/variant].
    #[arg(long)]
    pub platform: Option<String>,
    /// Architecture for a multi-arch image. --platform wins.
    #[arg(short = 'a', long)]
    pub arch: Option<String>,
    /// OS for a multi-OS image. --platform wins.
    #[arg(long)]
    pub os: Option<String>,
    /// Mount the root filesystem read-only.
    #[arg(long = "read-only")]
    pub read_only: bool,
    /// Add a Linux capability, e.g. CAP_NET_RAW or ALL.
    #[arg(long = "cap-add", value_name = "CAP")]
    pub cap_add: Vec<String>,
    /// Drop a Linux capability.
    #[arg(long = "cap-drop", value_name = "CAP")]
    pub cap_drop: Vec<String>,
    /// Run an init process that forwards signals and reaps children.
    #[arg(long)]
    pub init: bool,
    /// Custom init image.
    #[arg(long = "init-image", value_name = "IMAGE")]
    pub init_image: Option<String>,
    /// Custom kernel path.
    #[arg(short = 'k', long = "kernel", value_name = "PATH")]
    pub kernel: Option<String>,
    /// Runtime handler.
    #[arg(long)]
    pub runtime: Option<String>,
    /// DNS nameserver IP address.
    #[arg(long = "dns", value_name = "IP")]
    pub dns: Vec<String>,
    /// Default DNS domain.
    #[arg(long = "dns-domain", value_name = "DOMAIN")]
    pub dns_domain: Option<String>,
    /// DNS option.
    #[arg(long = "dns-option", value_name = "OPTION")]
    pub dns_option: Vec<String>,
    /// DNS search domain.
    #[arg(long = "dns-search", value_name = "DOMAIN")]
    pub dns_search: Vec<String>,
    /// Do not configure DNS in the container.
    #[arg(long = "no-dns")]
    pub no_dns: bool,
    /// Forward the SSH agent socket into the container.
    #[arg(long)]
    pub ssh: bool,
    /// Enable Rosetta in the container.
    #[arg(long)]
    pub rosetta: bool,
    /// Expose virtualization capabilities to the container.
    #[arg(long)]
    pub virtualization: bool,
    /// Write the container ID to this path.
    #[arg(long = "cidfile", value_name = "PATH")]
    pub cidfile: Option<String>,
    /// Registry scheme: http, https or auto.
    #[arg(long)]
    pub scheme: Option<String>,
    /// Progress output style.
    #[arg(long)]
    pub progress: Option<String>,
    /// Maximum concurrent image layer downloads.
    #[arg(long = "max-concurrent-downloads", value_name = "N")]
    pub max_concurrent_downloads: Option<u32>,
}
