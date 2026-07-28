use std::path::Path;
use std::process::ExitStatus;

use anyhow::{anyhow, Result};

use crate::cli::RunOpts;
use crate::commands::docker::opts::{report_urls, run_opts_argv};
use crate::commands::docker::target::{auto_tty, require_targets, resolve, resolve_all, wants_tty};
use crate::commands::project;
use crate::core::ctx::Ctx;
use crate::core::state::Snapshot;
use crate::core::util::exit_ok;
use crate::daemon::{self, supervisor};

pub fn run(
    ctx: &Ctx,
    opts: &RunOpts,
    rm: bool,
    image: &str,
    command: &[String],
) -> Result<ExitStatus> {
    daemon::ensure(ctx)?;
    supervisor::ensure(ctx)?;

    let mut args: Vec<String> = vec!["run".into()];
    if opts.detach {
        args.push("-d".into());
    }
    if rm {
        args.push("--rm".into());
    }
    if opts.interactive || (!opts.detach && auto_tty()) {
        args.push("-i".into());
    }
    if !opts.detach && wants_tty(opts.tty || auto_tty()) {
        args.push("-t".into());
    } else if opts.tty && !auto_tty() {
        ctx.warn("not allocating a TTY: stdin and stdout are not both terminals");
    }
    args.extend(run_opts_argv(opts, true));
    args.push(image.to_string());
    args.extend(command.iter().cloned());

    let status = ctx.container(&args).status()?;

    if opts.detach && status.success() {
        if let Some(name) = &opts.name {
            report_urls(ctx, name);
        }
    }
    supervisor::settle(ctx)?;
    Ok(status)
}

pub fn create(
    ctx: &Ctx,
    opts: &RunOpts,
    rm: bool,
    image: &str,
    command: &[String],
) -> Result<ExitStatus> {
    daemon::ensure(ctx)?;
    supervisor::ensure(ctx)?;

    if opts.progress.is_some() {
        ctx.warn("container create has no --progress; ignoring it");
    }
    let mut args: Vec<String> = vec!["create".into()];
    if rm {
        args.push("--rm".into());
    }
    if opts.interactive {
        args.push("-i".into());
    }
    args.extend(run_opts_argv(opts, false));
    args.push(image.to_string());
    args.extend(command.iter().cloned());

    let status = ctx.container(&args).status()?;
    supervisor::settle(ctx)?;
    Ok(status)
}

pub fn start(ctx: &Ctx, containers: &[String], attach: bool, interactive: bool) -> Result<()> {
    daemon::ensure(ctx)?;
    supervisor::ensure(ctx)?;
    let targets = resolve_all(ctx, containers)?;
    let mut failed = Vec::new();
    for c in &targets {
        let mut args: Vec<String> = vec!["start".into()];
        if attach {
            args.push("--attach".into());
        }
        if interactive {
            args.push("--interactive".into());
        }
        args.push(c.clone());
        if ctx.container(&args).status()?.success() {
            ctx.ok(&format!("{c} started"));
            report_urls(ctx, c);
        } else {
            failed.push(c.clone());
        }
    }
    supervisor::settle(ctx)?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("failed to start: {}", failed.join(", ")))
    }
}

fn all_running(ctx: &Ctx) -> Vec<String> {
    Snapshot::query_silent(ctx)
        .running_names()
        .into_iter()
        .map(String::from)
        .collect()
}

pub fn stop(
    ctx: &Ctx,
    containers: &[String],
    time: Option<u32>,
    signal: Option<&str>,
    all: bool,
) -> Result<()> {
    require_targets(all, containers, "stop")?;
    daemon::require(ctx)?;

    let targets = if all {
        all_running(ctx)
    } else {
        resolve_all(ctx, containers)?
    };

    let mut failed = Vec::new();
    for c in &targets {
        let stopped = match signal {
            Some(sig) => {
                let mut args: Vec<String> = vec!["stop".into(), "--signal".into(), sig.to_string()];
                if let Some(t) = time {
                    args.push("--time".into());
                    args.push(t.to_string());
                }
                args.push(c.clone());
                ctx.container(&args).status()?.success()
            }
            None => project::stop_container(ctx, c, time),
        };
        if stopped {
            ctx.ok(&format!("{c} stopped"));
        } else {
            failed.push(c.clone());
        }
    }
    supervisor::settle(ctx)?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("still running: {}", failed.join(", ")))
    }
}

pub fn restart(ctx: &Ctx, containers: &[String], time: Option<u32>) -> Result<()> {
    daemon::ensure(ctx)?;
    supervisor::ensure(ctx)?;
    let targets = resolve_all(ctx, containers)?;
    for c in &targets {
        if Snapshot::query_silent(ctx).state(c) == "running" {
            project::stop_container(ctx, c, time);
        }
    }
    let mut failed = Vec::new();
    for c in &targets {
        if ctx.container(["start", c]).status()?.success() {
            ctx.ok(&format!("{c} restarted"));
            report_urls(ctx, c);
        } else {
            failed.push(c.clone());
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("failed to restart: {}", failed.join(", ")))
    }
}

pub fn rm(ctx: &Ctx, containers: &[String], force: bool, all: bool) -> Result<()> {
    require_targets(all, containers, "rm")?;
    daemon::require(ctx)?;

    let mut args: Vec<String> = vec!["rm".into()];
    if force {
        args.push("--force".into());
    }
    if all {
        args.push("--all".into());
    } else {
        args.extend(resolve_all(ctx, containers)?);
    }
    let status = ctx.container(&args).status()?;
    supervisor::settle(ctx)?;
    exit_ok(status)
}

#[allow(clippy::too_many_arguments)]
pub fn exec(
    ctx: &Ctx,
    container: &str,
    command: &[String],
    tty: bool,
    detach: bool,
    env: &[String],
    workdir: Option<&str>,
    user: Option<&str>,
) -> Result<ExitStatus> {
    daemon::require(ctx)?;
    let cname = resolve(ctx, container)?;

    let mut args: Vec<String> = vec!["exec".into(), "-i".into()];
    if detach {
        args.push("--detach".into());
    }
    if wants_tty(tty || auto_tty()) {
        args.push("-t".into());
    } else if tty {
        ctx.warn("not allocating a TTY: stdin and stdout are not both terminals");
    }
    for e in env {
        args.push("--env".into());
        args.push(e.clone());
    }
    if let Some(w) = workdir {
        args.push("--workdir".into());
        args.push(w.to_string());
    }
    if let Some(u) = user {
        args.push("--user".into());
        args.push(u.to_string());
    }
    args.push(cname);
    args.extend(command.iter().cloned());
    ctx.container(&args).status()
}

pub fn sh(ctx: &Ctx, container: &str) -> Result<ExitStatus> {
    daemon::require(ctx)?;
    let cname = resolve(ctx, container)?;
    let has_bash = ctx
        .container(["exec", &cname, "sh", "-c", "command -v bash"])
        .silent()
        .quiet_ok();
    let shell = if has_bash { "bash" } else { "sh" };
    exec(
        ctx,
        &cname,
        &[shell.to_string()],
        true,
        false,
        &[],
        None,
        None,
    )
}

pub fn logs(
    ctx: &Ctx,
    container: &str,
    follow: bool,
    tail: Option<u64>,
    boot: bool,
) -> Result<ExitStatus> {
    daemon::require(ctx)?;
    let cname = resolve(ctx, container)?;
    let mut args: Vec<String> = vec!["logs".into()];
    if follow {
        args.push("--follow".into());
    }
    if boot {
        args.push("--boot".into());
    }
    if let Some(n) = tail {
        args.push("-n".into());
        args.push(n.to_string());
    }
    args.push(cname);
    ctx.container(&args).status()
}

pub fn inspect(ctx: &Ctx, containers: &[String]) -> Result<()> {
    daemon::require(ctx)?;
    let targets = resolve_all(ctx, containers)?;
    let mut args: Vec<String> = vec!["inspect".into()];
    args.extend(targets);
    let text = ctx.container(&args).stdout()?;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) if ctx.json => ctx.emit_json(&v),
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        Err(_) => {
            print!("{text}");
            Ok(())
        }
    }
}

pub fn kill(ctx: &Ctx, containers: &[String], signal: &str, all: bool) -> Result<()> {
    require_targets(all, containers, "kill")?;
    daemon::require(ctx)?;
    let mut args: Vec<String> = vec!["kill".into(), "--signal".into(), signal.to_string()];
    if all {
        args.push("--all".into());
    } else {
        args.extend(resolve_all(ctx, containers)?);
    }
    let status = ctx.container(&args).status()?;
    supervisor::settle(ctx)?;
    exit_ok(status)
}

fn rewrite_side(ctx: &Ctx, spec: &str) -> Result<String> {
    let Some((head, path)) = spec.split_once(':') else {
        return Ok(spec.to_string());
    };
    if head.is_empty() || Path::new(spec).exists() {
        return Ok(spec.to_string());
    }
    let cname = resolve(ctx, head)?;
    Ok(format!("{cname}:{path}"))
}

pub fn cp(ctx: &Ctx, src: &str, dst: &str) -> Result<()> {
    daemon::require(ctx)?;
    let s = rewrite_side(ctx, src)?;
    let d = rewrite_side(ctx, dst)?;
    ctx.warn("container cp is unreliable in Apple container 1.1.0; prefer `ac exec` with shell redirection");
    exit_ok(ctx.container(["cp", &s, &d]).status()?)
}

pub fn export(ctx: &Ctx, container: &str, output: Option<&Path>) -> Result<()> {
    daemon::require(ctx)?;
    let cname = resolve(ctx, container)?;
    if Snapshot::query_silent(ctx).state(&cname) == "running" {
        return Err(anyhow!(
            "{cname} is running; Apple container can only export a stopped container\n  stop it first: ac stop {cname}"
        ));
    }
    let default = format!("{cname}.tar");
    let out = output
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(default);
    let status = ctx.container(["export", "-o", &out, &cname]).status()?;
    if status.success() {
        ctx.ok(&format!("exported {cname} to {out}"));
    }
    exit_ok(status)
}

pub fn stats(ctx: &Ctx, containers: &[String], no_stream: bool) -> Result<()> {
    daemon::require(ctx)?;
    let targets = if containers.is_empty() {
        Vec::new()
    } else {
        resolve_all(ctx, containers)?
    };

    if ctx.json {
        let mut args: Vec<String> = vec![
            "stats".into(),
            "--no-stream".into(),
            "--format".into(),
            "json".into(),
        ];
        args.extend(targets);
        let text = ctx.container(&args).stdout_timeout(20)?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        return ctx.emit_json(&v);
    }

    let mut args: Vec<String> = vec!["stats".into()];
    if no_stream {
        args.push("--no-stream".into());
    }
    args.extend(targets);
    exit_ok(ctx.container(&args).status()?)
}

pub fn top(ctx: &Ctx, containers: &[String]) -> Result<()> {
    daemon::require(ctx)?;
    let targets = if containers.is_empty() {
        all_running(ctx)
    } else {
        resolve_all(ctx, containers)?
    };
    if targets.is_empty() {
        ctx.warn("no running containers");
        return Ok(());
    }
    for c in &targets {
        ctx.info(c);
        let ok = ctx
            .container(["exec", c, "ps", "aux"])
            .silent()
            .status()?
            .success();
        if !ok {
            ctx.container(["exec", c, "ps"]).silent().status().ok();
        }
    }
    Ok(())
}

pub fn port(ctx: &Ctx, container: &str) -> Result<()> {
    daemon::require(ctx)?;
    let cname = resolve(ctx, container)?;
    let text = ctx.container(["inspect", &cname]).stdout()?;
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let ports = v
        .get(0)
        .and_then(|c| c.get("configuration"))
        .and_then(|c| c.get("publishedPorts"))
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    if ctx.json {
        return ctx.emit_json(&serde_json::Value::Array(ports));
    }
    if ports.is_empty() {
        ctx.warn(&format!("{cname} publishes no ports"));
        if let Some(ip) = Snapshot::query_silent(ctx).ip(&cname) {
            ctx.dim(&format!(
                "  reachable directly at {}",
                ip.split('/').next().unwrap_or(&ip)
            ));
        }
        return Ok(());
    }
    for p in &ports {
        let cp = p.get("containerPort").and_then(|x| x.as_u64()).unwrap_or(0);
        let hp = p.get("hostPort").and_then(|x| x.as_u64()).unwrap_or(0);
        let proto = p
            .get("proto")
            .and_then(|x| x.as_str())
            .unwrap_or("tcp")
            .to_string();
        let addr = p
            .get("hostAddress")
            .and_then(|x| x.as_str())
            .unwrap_or("0.0.0.0");
        println!("{cp}/{proto} -> {addr}:{hp}");
    }
    Ok(())
}
