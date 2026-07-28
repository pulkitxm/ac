use std::path::Path;

use anyhow::{anyhow, Result};

use crate::build::reporter::{Mode, Reporter};
use crate::build::vars::{
    absolute_against, absolute_prog, hook_env, interpolate, BuildOverrides, Vars,
};
use crate::manifest::{json_scalar, Build, Project};

pub(crate) fn run_hooks(
    rep: &Reporter,
    root: &Path,
    key: &str,
    hooks: &[Vec<String>],
    v: &Vars,
    env: &[(String, String)],
) -> Result<()> {
    for hook in hooks {
        if hook.is_empty() {
            continue;
        }
        let mut argv: Vec<String> = hook.iter().map(|a| interpolate(a, v)).collect();
        argv[0] = absolute_prog(&argv[0], root);
        rep.info(key);
        rep.phase(&format!("{key}: {}", argv[0]));
        let runner = rep
            .ctx
            .exec(&argv[0], &argv[1..])
            .cwd(root)
            .envs(env.to_vec());
        let ok = rep
            .run(runner)
            .map_err(|e| anyhow!("{}{key} could not run: {e}", rep.label()))?;
        if !ok {
            return Err(anyhow!("{}{key} failed: {}", rep.label(), argv.join(" ")));
        }
    }
    Ok(())
}

pub(crate) struct Plan {
    pub(crate) args: Vec<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) platform: String,
    pub(crate) push: bool,
}

pub(crate) fn plan_build(
    proj: &Project,
    b: &Build,
    ov: &BuildOverrides,
    v: &Vars,
    progress: Option<&str>,
    root: &Path,
) -> Result<Plan> {
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
    for (raw, rendered) in b.tags.iter().filter(|t| !t.is_empty()).zip(tags.iter()) {
        let suffix = rendered.rsplit(':').next().unwrap_or("");
        if suffix.is_empty() || suffix.starts_with('-') {
            return Err(anyhow!(
                "build '{}': tag template '{raw}' rendered as '{rendered}'; \
git placeholders are empty because the build root is not a git repository",
                b.name
            ));
        }
    }

    let mut args: Vec<String> = vec![
        "build".into(),
        "--platform".into(),
        platform.clone(),
        "-f".into(),
        absolute_against(&b.dockerfile, root),
    ];
    if let Some(p) = progress {
        args.push("--progress".into());
        args.push(p.to_string());
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
    args.push(absolute_against(&b.context, root));

    Ok(Plan {
        args,
        tags,
        platform,
        push,
    })
}

pub(crate) fn profile_push(proj: &Project, profile: &str) -> bool {
    proj.manifest
        .profiles
        .get(profile)
        .and_then(|p| p.push)
        .unwrap_or(false)
}

pub(crate) fn builder_cpus(proj: &Project, ov: &BuildOverrides) -> Option<u32> {
    ov.builder_cpus
        .or_else(|| proj.manifest.builder.as_ref().and_then(|x| x.cpus))
}

pub(crate) fn builder_memory(proj: &Project, ov: &BuildOverrides) -> Option<String> {
    ov.builder_memory.clone().or_else(|| {
        proj.manifest
            .builder
            .as_ref()
            .and_then(|x| x.memory.clone())
    })
}

pub struct Outcome {
    pub name: String,
    pub ok: bool,
    pub secs: f32,
    pub steps_done: u32,
    pub steps_cached: u32,
    pub tags: Vec<String>,
    pub pushed: bool,
    pub error: Option<String>,
}

impl Outcome {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "build": self.name,
            "ok": self.ok,
            "seconds": (f64::from(self.secs) * 10.0).round() / 10.0,
            "steps": { "done": self.steps_done, "cached": self.steps_cached },
            "tags": self.tags,
            "pushed": self.pushed,
            "error": self.error,
        })
    }
}

pub(crate) fn build_one(
    rep: &Reporter,
    proj: &Project,
    root: &Path,
    b: &Build,
    ov: &BuildOverrides,
    v: &Vars,
    progress: Option<&str>,
) -> Result<(Vec<String>, bool)> {
    let plan = plan_build(proj, b, ov, v, progress, root)?;

    let env = hook_env(proj, v, root, std::slice::from_ref(&b.name));
    rep.phase("preflight");
    run_hooks(rep, root, "preflight", &b.preflight, v, &env)?;

    rep.info(&format!("building {} -> {}", plan.platform, plan.tags[0]));
    rep.phase("resolving");
    let runner = rep.ctx.container(&plan.args).cwd(root);
    if !rep.run(runner)? {
        if rep.mode == Mode::Fancy {
            rep.dump_tail(40);
        }
        return Err(anyhow!("[{}] build failed", rep.name));
    }
    rep.ok("built");

    if plan.push {
        for t in &plan.tags {
            rep.info(&format!("pushing {t}"));
            rep.phase(&format!("pushing {t}"));
            let runner = rep.ctx.container(["image", "push", t.as_str()]);
            if !rep.run(runner)? {
                return Err(anyhow!("[{}] push failed: {t}", rep.name));
            }
        }
        rep.ok("pushed");
        rep.phase("postPush");
        run_hooks(rep, root, "postPush", &b.post_push, v, &env)?;
    } else {
        rep.dim(&format!("push disabled for profile '{}'", v.profile));
    }
    Ok((plan.tags, plan.push))
}
