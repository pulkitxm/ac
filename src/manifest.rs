use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub type JsonMap = Map<String, Value>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
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

pub type JsonMapOf<T> = indexish::OrderedMap<T>;

pub mod indexish {
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
    #[serde(default)]
    pub cpus: Option<u32>,
    #[serde(default)]
    pub memory: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub push: Option<bool>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub registry: Option<String>,
    #[serde(default)]
    pub rollout: Option<Rollout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rollout {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub preflight: Vec<Vec<String>>,
    #[serde(default)]
    pub run: Vec<Vec<String>>,
    #[serde(default)]
    pub auto: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub server: String,
    #[serde(default = "default_username")]
    pub username: String,
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
    pub dockerfile: String,
    #[serde(default = "default_context")]
    pub context: String,
    pub image: String,
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
    #[serde(default)]
    pub preflight: Vec<Vec<String>>,
    #[serde(rename = "postPush", default)]
    pub post_push: Vec<Vec<String>>,
}

fn default_context() -> String {
    ".".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Volume {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub cpus: Option<u32>,
    #[serde(default)]
    pub memory: Option<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub env: JsonMap,
    #[serde(default)]
    pub volumes: Vec<Volume>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(rename = "readyCmd", default)]
    pub ready_cmd: Vec<String>,
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
    pub fn profile_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.profiles.keys().map(|k| k.to_string()).collect();
        v.sort();
        v
    }
}

pub struct Project {
    pub name: String,
    pub file: PathBuf,
    pub manifest: Manifest,
    pub raw: String,
}

impl Project {
    pub fn container_name(&self, svc: &str) -> String {
        format!("{}-{}", self.name, svc)
    }
    pub fn volume_name(&self, vol: &str) -> String {
        format!("{}-{}", self.name, vol)
    }

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
    validate(&manifest, file, name)?;
    Ok(Project {
        name: name.to_string(),
        file: file.to_path_buf(),
        manifest,
        raw,
    })
}

fn validate(manifest: &Manifest, file: &Path, name: &str) -> Result<()> {
    if manifest.name != name {
        return Err(anyhow!(
            "manifest {} declares name '{}' but the file is {name}.json; \
containers are named after the file, so rename one of them to match",
            file.display(),
            manifest.name
        ));
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for s in &manifest.services {
        if !seen.insert(s.name.as_str()) {
            return Err(anyhow!(
                "manifest {} declares the service '{}' more than once",
                file.display(),
                s.name
            ));
        }
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for b in &manifest.builds {
        if !seen.insert(b.name.as_str()) {
            return Err(anyhow!(
                "manifest {} declares the build '{}' more than once",
                file.display(),
                b.name
            ));
        }
    }
    Ok(())
}

pub fn load_all(config_dir: &Path, ac_home: &Path) -> Vec<Project> {
    project_names(config_dir, ac_home)
        .into_iter()
        .filter_map(|n| load_project(config_dir, ac_home, &n).ok())
        .collect()
}

pub fn json_scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(raw: &str, stem: &str) -> Result<Project> {
        let dir = std::env::temp_dir().join(format!("ac-manifest-test-{stem}"));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{stem}.json"));
        std::fs::write(&file, raw).unwrap();
        let out = load_project_file(&file, stem);
        std::fs::remove_dir_all(&dir).ok();
        out
    }

    #[test]
    fn name_must_match_the_file_name() {
        let err = load(r#"{"name":"other","services":[]}"#, "m1")
            .err()
            .unwrap();
        assert!(err.to_string().contains("'other'"), "{err}");
        assert!(err.to_string().contains("m1.json"), "{err}");
        assert!(load(r#"{"name":"m2","services":[]}"#, "m2").is_ok());
    }

    #[test]
    fn duplicate_service_and_build_names_are_rejected() {
        let err = load(
            r#"{"name":"m3","services":[
                {"name":"s","image":"a"},{"name":"s","image":"b"}]}"#,
            "m3",
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("service 's'"), "{err}");

        let err = load(
            r#"{"name":"m4","builds":[
                {"name":"b","dockerfile":"D","image":"i","tags":["t"]},
                {"name":"b","dockerfile":"D2","image":"i2","tags":["t"]}]}"#,
            "m4",
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("build 'b'"), "{err}");
    }
}
