use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::ctx::{now_stamp, Ctx, Runner};
use crate::manifest::{json_scalar, Build, Project};
use crate::style;
use crate::{daemon, project};

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
    v
}

fn git_dir_ok(root: &Path) -> bool {
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

#[derive(serde::Deserialize)]
struct BuilderResources {
    cpus: Option<u32>,
    #[serde(rename = "memoryInBytes")]
    memory_in_bytes: Option<u64>,
}

#[derive(serde::Deserialize)]
struct BuilderConfiguration {
    resources: Option<BuilderResources>,
}

#[derive(serde::Deserialize)]
struct BuilderEntry {
    configuration: Option<BuilderConfiguration>,
}

pub fn memory_to_mb(s: &str) -> Option<u64> {
    let t = s.trim().to_ascii_lowercase();
    let (num, mult) = if let Some(n) = t.strip_suffix("gb") {
        (n, 1024)
    } else if let Some(n) = t.strip_suffix('g') {
        (n, 1024)
    } else if let Some(n) = t.strip_suffix("mb") {
        (n, 1)
    } else if let Some(n) = t.strip_suffix('m') {
        (n, 1)
    } else {
        (t.as_str(), 1)
    };
    num.trim().parse::<u64>().ok().map(|n| n * mult)
}

pub fn ensure_builder(ctx: &Ctx, want_cpus: Option<u32>, want_mem: Option<&str>) {
    if want_cpus.is_none() && want_mem.is_none() {
        return;
    }

    let Ok(text) = ctx
        .container(["builder", "status", "--format", "json"])
        .silent()
        .stdout()
    else {
        return;
    };
    let Ok(entries) = serde_json::from_str::<Vec<BuilderEntry>>(&text) else {
        return;
    };
    let Some(res) = entries
        .first()
        .and_then(|e| e.configuration.as_ref())
        .and_then(|c| c.resources.as_ref())
    else {
        return;
    };

    let cur_cpus = res.cpus;
    let cur_mb = res.memory_in_bytes.map(|b| b / (1024 * 1024));
    let want_mb = want_mem.and_then(memory_to_mb);

    let cpus_ok = want_cpus.is_none() || want_cpus == cur_cpus;
    let mem_ok = want_mb.is_none() || want_mb == cur_mb;
    if cpus_ok && mem_ok {
        return;
    }

    let show = |v: Option<u64>| v.map(|x| x.to_string()).unwrap_or_else(|| "?".into());
    ctx.warn(&format!(
        "resizing buildkit builder from {} cpus / {} MB to {} cpus / {} MB. \
The builder only reads these values when it is created, so it is being stopped \
first and its layer cache is discarded.",
        show(cur_cpus.map(u64::from)),
        show(cur_mb),
        want_cpus
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unchanged".into()),
        want_mb
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unchanged".into()),
    ));
    ctx.container(["builder", "stop"]).quiet_ok();
    thread::sleep(Duration::from_secs(2));
}

struct Reporter<'a> {
    ctx: &'a Ctx,
    name: String,
    multi: Option<&'a MultiProgress>,
    bar: Option<ProgressBar>,
}

impl<'a> Reporter<'a> {
    fn plain(ctx: &'a Ctx, name: &str) -> Self {
        Reporter {
            ctx,
            name: name.to_string(),
            multi: None,
            bar: None,
        }
    }

    fn emit(&self, line: String) {
        match self.multi {
            Some(multi) => {
                multi.println(line).ok();
            }
            None => self.ctx.log(&line),
        }
    }

    fn info(&self, msg: &str) {
        self.emit(format!("{} [{}] {msg}", style::blue("==>"), self.name));
    }
    fn ok(&self, msg: &str) {
        self.emit(format!("{} [{}] {msg}", style::green("  ok"), self.name));
    }
    fn dim(&self, msg: &str) {
        self.emit(style::dim(&format!("  [{}] {msg}", self.name)));
    }
    fn tick(&self, msg: &str) {
        if let Some(bar) = &self.bar {
            bar.set_message(msg.to_string());
        }
    }

    fn run(&self, runner: Runner<'_>) -> Result<bool> {
        let Some(multi) = self.multi else {
            return Ok(runner.status()?.success());
        };

        let mut child = runner.spawn_piped()?;
        let (tx, rx) = mpsc::channel::<String>();
        let mut readers = Vec::new();
        if let Some(o) = child.stdout.take() {
            let tx = tx.clone();
            readers.push(thread::spawn(move || {
                for l in BufReader::new(o).lines().map_while(Result::ok) {
                    tx.send(l).ok();
                }
            }));
        }
        if let Some(e) = child.stderr.take() {
            let tx = tx.clone();
            readers.push(thread::spawn(move || {
                for l in BufReader::new(e).lines().map_while(Result::ok) {
                    tx.send(l).ok();
                }
            }));
        }
        drop(tx);

        for line in rx {
            self.tick(&line);
            multi
                .println(format!(
                    "{} {line}",
                    style::dim(&format!("{:>12} |", self.name))
                ))
                .ok();
        }
        for r in readers {
            r.join().ok();
        }
        Ok(child.wait()?.success())
    }
}

fn run_hooks(
    rep: &Reporter,
    root: &Path,
    key: &str,
    hooks: &[Vec<String>],
    v: &Vars,
) -> Result<()> {
    for hook in hooks {
        if hook.is_empty() {
            continue;
        }
        let argv: Vec<String> = hook.iter().map(|a| interpolate(a, v)).collect();
        rep.dim(&format!("{key}: {}", argv.join(" ")));
        rep.tick(&format!("{key}: {}", argv[0]));
        let runner = rep.ctx.exec(&argv[0], &argv[1..]).cwd(root);
        let ok = rep
            .run(runner)
            .map_err(|e| anyhow!("[{}] {key} could not run: {e}", rep.name))?;
        if !ok {
            return Err(anyhow!("[{}] {key} failed: {}", rep.name, argv.join(" ")));
        }
    }
    Ok(())
}

struct Plan {
    args: Vec<String>,
    tags: Vec<String>,
    platform: String,
    push: bool,
}

fn plan_build(proj: &Project, b: &Build, ov: &BuildOverrides, v: &Vars) -> Result<Plan> {
    let platform = ov
        .platform
        .clone()
        .or_else(|| {
            proj.manifest
                .profiles
                .get(&v.profile)
                .and_then(|p| p.platform.clone())
        })
        .or_else(|| b.platform.clone())
        .unwrap_or_else(|| "linux/arm64".to_string());

    let push = ov.push.unwrap_or_else(|| profile_push(proj, &v.profile));
    let target = ov.target.clone().or_else(|| b.target.clone());

    let image = interpolate(&b.image, v);
    let tags: Vec<String> = b
        .tags
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| format!("{image}:{}", interpolate(t, v)))
        .collect();
    if tags.is_empty() {
        return Err(anyhow!("build '{}' declares no tags", b.name));
    }

    let mut args: Vec<String> = vec![
        "build".into(),
        "--platform".into(),
        platform.clone(),
        "-f".into(),
        b.dockerfile.clone(),
    ];
    if let Some(p) = &ov.progress {
        args.push("--progress".into());
        args.push(p.clone());
    }
    if let Some(t) = &target {
        args.push("--target".into());
        args.push(t.clone());
    }
    if ov.no_cache || std::env::var_os("NO_CACHE").is_some() {
        args.push("--no-cache".into());
    }

    if let Some(c) = builder_cpus(proj, ov) {
        args.push("--cpus".into());
        args.push(c.to_string());
    }
    if let Some(m) = builder_memory(proj, ov) {
        args.push("--memory".into());
        args.push(m);
    }

    for (k, val) in &b.build_args {
        args.push("--build-arg".into());
        args.push(interpolate(&format!("{k}={}", json_scalar(val)), v));
    }
    for s in &b.secrets {
        let mut spec = format!("id={}", s.id);
        if let Some(e) = &s.env {
            spec.push_str(&format!(",env={e}"));
        }
        if let Some(src) = &s.src {
            spec.push_str(&format!(",src={src}"));
        }
        args.push("--secret".into());
        args.push(spec);
    }
    for (k, val) in &b.labels {
        args.push("--label".into());
        args.push(interpolate(&format!("{k}={}", json_scalar(val)), v));
    }
    for t in &tags {
        args.push("-t".into());
        args.push(t.clone());
    }
    args.push(b.context.clone());

    Ok(Plan {
        args,
        tags,
        platform,
        push,
    })
}

fn profile_push(proj: &Project, profile: &str) -> bool {
    proj.manifest
        .profiles
        .get(profile)
        .and_then(|p| p.push)
        .unwrap_or(false)
}

fn builder_cpus(proj: &Project, ov: &BuildOverrides) -> Option<u32> {
    ov.builder_cpus
        .or_else(|| proj.manifest.builder.as_ref().and_then(|x| x.cpus))
}

fn builder_memory(proj: &Project, ov: &BuildOverrides) -> Option<String> {
    ov.builder_memory.clone().or_else(|| {
        proj.manifest
            .builder
            .as_ref()
            .and_then(|x| x.memory.clone())
    })
}

fn build_one(
    rep: &Reporter,
    proj: &Project,
    root: &Path,
    b: &Build,
    ov: &BuildOverrides,
    v: &Vars,
) -> Result<()> {
    let plan = plan_build(proj, b, ov, v)?;

    rep.info("preflight");
    rep.tick("preflight");
    run_hooks(rep, root, "preflight", &b.preflight, v)?;

    rep.dim(&format!("root: {}", root.display()));
    rep.info(&format!("building {} -> {}", plan.platform, plan.tags[0]));
    rep.tick("building");
    let runner = rep.ctx.container(&plan.args).cwd(root);
    if !rep.run(runner)? {
        return Err(anyhow!("[{}] build failed", rep.name));
    }
    rep.ok("built");

    if plan.push {
        for t in &plan.tags {
            rep.info(&format!("pushing {t}"));
            rep.tick(&format!("pushing {t}"));
            let runner = rep.ctx.container(["image", "push", t.as_str()]);
            if !rep.run(runner)? {
                return Err(anyhow!("[{}] push failed: {t}", rep.name));
            }
        }
        rep.ok("pushed");
        rep.tick("postPush");
        run_hooks(rep, root, "postPush", &b.post_push, v)?;
    } else {
        rep.dim(&format!("push disabled for profile '{}'", v.profile));
    }
    Ok(())
}

pub fn project_build(
    ctx: &Ctx,
    proj: &Project,
    names: &[String],
    ov: &BuildOverrides,
) -> Result<()> {
    let profile = ov.profile_name();
    if proj.manifest.profiles.get(&profile).is_none() {
        return Err(anyhow!(
            "unknown profile '{profile}' (have: {})",
            proj.manifest.profiles.keys().collect::<Vec<_>>().join(", ")
        ));
    }

    let all = proj.manifest.build_names();
    let targets: Vec<String> = if names.is_empty() {
        all.clone()
    } else {
        for n in names {
            if !all.contains(n) {
                return Err(anyhow!("no such build '{n}' (have: {})", all.join(" ")));
            }
        }
        names.to_vec()
    };
    if targets.is_empty() {
        return Err(anyhow!("project '{}' declares no builds", proj.name));
    }

    let root = resolve_root(ctx, proj, ov)?;
    let vars_preview = vars_for(proj, &profile, &root);

    if ov.dry_run {
        let plans: Vec<serde_json::Value> = targets
            .iter()
            .filter_map(|t| proj.manifest.build(t))
            .filter_map(|b| {
                plan_build(proj, b, ov, &vars_preview)
                    .ok()
                    .map(|plan| (b, plan))
            })
            .map(|(b, plan)| {
                serde_json::json!({
                    "build": b.name,
                    "profile": profile,
                    "root": root.display().to_string(),
                    "dockerfile": b.dockerfile,
                    "platform": plan.platform,
                    "tags": plan.tags,
                    "push": plan.push,
                    "command": plan.args,
                })
            })
            .collect();

        if ctx.json {
            return ctx.emit_json(&serde_json::Value::Array(plans));
        }
        for p in &plans {
            println!("{}", style::bold(p["build"].as_str().unwrap_or("")));
            println!("  profile     {}", p["profile"].as_str().unwrap_or(""));
            println!("  root        {}", p["root"].as_str().unwrap_or(""));
            println!("  dockerfile  {}", p["dockerfile"].as_str().unwrap_or(""));
            println!("  platform    {}", p["platform"].as_str().unwrap_or(""));
            for t in p["tags"].as_array().into_iter().flatten() {
                println!("  tag         {}", t.as_str().unwrap_or(""));
            }
            println!("  push        {}", p["push"]);
            if let Some(a) = p["command"].as_array() {
                let joined: Vec<String> = a
                    .iter()
                    .map(|x| x.as_str().unwrap_or("").to_string())
                    .collect();
                println!("  {}", style::dim(&format!("$ container {}", joined.join(" "))));
            }
            println!();
        }
        ctx.dim("dry run, nothing was built or pushed");
        return Ok(());
    }

    ctx.info(&format!("build root: {}", root.display()));

    daemon::ensure(ctx)?;
    ensure_builder(
        ctx,
        builder_cpus(proj, ov),
        builder_memory(proj, ov).as_deref(),
    );

    let vars = vars_for(proj, &profile, &root);

    let entries: Vec<Build> = targets
        .iter()
        .filter_map(|t| proj.manifest.build(t).cloned())
        .collect();

    if ov.push.unwrap_or_else(|| profile_push(proj, &profile)) {
        let images: Vec<String> = entries
            .iter()
            .map(|b| interpolate(&b.image, &vars))
            .collect();
        project::login(ctx, proj, &vars, &images).ok();
    }

    if entries.len() > 1 && !ov.sequential {
        ctx.info(&format!(
            "building {} images in parallel (--sequential to disable)",
            entries.len()
        ));
        run_parallel(ctx, proj, &root, &entries, ov, &vars)
    } else {
        let mut failures = Vec::new();
        for b in &entries {
            let rep = Reporter::plain(ctx, &b.name);
            if let Err(e) = build_one(&rep, proj, &root, b, ov, &vars) {
                ctx.err(&format!("{e}"));
                failures.push(b.name.clone());
            }
        }
        finish(ctx, failures)
    }
}

fn run_parallel(
    ctx: &Ctx,
    proj: &Project,
    root: &Path,
    entries: &[Build],
    ov: &BuildOverrides,
    vars: &Vars,
) -> Result<()> {
    let multi = if ctx.color {
        MultiProgress::new()
    } else {
        MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden())
    };
    let style = ProgressStyle::with_template("{spinner:.cyan} {prefix:12} {wide_msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner());
    let multi_ref = &multi;

    let failures: Vec<String> = thread::scope(|scope| {
        let handles: Vec<_> = entries
            .iter()
            .map(|b| {
                let bar = if ctx.color {
                    let bar = multi_ref.add(ProgressBar::new_spinner());
                    bar.set_style(style.clone());
                    bar.set_prefix(b.name.clone());
                    bar.set_message("starting");
                    bar.enable_steady_tick(Duration::from_millis(120));
                    Some(bar)
                } else {
                    None
                };
                scope.spawn(move || {
                    let rep = Reporter {
                        ctx,
                        name: b.name.clone(),
                        multi: Some(multi_ref),
                        bar: bar.clone(),
                    };
                    let res = build_one(&rep, proj, root, b, ov, vars);
                    if let Some(bar) = &bar {
                        match &res {
                            Ok(()) => bar.finish_with_message("done"),
                            Err(_) => bar.finish_with_message("failed"),
                        }
                    }
                    if let Err(e) = &res {
                        ctx.err(&format!("{e}"));
                    }
                    (b.name.clone(), res.is_err())
                })
            })
            .collect();

        handles
            .into_iter()
            .filter_map(|h| match h.join() {
                Ok((name, true)) => Some(name),
                Ok((_, false)) => None,
                Err(_) => Some("<panicked>".to_string()),
            })
            .collect()
    });

    finish(ctx, failures)
}

fn finish(ctx: &Ctx, failures: Vec<String>) -> Result<()> {
    if !failures.is_empty() {
        return Err(anyhow!(
            "one or more builds failed: {}",
            failures.join(", ")
        ));
    }
    ctx.ok("all builds finished");
    Ok(())
}
