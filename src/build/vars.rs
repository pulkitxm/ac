use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::core::ctx::{now_stamp, Ctx};
use crate::manifest::Project;

#[derive(Debug, Clone, Default)]
pub struct BuildOverrides {
    pub profile: Option<String>,
    pub root: Option<PathBuf>,
    pub platform: Option<String>,
    pub push: Option<bool>,
    pub no_cache: bool,
    pub progress: Option<String>,
    pub target: Option<String>,
    pub builder_cpus: Option<u32>,
    pub builder_memory: Option<String>,
    pub sequential: bool,
    pub dry_run: bool,
    pub rollout: Option<bool>,
}

impl BuildOverrides {
    pub fn profile_name(&self) -> String {
        self.profile
            .clone()
            .or_else(|| std::env::var("AC_PROFILE").ok().filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "local".to_string())
    }
}

pub fn resolve_root(ctx: &Ctx, proj: &Project, ov: &BuildOverrides) -> Result<PathBuf> {
    if let Some(r) = &ov.root {
        return std::fs::canonicalize(r)
            .map_err(|_| anyhow!("--root does not exist: {}", r.display()));
    }
    if let Ok(r) = std::env::var("AC_ROOT") {
        if !r.is_empty() {
            return std::fs::canonicalize(&r).map_err(|_| anyhow!("AC_ROOT does not exist: {r}"));
        }
    }

    let manifest_root = proj.manifest.root.clone().unwrap_or_default();
    let cwd = std::env::current_dir()?;

    match git_toplevel(&cwd) {
        Some(top) => match proj.manifest.builds.first().map(|b| b.dockerfile.clone()) {
            Some(m) if !m.is_empty() => {
                if top.join(&m).exists() {
                    return Ok(top);
                }
            }
            _ => {
                if !manifest_root.is_empty()
                    && top.file_name() == Path::new(&manifest_root).file_name()
                {
                    return Ok(top);
                }
            }
        },
        None => {
            if !proj.manifest.builds.is_empty()
                && proj
                    .manifest
                    .builds
                    .iter()
                    .all(|b| cwd.join(&b.dockerfile).exists())
            {
                return Ok(cwd);
            }
        }
    }

    if !manifest_root.is_empty() {
        if let Ok(abs) = std::fs::canonicalize(&manifest_root) {
            return Ok(abs);
        }
        ctx.warn(&format!("manifest root does not exist: {manifest_root}"));
    }
    Ok(cwd)
}

fn git_toplevel(dir: &Path) -> Option<PathBuf> {
    crate::core::ctx::echo_external("git", &["rev-parse", "--show-toplevel"]);
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Vars {
    pub profile: String,
    pub account: String,
    pub tag: String,
    pub region: String,
    pub registry: String,
    pub version: String,
    pub git_sha: String,
    pub git_short_sha: String,
    pub git_branch: String,
    pub git_dirty_suffix: String,
    pub timestamp: String,
    pub images: BTreeMap<String, Vec<String>>,
}

pub fn vars_for(proj: &Project, profile: &str, root: &Path) -> Vars {
    let p = proj.manifest.profiles.get(profile);
    let account = p.and_then(|x| x.account.clone()).unwrap_or_default();
    let tag = p.and_then(|x| x.tag.clone()).unwrap_or_default();
    let region = p
        .and_then(|x| x.region.clone())
        .or_else(|| proj.manifest.region.clone())
        .unwrap_or_else(|| "us-east-1".to_string());

    let registry = p
        .and_then(|x| x.registry.clone())
        .unwrap_or_default()
        .replace("{{account}}", &account)
        .replace("{{region}}", &region);

    let mut v = Vars {
        profile: profile.to_string(),
        account,
        tag,
        region,
        registry,
        version: "0.0.0".to_string(),
        timestamp: now_stamp(),
        ..Default::default()
    };

    if git_dir_ok(root) {
        v.git_sha = git(root, &["rev-parse", "HEAD"]);
        v.git_short_sha = git(root, &["rev-parse", "--short", "HEAD"]);
        v.git_branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]);
        if !git(root, &["status", "--porcelain"]).is_empty() {
            v.git_dirty_suffix = format!("-local-{}", v.timestamp);
        }
    }

    if let Ok(text) = std::fs::read_to_string(root.join("package.json")) {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(ver) = j.get("version").and_then(|x| x.as_str()) {
                v.version = ver.to_string();
            }
        }
    }
    v.images = resolve_images(proj, &v);
    v
}

fn resolve_images(proj: &Project, v: &Vars) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    for b in &proj.manifest.builds {
        let image = interpolate(&b.image, v);
        let tags: Vec<String> = b
            .tags
            .iter()
            .filter(|t| !t.is_empty())
            .map(|t| format!("{image}:{}", interpolate(t, v)))
            .collect();
        if !tags.is_empty() {
            map.insert(b.name.clone(), tags);
        }
    }
    map
}

pub fn absolute_against(path: &str, root: &Path) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        return path.to_string();
    }
    let joined = root.join(p);
    match std::fs::canonicalize(&joined) {
        Ok(c) => c.display().to_string(),
        Err(_) => joined.display().to_string(),
    }
}

pub(crate) fn absolute_prog(prog: &str, root: &Path) -> String {
    if prog.contains('/') {
        absolute_against(prog, root)
    } else {
        prog.to_string()
    }
}

pub fn env_key(prefix: &str, name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("{prefix}{cleaned}")
}

pub fn hook_env(proj: &Project, v: &Vars, root: &Path, builds: &[String]) -> Vec<(String, String)> {
    let mut env = vec![
        ("AC_PROJECT".to_string(), proj.name.clone()),
        ("AC_PROFILE".to_string(), v.profile.clone()),
        ("AC_ACCOUNT".to_string(), v.account.clone()),
        ("AC_REGION".to_string(), v.region.clone()),
        ("AC_REGISTRY".to_string(), v.registry.clone()),
        ("AC_TAG".to_string(), v.tag.clone()),
        ("AC_VERSION".to_string(), v.version.clone()),
        ("AC_ROOT".to_string(), root.display().to_string()),
        ("AC_GIT_SHA".to_string(), v.git_sha.clone()),
        ("AC_GIT_SHORT_SHA".to_string(), v.git_short_sha.clone()),
        ("AC_GIT_BRANCH".to_string(), v.git_branch.clone()),
        (
            "AC_GIT_DIRTY".to_string(),
            if v.git_dirty_suffix.is_empty() {
                "0".to_string()
            } else {
                "1".to_string()
            },
        ),
        ("AC_TIMESTAMP".to_string(), v.timestamp.clone()),
        ("AC_BUILDS".to_string(), builds.join(" ")),
        (
            "AC_QUIET".to_string(),
            if crate::core::ctx::is_quiet() {
                "1"
            } else {
                "0"
            }
            .to_string(),
        ),
    ];

    let mut all: Vec<String> = Vec::new();
    for (name, tags) in &v.images {
        if let Some(first) = tags.first() {
            env.push((env_key("AC_IMAGE_", name), first.clone()));
        }
        env.push((env_key("AC_IMAGES_", name), tags.join(" ")));
        if builds.iter().any(|b| b == name) {
            all.extend(tags.iter().cloned());
        }
    }
    env.push(("AC_IMAGES".to_string(), all.join(" ")));
    env
}

fn git_dir_ok(root: &Path) -> bool {
    crate::core::ctx::echo_external(
        "git",
        &["-C", &root.to_string_lossy(), "rev-parse", "--git-dir"],
    );
    std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "rev-parse", "--git-dir"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git(root: &Path, args: &[&str]) -> String {
    let mut a = vec!["-C".to_string(), root.to_string_lossy().to_string()];
    a.extend(args.iter().map(|s| s.to_string()));
    crate::core::ctx::echo_external("git", &a);
    std::process::Command::new("git")
        .args(&a)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn interpolate(s: &str, v: &Vars) -> String {
    if !s.contains("{{") {
        return s.to_string();
    }
    let mut s = s.to_string();
    for (name, tags) in &v.images {
        if let Some(first) = tags.first() {
            s = s.replace(&format!("{{{{image.{name}}}}}"), first);
        }
    }
    s.replace("{{profile}}", &v.profile)
        .replace("{{account}}", &v.account)
        .replace("{{tag}}", &v.tag)
        .replace("{{region}}", &v.region)
        .replace("{{registry}}", &v.registry)
        .replace("{{version}}", &v.version)
        .replace("{{git.sha}}", &v.git_sha)
        .replace("{{git.shortSha}}", &v.git_short_sha)
        .replace("{{git.branch}}", &v.git_branch)
        .replace("{{git.dirtySuffix}}", &v.git_dirty_suffix)
        .replace("{{timestamp}}", &v.timestamp)
}
