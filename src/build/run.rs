use std::path::Path;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::build::builder::ensure_builder;
use crate::build::plan::{
    build_one, builder_cpus, builder_memory, plan_build, profile_push, Outcome,
};
use crate::build::reporter::{name_width, output_mode, spawn_ticker, Mode, Reporter};
use crate::build::rollout::{
    emit_rollout_plan, profile_rollout, require_rollout, run_rollout_hooks, wants_rollout,
};
use crate::build::vars::{interpolate, resolve_root, vars_for, BuildOverrides, Vars};
use crate::commands::project;
use crate::core::ctx::Ctx;
use crate::core::style;
use crate::core::util::Table;
use crate::daemon;
use crate::manifest::{Build, Project};
use crate::progress::fmt_secs;

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
    let do_rollout = wants_rollout(proj, &profile, ov)?;
    let pushes = ov.push.unwrap_or_else(|| profile_push(proj, &profile));
    if do_rollout && !pushes {
        return Err(anyhow!(
            "--rollout needs a profile that pushes, but '{profile}' resolves to push=false, \
so nothing would reach the registry for the rollout to pick up"
        ));
    }

    if ov.dry_run {
        let plans: Vec<serde_json::Value> = targets
            .iter()
            .filter_map(|t| proj.manifest.build(t))
            .filter_map(|b| {
                plan_build(proj, b, ov, &vars_preview, ov.progress.as_deref(), &root)
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
                    .map(|x| crate::core::ctx::shell_quote(x.as_str().unwrap_or("")))
                    .collect();
                println!(
                    "  {}",
                    style::dim(&format!("$ container {}", joined.join(" ")))
                );
            }
            println!();
        }
        if do_rollout {
            if let Some(r) = profile_rollout(proj, &profile) {
                emit_rollout_plan(ctx, &profile, r, &vars_preview, &root, proj, &targets)?;
            }
        }
        ctx.dim("dry run, nothing was built or pushed");
        return Ok(());
    }

    ctx.info(&format!("build root: {}", root.display()));

    if do_rollout {
        let r = require_rollout(proj, &profile)?.clone();
        run_rollout_hooks(
            ctx,
            proj,
            &root,
            "rollout.preflight",
            &r.preflight,
            &vars_preview,
            &targets,
        )?;
    }

    daemon::ensure(ctx)?;
    ensure_builder(
        ctx,
        builder_cpus(proj, ov),
        builder_memory(proj, ov).as_deref(),
    );

    let vars = vars_preview;

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

    let mode = output_mode(ctx, ov, entries.len());
    let progress = match mode {
        Mode::Fancy | Mode::Stream => Some("plain"),
        Mode::Inherit => ov.progress.as_deref(),
    };

    let parallel = entries.len() > 1 && !ov.sequential && mode != Mode::Inherit;
    if parallel {
        ctx.info(&format!(
            "building {} images in parallel (--sequential to disable)",
            entries.len()
        ));
    }

    let outcomes = if mode == Mode::Fancy {
        run_fancy(ctx, proj, &root, &entries, ov, &vars, progress, parallel)
    } else {
        run_basic(ctx, proj, &root, &entries, ov, &vars, progress, mode)
    };

    report(ctx, &outcomes)?;

    if do_rollout {
        let r = require_rollout(proj, &profile)?.clone();
        ctx.info(&format!("rolling out profile '{profile}'"));
        run_rollout_hooks(ctx, proj, &root, "rollout", &r.run, &vars, &targets)?;
        ctx.ok("rollout finished");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_fancy(
    ctx: &Ctx,
    proj: &Project,
    root: &Path,
    entries: &[Build],
    ov: &BuildOverrides,
    vars: &Vars,
    progress: Option<&str>,
    parallel: bool,
) -> Vec<Outcome> {
    let multi = MultiProgress::new();
    let width = name_width(entries);
    let bar_style = ProgressStyle::with_template(&format!(
        "{{spinner:.cyan}} {{prefix:<{width}}} {{wide_msg}}"
    ))
    .unwrap_or_else(|_| ProgressStyle::default_spinner());

    let reporters: Vec<Reporter<'_>> = entries
        .iter()
        .map(|b| {
            let bar = multi.add(ProgressBar::new_spinner());
            bar.set_style(bar_style.clone());
            bar.set_prefix(b.name.clone());
            bar.set_message("starting");
            bar.enable_steady_tick(Duration::from_millis(120));
            Reporter::new(ctx, &b.name, Mode::Fancy, Some(&multi), Some(bar)).padded_to(width)
        })
        .collect();

    let refs: Vec<&Reporter<'_>> = reporters.iter().collect();
    let (stop, ticker) = spawn_ticker(&refs);

    let run_one = |rep: &Reporter<'_>, b: &Build| -> Outcome {
        let res = build_one(rep, proj, root, b, ov, vars, progress);
        let (steps_done, steps_cached, secs) = rep
            .tracker
            .lock()
            .map(|t| (t.steps_done, t.steps_cached, t.total_elapsed()))
            .unwrap_or((0, 0, 0.0));
        let outcome = match res {
            Ok((tags, pushed)) => Outcome {
                name: b.name.clone(),
                ok: true,
                secs,
                steps_done,
                steps_cached,
                tags,
                pushed,
                error: None,
            },
            Err(e) => Outcome {
                name: b.name.clone(),
                ok: false,
                secs,
                steps_done,
                steps_cached,
                tags: Vec::new(),
                pushed: false,
                error: Some(e.to_string()),
            },
        };
        if let Some(bar) = &rep.bar {
            if outcome.ok {
                bar.finish_with_message(format!(
                    "done in {}  ({} steps, {} cached)",
                    fmt_secs(outcome.secs),
                    outcome.steps_done,
                    outcome.steps_cached
                ));
            } else {
                bar.finish_with_message(format!("failed after {}", fmt_secs(outcome.secs)));
            }
        }
        outcome
    };

    let outcomes: Vec<Outcome> = if parallel {
        thread::scope(|scope| {
            let handles: Vec<_> = entries
                .iter()
                .zip(reporters.iter())
                .map(|(b, rep)| scope.spawn(move || run_one(rep, b)))
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| Outcome {
                        name: "<panicked>".into(),
                        ok: false,
                        secs: 0.0,
                        steps_done: 0,
                        steps_cached: 0,
                        tags: Vec::new(),
                        pushed: false,
                        error: Some("build thread panicked".into()),
                    })
                })
                .collect()
        })
    } else {
        entries
            .iter()
            .zip(reporters.iter())
            .map(|(b, rep)| run_one(rep, b))
            .collect()
    };

    stop.store(true, Ordering::Relaxed);
    ticker.join().ok();
    outcomes
}

#[allow(clippy::too_many_arguments)]
fn run_basic(
    ctx: &Ctx,
    proj: &Project,
    root: &Path,
    entries: &[Build],
    ov: &BuildOverrides,
    vars: &Vars,
    progress: Option<&str>,
    mode: Mode,
) -> Vec<Outcome> {
    let width = name_width(entries);
    let run_one = |rep: &Reporter<'_>, b: &Build| -> Outcome {
        let res = build_one(rep, proj, root, b, ov, vars, progress);
        let (steps_done, steps_cached, secs) = rep
            .tracker
            .lock()
            .map(|t| (t.steps_done, t.steps_cached, t.total_elapsed()))
            .unwrap_or((0, 0, 0.0));
        match res {
            Ok((tags, pushed)) => Outcome {
                name: b.name.clone(),
                ok: true,
                secs,
                steps_done,
                steps_cached,
                tags,
                pushed,
                error: None,
            },
            Err(e) => {
                ctx.err(&format!("{e}"));
                Outcome {
                    name: b.name.clone(),
                    ok: false,
                    secs,
                    steps_done,
                    steps_cached,
                    tags: Vec::new(),
                    pushed: false,
                    error: Some(e.to_string()),
                }
            }
        }
    };

    let parallel = entries.len() > 1 && !ov.sequential && mode == Mode::Stream;
    if parallel {
        thread::scope(|scope| {
            let handles: Vec<_> = entries
                .iter()
                .map(|b| {
                    scope.spawn(move || {
                        let rep = Reporter::new(ctx, &b.name, mode, None, None).padded_to(width);
                        run_one(&rep, b)
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        })
    } else {
        entries
            .iter()
            .map(|b| {
                let rep = Reporter::new(ctx, &b.name, mode, None, None).padded_to(width);
                run_one(&rep, b)
            })
            .collect()
    }
}

pub fn project_push(
    ctx: &Ctx,
    proj: &Project,
    names: &[String],
    profile_arg: Option<&str>,
) -> Result<()> {
    let ov = BuildOverrides {
        profile: profile_arg.map(String::from),
        ..Default::default()
    };
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

    let root = resolve_root(ctx, proj, &ov)?;
    let vars = vars_for(proj, &profile, &root);

    let entries: Vec<Build> = targets
        .iter()
        .filter_map(|t| proj.manifest.build(t).cloned())
        .collect();
    let images: Vec<String> = entries
        .iter()
        .map(|b| interpolate(&b.image, &vars))
        .collect();

    daemon::ensure(ctx)?;
    project::login(ctx, proj, &vars, &images).ok();

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for b in &entries {
        let image = interpolate(&b.image, &vars);
        let tags: Vec<String> = b
            .tags
            .iter()
            .filter(|t| !t.is_empty())
            .map(|t| format!("{image}:{}", interpolate(t, &vars)))
            .collect();
        let mut pushed: Vec<String> = Vec::new();
        for t in &tags {
            ctx.info(&format!("pushing {t}"));
            if ctx
                .container(["image", "push", t.as_str()])
                .status()?
                .success()
            {
                ctx.ok(t);
                pushed.push(t.clone());
            } else {
                ctx.err(&format!("push failed: {t}"));
                failures.push(b.name.clone());
            }
        }
        results.push(serde_json::json!({
            "build": b.name,
            "profile": profile,
            "tags": tags,
            "pushed": pushed,
        }));
    }

    if ctx.json {
        ctx.emit_json(&serde_json::Value::Array(results))?;
    }
    if failures.is_empty() {
        Ok(())
    } else {
        failures.dedup();
        Err(anyhow!("push failed for: {}", failures.join(", ")))
    }
}

fn report(ctx: &Ctx, outcomes: &[Outcome]) -> Result<()> {
    if ctx.json {
        let items: Vec<serde_json::Value> = outcomes.iter().map(|o| o.to_json()).collect();
        ctx.emit_json(&serde_json::Value::Array(items))?;
    } else {
        let mut table = Table::new(&["BUILD", "STATUS", "TIME", "STEPS", "TAGS"]).right(&[2, 3]);
        for o in outcomes {
            let status = if o.ok { "ok" } else { "failed" };
            let steps = if o.steps_done > 0 {
                format!("{} ({}c)", o.steps_done, o.steps_cached)
            } else {
                "-".to_string()
            };
            table.row([
                o.name.clone(),
                status.to_string(),
                fmt_secs(o.secs),
                steps,
                o.tags.join(", "),
            ]);
        }
        table.print(ctx);
    }

    let failures: Vec<&str> = outcomes
        .iter()
        .filter(|o| !o.ok)
        .map(|o| o.name.as_str())
        .collect();
    if !failures.is_empty() {
        return Err(anyhow!(
            "one or more builds failed: {}",
            failures.join(", ")
        ));
    }
    ctx.ok("all builds finished");
    Ok(())
}
