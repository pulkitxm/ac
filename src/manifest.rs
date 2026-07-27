//! The typed project manifest.
//!
//! A project is a JSON file in `~/.config/ac/projects/` or `<repo>/projects/`.
//! Every struct here denies unknown fields, so a typo produces an error that
//! names both the bad field and the valid alternatives instead of being
//! silently ignored.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Ordered string map. `preserve_order` on serde_json keeps manifest order.
pub type JsonMap = Map<String, Value>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Project name. Conventionally matches the file name.
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Default directory builds run from when nothing better can be inferred.
    #[serde(default)]
    pub root: Option<String>,
    /// Default `{{region}}` value. Defaults to `us-east-1`.
    #[serde(default)]
    pub region: Option<String>,
    /// Sizing for the shared buildkit builder container.
    #[serde(default)]
    pub builder: Option<Builder>,
    #[serde(default)]
    pub profiles: JsonMapOf<Profile>,
    #[serde(default)]
    pub registries: Vec<Registry>,
    #[serde(default)]
    pub builds: Vec<Build>,
    #[serde(default)]
    pub services: Vec<Service>,
}

/// An insertion-ordered map with typed values.
pub type JsonMapOf<T> = indexish::OrderedMap<T>;

pub mod indexish {
    //! A tiny insertion-ordered map built on `serde_json::Map`, which keeps
    //! order thanks to the `preserve_order` feature. Avoids taking a direct
    //! dependency on `indexmap` for a single field.
    use serde::de::DeserializeOwned;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::{Map, Value};

    #[derive(Debug, Clone, Default)]
    pub struct OrderedMap<T>(pub Vec<(String, T)>);

    impl<T> OrderedMap<T> {
        pub fn get(&self, k: &str) -> Option<&T> {
            self.0.iter().find(|(n, _)| n == k).map(|(_, v)| v)
        }
        pub fn keys(&self) -> impl Iterator<Item = &str> {
            self.0.iter().map(|(k, _)| k.as_str())
        }
    }

    impl<'de, T: DeserializeOwned> Deserialize<'de> for OrderedMap<T> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let raw = Map::<String, Value>::deserialize(d)?;
            let mut out = Vec::with_capacity(raw.len());
            for (k, v) in raw {
                let t = T::deserialize(v).map_err(serde::de::Error::custom)?;
                out.push((k, t));
            }
            Ok(OrderedMap(out))
        }
    }

    impl<T: Serialize> Serialize for OrderedMap<T> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut m = s.serialize_map(Some(self.0.len()))?;
            for (k, v) in &self.0 {
                m.serialize_entry(k, v)?;
            }
            m.end()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Builder {
    /// vCPUs given to the buildkit builder when it is created.
    #[serde(default)]
    pub cpus: Option<u32>,
    /// Memory given to the buildkit builder when it is created, e.g. "8g".
    #[serde(default)]
    pub memory: Option<String>,
}

/// A named build target such as `local`, `dev` or `prod`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// `{{profile}}` platform override, e.g. `linux/amd64`.
    #[serde(default)]
    pub platform: Option<String>,
    /// Whether images built for this profile are pushed.
    #[serde(default)]
    pub push: Option<bool>,
    /// Value of `{{tag}}`.
    #[serde(default)]
    pub tag: Option<String>,
    /// Value of `{{account}}`.
    #[serde(default)]
    pub account: Option<String>,
    /// Value of `{{region}}`, overriding the project default.
    #[serde(default)]
    pub region: Option<String>,
    /// Value of `{{registry}}`. Usually a host plus a trailing slash, and empty
    /// for purely local profiles so the same image template yields `app:tag`.
    #[serde(default)]
    pub registry: Option<String>,
}

/// A private registry to authenticate against before pulling or pushing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    /// Registry host. Supports `{{...}}` interpolation.
    pub server: String,
    #[serde(default = "default_username")]
    pub username: String,
    /// argv executed and piped to `container registry login --password-stdin`.
    /// Credentials are never stored in the manifest.
    #[serde(rename = "passwordCmd")]
    pub password_cmd: Vec<String>,
}

fn default_username() -> String {
    "AWS".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secret {
    pub id: String,
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub src: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub name: String,
    /// Path to the Dockerfile, relative to the resolved build root.
    pub dockerfile: String,
    /// Build context, relative to the resolved build root. Defaults to ".".
    #[serde(default = "default_context")]
    pub context: String,
    /// Image repository. Supports `{{...}}`, typically `{{registry}}name`.
    pub image: String,
    /// Tags appended to `image`. Each supports `{{...}}`.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(rename = "buildArgs", default)]
    pub build_args: JsonMap,
    #[serde(default)]
    pub labels: JsonMap,
    #[serde(default)]
    pub secrets: Vec<Secret>,
    /// argv arrays run from the build root before building. A failure aborts.
    #[serde(default)]
    pub preflight: Vec<Vec<String>>,
    /// argv arrays run from the build root after a successful push. A failure
    /// aborts and is reported as an error.
    #[serde(rename = "postPush", default)]
    pub post_push: Vec<Vec<String>>,
}

fn default_context() -> String {
    ".".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Volume {
    /// Logical name. The real volume is `<project>-<name>`.
    pub name: String,
    /// Mount point inside the container.
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    /// Full OCI reference, including the registry host.
    pub image: String,
    /// Sizes the container's VM, not a cgroup. Each container is its own VM.
    #[serde(default)]
    pub cpus: Option<u32>,
    /// Memory for the container's VM, e.g. "1g".
    #[serde(default)]
    pub memory: Option<String>,
    /// `host:container` port publications.
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub env: JsonMap,
    #[serde(default)]
    pub volumes: Vec<Volume>,
    /// Extra argv appended after the image reference.
    #[serde(default)]
    pub args: Vec<String>,
    /// Polled through `container exec` until it exits 0. Apple `container` has
    /// no healthcheck primitive, so readiness is implemented here.
    #[serde(rename = "readyCmd", default)]
    pub ready_cmd: Vec<String>,
    /// Seconds before giving up on `readyCmd`. Start continues with a warning.
    #[serde(rename = "readyTimeout", default = "default_ready_timeout")]
    pub ready_timeout: u64,
}

fn default_ready_timeout() -> u64 {
    90
}

impl Manifest {
    pub fn service(&self, name: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.name == name)
    }
    pub fn build(&self, name: &str) -> Option<&Build> {
        self.builds.iter().find(|b| b.name == name)
    }
    pub fn service_names(&self) -> Vec<String> {
        self.services.iter().map(|s| s.name.clone()).collect()
    }
    pub fn build_names(&self) -> Vec<String> {
        self.builds.iter().map(|b| b.name.clone()).collect()
    }
}

/// A manifest plus where it came from.
pub struct Project {
    pub name: String,
    pub file: PathBuf,
    pub manifest: Manifest,
    /// The manifest exactly as written, for `ac <project> config`.
    pub raw: String,
}

impl Project {
    pub fn container_name(&self, svc: &str) -> String {
        format!("{}-{}", self.name, svc)
    }
    pub fn volume_name(&self, vol: &str) -> String {
        format!("{}-{}", self.name, vol)
    }

    /// Accept either the bare service name (`redis`) or the container name
    /// (`noveum-redis`), since the latter is what `ac <p> ls` prints.
    pub fn normalize_service(&self, name: &str) -> String {
        name.strip_prefix(&format!("{}-", self.name))
            .unwrap_or(name)
            .to_string()
    }

    pub fn has_service(&self, name: &str) -> bool {
        self.manifest
            .service(&self.normalize_service(name))
            .is_some()
    }

    /// Services an action applies to: all of them when none are named,
    /// otherwise just the named ones, validated so a typo fails loudly.
    pub fn target_services(&self, names: &[String]) -> Result<Vec<String>> {
        if names.is_empty() {
            return Ok(self.manifest.service_names());
        }
        let mut out = Vec::new();
        for n in names {
            let norm = self.normalize_service(n);
            if self.manifest.service(&norm).is_none() {
                return Err(anyhow!(
                    "no such service '{}' in project '{}' (have: {})",
                    n,
                    self.name,
                    self.manifest.service_names().join(" ")
                ));
            }
            out.push(norm);
        }
        Ok(out)
    }

    pub fn target_container_names(&self, names: &[String]) -> Result<Vec<String>> {
        Ok(self
            .target_services(names)?
            .iter()
            .map(|s| self.container_name(s))
            .collect())
    }
}

/// Directories searched for manifests, highest priority first. User projects in
/// `~/.config/ac/projects` shadow the ones bundled in the repo, so the repo
/// stays cleanly updatable while remaining customisable.
pub fn project_dirs(config_dir: &Path, ac_home: &Path) -> Vec<PathBuf> {
    vec![config_dir.join("projects"), ac_home.join("projects")]
}

pub fn project_names(config_dir: &Path, ac_home: &Path) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for d in project_dirs(config_dir, ac_home) {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("json") {
                if let Some(stem) = p.file_stem().and_then(|x| x.to_str()) {
                    set.insert(stem.to_string());
                }
            }
        }
    }
    set.into_iter().collect()
}

pub fn project_file(config_dir: &Path, ac_home: &Path, name: &str) -> Option<PathBuf> {
    for d in project_dirs(config_dir, ac_home) {
        let f = d.join(format!("{name}.json"));
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

pub fn load_project(config_dir: &Path, ac_home: &Path, name: &str) -> Result<Project> {
    let file = project_file(config_dir, ac_home, name)
        .ok_or_else(|| anyhow!("unknown project: {name} (try: ac ls)"))?;
    load_project_file(&file, name)
}

pub fn load_project_file(file: &Path, name: &str) -> Result<Project> {
    let raw = fs::read_to_string(file).map_err(|e| anyhow!("reading {}: {e}", file.display()))?;
    let manifest: Manifest = serde_json::from_str(&raw).map_err(|e| {
        anyhow!(
            "invalid manifest {} at line {} column {}: {}",
            file.display(),
            e.line(),
            e.column(),
            e
        )
    })?;
    Ok(Project {
        name: name.to_string(),
        file: file.to_path_buf(),
        manifest,
        raw,
    })
}

/// Load every discoverable project, skipping the ones that fail to parse so a
/// single broken manifest cannot break daemon refcounting.
pub fn load_all(config_dir: &Path, ac_home: &Path) -> Vec<Project> {
    project_names(config_dir, ac_home)
        .into_iter()
        .filter_map(|n| load_project(config_dir, ac_home, &n).ok())
        .collect()
}

/// Render a JSON value the way `jq -r` would: strings bare, everything else as
/// compact JSON. Used for env values, build args and labels.
pub fn json_scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
