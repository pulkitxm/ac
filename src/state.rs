//! Querying the daemon for container state.
//!
//! `container ls -a --format json` returns
//! `[{id, status:{state, networks:[{ipv4Address}]}}]`, which is everything the
//! rest of the tool needs. It is fetched once per logical operation rather than
//! once per service, so the echoed output stays readable.

use serde::Deserialize;

use crate::ctx::Ctx;

#[derive(Debug, Clone, Deserialize)]
struct RawNetwork {
    #[serde(rename = "ipv4Address")]
    ipv4_address: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawStatus {
    state: Option<String>,
    #[serde(default)]
    networks: Vec<RawNetwork>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawContainer {
    id: String,
    #[serde(default)]
    status: Option<RawStatus>,
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    /// `running`, `stopped`, `exited`, ... or `absent` when not present at all.
    pub state: String,
    pub ip: Option<String>,
}

/// A snapshot of every container the daemon knows about.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub items: Vec<ContainerInfo>,
}

impl Snapshot {
    /// Query the daemon. A failure (usually "daemon not running") yields an
    /// empty snapshot rather than an error, matching the bash behaviour where
    /// everything then reads as `absent`.
    pub fn query(ctx: &Ctx) -> Snapshot {
        Self::query_inner(ctx, false)
    }

    /// Same, without echoing the command. Used by polling loops.
    pub fn query_silent(ctx: &Ctx) -> Snapshot {
        Self::query_inner(ctx, true)
    }

    fn query_inner(ctx: &Ctx, silent: bool) -> Snapshot {
        let runner = ctx.container(["ls", "-a", "--format", "json"]);
        let runner = if silent { runner.silent() } else { runner };
        let Ok(text) = runner.stdout() else {
            return Snapshot::default();
        };
        let Ok(raw) = serde_json::from_str::<Vec<RawContainer>>(&text) else {
            return Snapshot::default();
        };
        Snapshot {
            items: raw
                .into_iter()
                .map(|c| {
                    let state = c
                        .status
                        .as_ref()
                        .and_then(|s| s.state.clone())
                        .unwrap_or_else(|| "unknown".to_string());
                    let ip = c
                        .status
                        .as_ref()
                        .and_then(|s| s.networks.first())
                        .and_then(|n| n.ipv4_address.clone());
                    ContainerInfo {
                        id: c.id,
                        state,
                        ip,
                    }
                })
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&ContainerInfo> {
        self.items.iter().find(|c| c.id == name)
    }

    pub fn state(&self, name: &str) -> String {
        self.get(name)
            .map(|c| c.state.clone())
            .unwrap_or_else(|| "absent".to_string())
    }

    /// The IP of a running container. Stopped containers have no address.
    pub fn ip(&self, name: &str) -> Option<String> {
        self.get(name)
            .filter(|c| c.state == "running")
            .and_then(|c| c.ip.clone())
    }

    pub fn running_names(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter(|c| c.state == "running")
            .map(|c| c.id.as_str())
            .collect()
    }
}

/// Every container belonging to any known project that is currently running.
/// This is what the supervisor counts to decide when the daemon can go away,
/// which is why it spans all projects rather than just the current one.
pub fn ac_running_containers(ctx: &Ctx, silent: bool) -> Vec<String> {
    let projects = crate::manifest::load_all(&ctx.config_dir, &ctx.ac_home);
    let mut owned: Vec<String> = Vec::new();
    for p in &projects {
        for s in &p.manifest.services {
            owned.push(format!("{}-{}", p.name, s.name));
        }
    }
    if owned.is_empty() {
        return Vec::new();
    }
    let snap = if silent {
        Snapshot::query_silent(ctx)
    } else {
        Snapshot::query(ctx)
    };
    snap.running_names()
        .into_iter()
        .filter(|n| owned.iter().any(|o| o == n))
        .map(|s| s.to_string())
        .collect()
}
