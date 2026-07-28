use serde::Deserialize;

use crate::core::ctx::Ctx;

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

#[derive(Debug, Clone, Default, Deserialize)]
struct RawConfiguration {
    #[serde(default)]
    labels: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawContainer {
    id: String,
    #[serde(default)]
    status: Option<RawStatus>,
    #[serde(default)]
    configuration: Option<RawConfiguration>,
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub state: String,
    pub ip: Option<String>,
    pub ac_managed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub items: Vec<ContainerInfo>,
}

impl Snapshot {
    pub fn query(ctx: &Ctx) -> Snapshot {
        Self::query_inner(ctx, false)
    }

    pub fn query_silent(ctx: &Ctx) -> Snapshot {
        Self::query_inner(ctx, true)
    }

    fn query_inner(ctx: &Ctx, silent: bool) -> Snapshot {
        let runner = ctx.container(["ls", "-a", "--format", "json"]);
        let runner = if silent { runner.echo_once() } else { runner };
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
                    let labels = c.configuration.as_ref().map(|cfg| &cfg.labels);
                    let ac_managed = labels
                        .map(|l| l.contains_key("ac.managed") || l.contains_key("ac.project"))
                        .unwrap_or(false);
                    ContainerInfo {
                        id: c.id,
                        state,
                        ip,
                        ac_managed,
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

pub fn ac_running_containers(ctx: &Ctx, silent: bool) -> Vec<String> {
    let projects = crate::manifest::load_all(&ctx.config_dir, &ctx.ac_home);
    let mut owned: Vec<String> = Vec::new();
    for p in &projects {
        for s in &p.manifest.services {
            owned.push(format!("{}-{}", p.name, s.name));
        }
    }
    let snap = if silent {
        Snapshot::query_silent(ctx)
    } else {
        Snapshot::query(ctx)
    };
    snap.items
        .iter()
        .filter(|c| c.state == "running")
        .filter(|c| c.ac_managed || owned.iter().any(|o| o == &c.id))
        .map(|c| c.id.clone())
        .collect()
}
