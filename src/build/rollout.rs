use std::path::Path;

use anyhow::{anyhow, Result};

use crate::build::plan::run_hooks;
use crate::build::reporter::{Mode, Reporter};
use crate::build::vars::{
    absolute_prog, hook_env, interpolate, resolve_root, vars_for, BuildOverrides, Vars,
};
use crate::core::ctx::Ctx;
use crate::core::style;
use crate::manifest::Project;

pub(crate) fn profile_rollout<'a>(
    proj: &'a Project,
    profile: &str,
) -> Option<&'a crate::manifest::Rollout> {
    proj.manifest
        .profiles
        .get(profile)
        .and_then(|p| p.rollout.as_ref())
}

fn rollout_profiles(proj: &Project) -> Vec<&str> {
    proj.manifest
        .profiles
        .0
        .iter()
        .filter(|(_, p)| p.rollout.is_some())
        .map(|(n, _)| n.as_str())
        .collect()
}

pub(crate) fn require_rollout<'a>(
    proj: &'a Project,
    profile: &str,
) -> Result<&'a crate::manifest::Rollout> {
    profile_rollout(proj, profile).ok_or_else(|| {
        let have = rollout_profiles(proj);
        if have.is_empty() {
            anyhow!(
                "profile '{profile}' declares no rollout, and neither does any other \
profile in project '{}'. Add \"rollout\": {{ \"run\": [[...]] }} to a profile.",
                proj.name
            )
        } else {
            anyhow!(
                "profile '{profile}' declares no rollout (profiles that do: {})",
                have.join(", ")
            )
        }
    })
}

pub(crate) fn wants_rollout(proj: &Project, profile: &str, ov: &BuildOverrides) -> Result<bool> {
    match ov.rollout {
        Some(false) => Ok(false),
        Some(true) => {
            require_rollout(proj, profile)?;
            Ok(true)
        }
        None => Ok(profile_rollout(proj, profile).is_some_and(|r| r.auto)),
    }
}

pub(crate) fn run_rollout_hooks(
    ctx: &Ctx,
    proj: &Project,
    root: &Path,
    key: &str,
    hooks: &[Vec<String>],
    v: &Vars,
    builds: &[String],
) -> Result<()> {
    if hooks.is_empty() {
        return Ok(());
    }
    let rep = Reporter::new(ctx, "", Mode::Inherit, None, None);
    let env = hook_env(proj, v, root, builds);
    run_hooks(&rep, root, key, hooks, v, &env)
}

pub fn project_rollout(
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
    let rollout = require_rollout(proj, &profile)?.clone();

    let all = proj.manifest.build_names();
    let builds: Vec<String> = if names.is_empty() {
        all.clone()
    } else {
        for n in names {
            if !all.contains(n) {
                return Err(anyhow!("no such build '{n}' (have: {})", all.join(" ")));
            }
        }
        names.to_vec()
    };

    let root = resolve_root(ctx, proj, ov)?;
    let vars = vars_for(proj, &profile, &root);

    if ov.dry_run {
        return emit_rollout_plan(ctx, &profile, &rollout, &vars, &root, proj, &builds);
    }

    ctx.info(&format!("rollout profile: {profile}"));
    ctx.info(&format!("build root: {}", root.display()));
    run_rollout_hooks(
        ctx,
        proj,
        &root,
        "rollout.preflight",
        &rollout.preflight,
        &vars,
        &builds,
    )?;
    run_rollout_hooks(ctx, proj, &root, "rollout", &rollout.run, &vars, &builds)?;
    ctx.ok("rollout finished");
    Ok(())
}

pub(crate) fn emit_rollout_plan(
    ctx: &Ctx,
    profile: &str,
    rollout: &crate::manifest::Rollout,
    vars: &Vars,
    root: &Path,
    proj: &Project,
    builds: &[String],
) -> Result<()> {
    let render = |hooks: &[Vec<String>]| -> Vec<Vec<String>> {
        hooks
            .iter()
            .filter(|h| !h.is_empty())
            .map(|h| {
                let mut argv: Vec<String> = h.iter().map(|a| interpolate(a, vars)).collect();
                argv[0] = absolute_prog(&argv[0], root);
                argv
            })
            .collect()
    };
    let pre = render(&rollout.preflight);
    let run = render(&rollout.run);
    let env = hook_env(proj, vars, root, builds);

    if ctx.json {
        let env_map: serde_json::Map<String, serde_json::Value> = env
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
        return ctx.emit_json(&serde_json::json!({
            "profile": profile,
            "root": root.display().to_string(),
            "builds": builds,
            "preflight": pre,
            "run": run,
            "env": env_map,
        }));
    }

    println!("{}", style::bold(profile));
    if let Some(d) = &rollout.description {
        println!("  {d}");
    }
    println!("  root        {}", root.display());
    println!("  builds      {}", builds.join(" "));
    for (label, hooks) in [("preflight", &pre), ("rollout", &run)] {
        for h in hooks {
            println!(
                "  {}",
                style::dim(&format!(
                    "$ {label}: {}",
                    h.iter()
                        .map(|a| crate::core::ctx::shell_quote(a))
                        .collect::<Vec<_>>()
                        .join(" ")
                ))
            );
        }
    }
    ctx.dim("dry run, nothing was rolled out");
    Ok(())
}
