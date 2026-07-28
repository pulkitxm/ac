use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitStatus;

use anyhow::{anyhow, Result};

use crate::cli::{BuilderAction, RunOpts};
use crate::ctx::Ctx;
use crate::state::Snapshot;
use crate::{daemon, manifest, project, supervisor};

pub const MANAGED_LABEL: &str = "ac.managed=1";

fn exit_ok(status: ExitStatus) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("command exited {status}"))
    }
}

fn wants_tty(asked: bool) -> bool {
    asked && std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn auto_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn normalize(name: &str) -> String {
    match name.split_once('/') {
        Some((p, s)) if !p.is_empty() && !s.is_empty() => format!("{p}-{s}"),
        _ => name.to_string(),
    }
}

fn resolve(ctx: &Ctx, name: &str) -> Result<String> {
    let want = normalize(name);
    let snap = Snapshot::query_silent(ctx);
    if snap.get(&want).is_some() {
        return Ok(want);
    }

    let projects = manifest::project_names(&ctx.config_dir, &ctx.ac_home);
    if projects.iter().any(|p| p == &want) {
        return Err(anyhow!(
            "'{want}' is a project, not a container\n  try: ac {want} status, or name a service directly (ac logs {want}-<service>)"
        ));
    }

    let mut known: Vec<&str> = snap.items.iter().map(|c| c.id.as_str()).collect();
    known.sort_unstable();
    if known.is_empty() {
        Err(anyhow!("no such container: {want}; none exist right now"))
    } else {
        Err(anyhow!(
            "no such container: {want}\n  containers: {}",
            known.join(" ")
        ))
    }
}

fn resolve_all(ctx: &Ctx, names: &[String]) -> Result<Vec<String>> {
    names.iter().map(|n| resolve(ctx, n)).collect()
}

fn require_targets(all: bool, containers: &[String], verb: &str) -> Result<()> {
    if all || !containers.is_empty() {
        return Ok(());
    }
    Err(anyhow!(
        "{verb} needs a container name, or --all\n  for a whole project stack: ac <project> {verb}"
    ))
}

fn run_opts_argv(opts: &RunOpts, progress: bool) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();

    let mut flag = |name: &str, val: &Option<String>| {
        if let Some(v) = val {
            a.push(name.to_string());
            a.push(v.clone());
        }
    };
    flag("--name", &opts.name);
    flag("--shm-size", &opts.shm_size);
    flag("--user", &opts.user);
    flag("--uid", &opts.uid);
    flag("--gid", &opts.gid);
    flag("--workdir", &opts.workdir);
    flag("--entrypoint", &opts.entrypoint);
    flag("--network", &opts.network);
    flag("--platform", &opts.platform);
    flag("--arch", &opts.arch);
    flag("--os", &opts.os);
    flag("--init-image", &opts.init_image);
    flag("--kernel", &opts.kernel);
    flag("--runtime", &opts.runtime);
    flag("--dns-domain", &opts.dns_domain);
    flag("--cidfile", &opts.cidfile);
    flag("--scheme", &opts.scheme);
    if progress {
        flag("--progress", &opts.progress);
    }

    if let Some(c) = opts.cpus {
        a.push("--cpus".into());
        a.push(c.to_string());
    }
    if let Some(m) = &opts.memory {
        a.push("--memory".into());
        a.push(m.clone());
    }
    if let Some(n) = opts.max_concurrent_downloads {
        a.push("--max-concurrent-downloads".into());
        a.push(n.to_string());
    }

    let mut repeat = |name: &str, vals: &Vec<String>| {
        for v in vals {
            a.push(name.to_string());
            a.push(v.clone());
        }
    };
    repeat("--env", &opts.env);
    repeat("--env-file", &opts.env_file);
    repeat("--publish", &opts.publish);
    repeat("--publish-socket", &opts.publish_socket);
    repeat("--volume", &opts.volume);
    repeat("--mount", &opts.mount);
    repeat("--tmpfs", &opts.tmpfs);
    repeat("--label", &opts.label);
    repeat("--ulimit", &opts.ulimit);
    repeat("--cap-add", &opts.cap_add);
    repeat("--cap-drop", &opts.cap_drop);
    repeat("--dns", &opts.dns);
    repeat("--dns-option", &opts.dns_option);
    repeat("--dns-search", &opts.dns_search);

    for (on, name) in [
        (opts.read_only, "--read-only"),
        (opts.init, "--init"),
        (opts.no_dns, "--no-dns"),
        (opts.ssh, "--ssh"),
        (opts.rosetta, "--rosetta"),
        (opts.virtualization, "--virtualization"),
    ] {
        if on {
            a.push(name.to_string());
        }
    }

    a.push("--label".into());
    a.push(MANAGED_LABEL.to_string());
    a
}

fn published_urls(ctx: &Ctx, cname: &str) -> Vec<String> {
    let Ok(text) = ctx.container(["inspect", cname]).silent().stdout() else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(ports) = v
        .get(0)
        .and_then(|c| c.get("configuration"))
        .and_then(|c| c.get("publishedPorts"))
        .and_then(|p| p.as_array())
    else {
        return Vec::new();
    };
    ports
        .iter()
        .filter(|p| p.get("proto").and_then(|x| x.as_str()) != Some("udp"))
        .filter_map(|p| p.get("hostPort").and_then(|x| x.as_u64()))
        .map(|port| format!("http://localhost:{port}"))
        .collect()
}

fn report_urls(ctx: &Ctx, cname: &str) {
    for url in published_urls(ctx, cname) {
        ctx.dim(&format!("  {url}"));
    }
}

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

#[allow(clippy::too_many_arguments)]
pub struct BuildArgs<'a> {
    pub tags: &'a [String],
    pub file: Option<&'a str>,
    pub target: Option<&'a str>,
    pub platform: Option<&'a str>,
    pub arch: Option<&'a str>,
    pub os: Option<&'a str>,
    pub build_args: &'a [String],
    pub labels: &'a [String],
    pub secrets: &'a [String],
    pub no_cache: bool,
    pub pull: bool,
    pub progress: Option<&'a str>,
    pub output: Option<&'a str>,
    pub cpus: Option<u32>,
    pub memory: Option<&'a str>,
    pub build_quiet: bool,
    pub context: &'a str,
}

pub fn build(ctx: &Ctx, b: &BuildArgs) -> Result<()> {
    daemon::ensure(ctx)?;
    crate::build::ensure_builder(ctx, b.cpus, b.memory);

    let mut args: Vec<String> = vec!["build".into()];
    for t in b.tags {
        args.push("--tag".into());
        args.push(t.clone());
    }
    let mut flag = |name: &str, val: Option<&str>| {
        if let Some(v) = val {
            args.push(name.to_string());
            args.push(v.to_string());
        }
    };
    flag("--file", b.file);
    flag("--target", b.target);
    flag("--platform", b.platform);
    flag("--arch", b.arch);
    flag("--os", b.os);
    flag("--progress", b.progress);
    flag("--output", b.output);

    for v in b.build_args {
        args.push("--build-arg".into());
        args.push(v.clone());
    }
    for v in b.labels {
        args.push("--label".into());
        args.push(v.clone());
    }
    for v in b.secrets {
        args.push("--secret".into());
        args.push(v.clone());
    }
    if b.no_cache {
        args.push("--no-cache".into());
    }
    if b.pull {
        args.push("--pull".into());
    }
    if b.build_quiet {
        args.push("--quiet".into());
    }
    args.push(b.context.to_string());

    let status = ctx.container(&args).status()?;
    supervisor::settle(ctx)?;

    if status.success() && !b.tags.is_empty() {
        for t in b.tags {
            ctx.ok(&format!("built {t}"));
        }
        ctx.dim(&format!("  run it: ac run -d -p 8080:8080 {}", b.tags[0]));
    }
    exit_ok(status)
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
                let mut args: Vec<String> =
                    vec!["stop".into(), "--signal".into(), sig.to_string()];
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

pub fn pull(ctx: &Ctx, reference: &str, platform: Option<&str>) -> Result<()> {
    daemon::ensure(ctx)?;
    let mut args: Vec<String> = vec!["image".into(), "pull".into()];
    if let Some(p) = platform {
        args.push("--platform".into());
        args.push(p.to_string());
    }
    args.push(reference.to_string());
    let status = ctx.container(&args).status()?;
    supervisor::settle(ctx)?;
    exit_ok(status)
}

pub fn push(ctx: &Ctx, reference: &str, platform: Option<&str>) -> Result<()> {
    daemon::ensure(ctx)?;
    let mut args: Vec<String> = vec!["image".into(), "push".into()];
    if let Some(p) = platform {
        args.push("--platform".into());
        args.push(p.to_string());
    }
    args.push(reference.to_string());
    let status = ctx.container(&args).status()?;
    supervisor::settle(ctx)?;
    exit_ok(status)
}

pub fn tag(ctx: &Ctx, source: &str, target: &str) -> Result<()> {
    daemon::ensure(ctx)?;
    let status = ctx.container(["image", "tag", source, target]).status()?;
    supervisor::settle(ctx)?;
    exit_ok(status)
}

pub fn save(ctx: &Ctx, reference: &str, output: &Path) -> Result<()> {
    daemon::require(ctx)?;
    let out = output.to_string_lossy().to_string();
    exit_ok(
        ctx.container(["image", "save", "-o", &out, reference])
            .status()?,
    )
}

pub fn load(ctx: &Ctx, input: &Path) -> Result<()> {
    daemon::ensure(ctx)?;
    let inp = input.to_string_lossy().to_string();
    let status = ctx.container(["image", "load", "-i", &inp]).status()?;
    supervisor::settle(ctx)?;
    exit_ok(status)
}

pub fn login(
    ctx: &Ctx,
    server: &str,
    username: Option<&str>,
    password: Option<&str>,
    password_stdin: bool,
) -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;

    daemon::ensure(ctx)?;
    let mut args: Vec<String> = vec!["registry".into(), "login".into()];
    if let Some(u) = username {
        args.push("--username".into());
        args.push(u.to_string());
    }
    if password_stdin || password.is_some() {
        args.push("--password-stdin".into());
    }
    args.push(server.to_string());

    let status = if let Some(p) = password.filter(|_| !password_stdin) {
        let mut child = ctx
            .container(&args)
            .command()
            .stdin(Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(p.as_bytes()).ok();
            stdin.write_all(b"\n").ok();
        }
        child.wait()?
    } else {
        ctx.container(&args).status()?
    };
    supervisor::settle(ctx)?;
    exit_ok(status)
}

pub fn logout(ctx: &Ctx, server: &str) -> Result<()> {
    daemon::require(ctx)?;
    exit_ok(ctx.container(["registry", "logout", server]).status()?)
}

pub fn builder(ctx: &Ctx, action: Option<&BuilderAction>) -> Result<()> {
    match action.unwrap_or(&BuilderAction::Status) {
        BuilderAction::Status => {
            daemon::require(ctx)?;
            if ctx.json {
                let text = ctx
                    .container(["builder", "status", "--format", "json"])
                    .stdout()?;
                let v: serde_json::Value = serde_json::from_str(&text)?;
                return ctx.emit_json(&v);
            }
            exit_ok(ctx.container(["builder", "status"]).status()?)
        }
        BuilderAction::Start { cpus, memory } => {
            daemon::ensure(ctx)?;
            let mut args: Vec<String> = vec!["builder".into(), "start".into()];
            if let Some(c) = cpus {
                args.push("--cpus".into());
                args.push(c.to_string());
            }
            if let Some(m) = memory {
                args.push("--memory".into());
                args.push(m.clone());
            }
            let status = ctx.container(&args).status()?;
            supervisor::settle(ctx)?;
            exit_ok(status)
        }
        BuilderAction::Stop => {
            daemon::require(ctx)?;
            let status = ctx.container(["builder", "stop"]).status()?;
            supervisor::settle(ctx)?;
            exit_ok(status)
        }
        BuilderAction::Delete { force } => {
            daemon::require(ctx)?;
            ctx.warn("deleting the builder discards its layer cache");
            let mut args: Vec<String> = vec!["builder".into(), "delete".into()];
            if *force {
                args.push("--force".into());
            }
            let status = ctx.container(&args).status()?;
            supervisor::settle(ctx)?;
            exit_ok(status)
        }
    }
}

pub fn machine(ctx: &Ctx, args: &[String]) -> Result<()> {
    let read_only = matches!(
        args.first().map(String::as_str),
        Some("ls") | Some("list") | Some("inspect") | Some("logs") | None
    );
    if read_only {
        daemon::require(ctx)?;
    } else {
        daemon::ensure(ctx)?;
    }
    let mut argv: Vec<String> = vec!["machine".into()];
    if args.is_empty() {
        argv.push("list".into());
    } else {
        argv.extend(args.iter().cloned());
    }
    let status = ctx.container(&argv).status()?;
    if !read_only {
        supervisor::settle(ctx)?;
    }
    exit_ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::RunOpts;

    #[test]
    fn slash_form_becomes_dashed_container_name() {
        assert_eq!(normalize("shop/redis"), "shop-redis");
        assert_eq!(normalize("shop-redis"), "shop-redis");
        assert_eq!(normalize("web"), "web");
        assert_eq!(normalize("/leading"), "/leading");
    }

    #[test]
    fn run_argv_always_carries_the_managed_label() {
        let opts = RunOpts::default();
        let argv = run_opts_argv(&opts, true);
        let joined = argv.join(" ");
        assert!(joined.contains("--label ac.managed=1"), "{joined}");
    }

    #[test]
    fn create_never_gets_progress_because_container_create_rejects_it() {
        let opts = RunOpts {
            progress: Some("plain".into()),
            ..RunOpts::default()
        };
        assert!(run_opts_argv(&opts, true).join(" ").contains("--progress"));
        assert!(!run_opts_argv(&opts, false).join(" ").contains("--progress"));
    }

    #[test]
    fn run_argv_maps_repeatable_and_scalar_flags() {
        let opts = RunOpts {
            name: Some("web".into()),
            publish: vec!["3000:3000".into(), "9229:9229".into()],
            env: vec!["A=1".into(), "B=2".into()],
            cpus: Some(4),
            memory: Some("2g".into()),
            read_only: true,
            ..RunOpts::default()
        };
        let joined = run_opts_argv(&opts, true).join(" ");
        assert!(joined.contains("--name web"), "{joined}");
        assert!(joined.contains("--publish 3000:3000"), "{joined}");
        assert!(joined.contains("--publish 9229:9229"), "{joined}");
        assert!(joined.contains("--env A=1"), "{joined}");
        assert!(joined.contains("--cpus 4"), "{joined}");
        assert!(joined.contains("--memory 2g"), "{joined}");
        assert!(joined.contains("--read-only"), "{joined}");
    }

    #[test]
    fn unset_flags_are_absent() {
        let joined = run_opts_argv(&RunOpts::default(), true).join(" ");
        assert!(!joined.contains("--name"), "{joined}");
        assert!(!joined.contains("--cpus"), "{joined}");
        assert!(!joined.contains("--rosetta"), "{joined}");
    }
}
