use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::build::{interpolate, Vars};
use crate::core::ctx::Ctx;
use crate::core::state::Snapshot;
use crate::core::style;
use crate::core::util::Table;
use crate::daemon::{self, supervisor};
use crate::manifest::{json_scalar, Project, Service};

pub fn existing_volumes(ctx: &Ctx) -> Vec<String> {
    ctx.container(["volume", "ls", "--format", "json"])
        .stdout()
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| {
                            e.get("id")
                                .and_then(|x| x.as_str())
                                .or_else(|| {
                                    e.get("configuration")
                                        .and_then(|c| c.get("name"))
                                        .and_then(|x| x.as_str())
                                })
                                .map(String::from)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

fn ensure_volumes(ctx: &Ctx, proj: &Project, svc: &Service) {
    if svc.volumes.is_empty() {
        return;
    }
    let existing = existing_volumes(ctx);

    for vol in &svc.volumes {
        let full = proj.volume_name(&vol.name);
        if existing.iter().any(|e| e == &full) {
            continue;
        }
        if ctx.container(["volume", "create", &full]).quiet_ok() {
            ctx.dim(&format!("  volume {full} created"));
        }
    }
}

fn poll_ready(ctx: &Ctx, cname: &str, svc: &Service, timeout: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout);
    if !ctx.json {
        print!("  waiting for {cname} ");
        std::io::stdout().flush().ok();
    }
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let ok = if svc.ready_cmd.is_empty() {
            Snapshot::query_silent(ctx).state(cname) == "running"
        } else {
            let mut argv: Vec<String> = vec!["exec".into(), cname.to_string()];
            argv.extend(svc.ready_cmd.iter().cloned());
            let probe_cap = remaining.as_secs().clamp(2, 20);
            ctx.container(&argv)
                .echo_once()
                .quiet_ok_timeout(probe_cap)
                .unwrap_or(false)
        };
        if ok {
            if !ctx.json {
                println!(" {}", style::green("ready"));
            }
            return true;
        }
        if !ctx.json {
            print!(".");
            std::io::stdout().flush().ok();
        }
        if std::time::Instant::now() + Duration::from_secs(2) >= deadline {
            break;
        }
        thread::sleep(Duration::from_secs(2));
    }
    if !ctx.json {
        println!(" {}", style::yellow("timeout"));
    }
    false
}

fn wait_ready(ctx: &Ctx, cname: &str, svc: &Service) {
    if svc.ready_cmd.is_empty() {
        return;
    }
    if !poll_ready(ctx, cname, svc, svc.ready_timeout) {
        ctx.warn(&format!(
            "{cname} did not become ready within {}s (continuing)",
            svc.ready_timeout
        ));
    }
}

pub fn wait(ctx: &Ctx, proj: &Project, services: &[String], timeout: Option<u64>) -> Result<()> {
    let targets = proj.target_services(services)?;
    daemon::require(ctx)?;

    let mut results: Vec<(String, bool)> = Vec::new();
    for name in &targets {
        let Some(svc) = proj.manifest.service(name) else {
            continue;
        };
        let cname = proj.container_name(name);
        let limit = timeout.unwrap_or(svc.ready_timeout);
        results.push((name.clone(), poll_ready(ctx, &cname, svc, limit)));
    }

    if ctx.json {
        let items: Vec<serde_json::Value> = results
            .iter()
            .map(|(name, ready)| serde_json::json!({ "service": name, "ready": ready }))
            .collect();
        ctx.emit_json(&serde_json::Value::Array(items))?;
    }

    let failed: Vec<&str> = results
        .iter()
        .filter(|(_, ready)| !ready)
        .map(|(n, _)| n.as_str())
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("not ready: {}", failed.join(", ")))
    }
}

fn resource_args(
    proj: &Project,
    svc: &Service,
    cname: &str,
    ports: bool,
    volumes: bool,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--name".into(),
        cname.to_string(),
        "--label".into(),
        format!("ac.project={}", proj.name),
    ];
    if let Some(c) = svc.cpus {
        args.push("--cpus".into());
        args.push(c.to_string());
    }
    if let Some(m) = &svc.memory {
        args.push("--memory".into());
        args.push(m.clone());
    }
    for (k, v) in &svc.env {
        args.push("--env".into());
        args.push(format!("{k}={}", json_scalar(v)));
    }
    if ports {
        for p in &svc.ports {
            args.push("--publish".into());
            args.push(p.clone());
        }
    }
    if volumes {
        for v in &svc.volumes {
            args.push("--volume".into());
            args.push(format!("{}:{}", proj.volume_name(&v.name), v.target));
        }
    }
    args
}

fn run_args(proj: &Project, svc: &Service, cname: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "--progress".into(),
        "none".into(),
    ];
    args.extend(resource_args(proj, svc, cname, true, true));
    args.push(svc.image.clone());
    args.extend(svc.args.iter().cloned());
    args
}

fn create_args(proj: &Project, svc: &Service, cname: &str) -> Vec<String> {
    let mut args: Vec<String> = vec!["create".into()];
    args.extend(resource_args(proj, svc, cname, true, true));
    args.push(svc.image.clone());
    args.extend(svc.args.iter().cloned());
    args
}

pub fn start_service(ctx: &Ctx, proj: &Project, name: &str, recreate: bool) -> Result<()> {
    let svc = proj
        .manifest
        .service(name)
        .ok_or_else(|| anyhow!("no such service '{name}' in project '{}'", proj.name))?;
    let cname = proj.container_name(name);

    let snap = Snapshot::query(ctx);
    let state = snap.state(&cname);

    if state == "running" {
        if !recreate {
            ctx.ok(&format!("{cname} already running"));
            return Ok(());
        }
        ctx.info(&format!("stopping {cname} to recreate it"));
        stop_container(ctx, &cname, None);
    }

    if state == "running" || state == "stopped" || state == "exited" || state == "created" {
        if recreate {
            ctx.dim(&format!("  recreating {cname}"));
            ctx.container(["rm", &cname]).quiet_ok();
        } else {
            ctx.info(&format!("restarting {cname}"));
            if ctx.container(["start", &cname]).quiet_ok() {
                wait_ready(ctx, &cname, svc);
                report_up(ctx, &cname);
                return Ok(());
            }
            ctx.dim("  restart failed, recreating");
            ctx.container(["rm", &cname]).quiet_ok();
        }
    }

    ensure_volumes(ctx, proj, svc);

    ctx.info(&format!("starting {cname}"));
    let args = run_args(proj, svc, &cname);
    if !ctx.container(&args).quiet_ok() {
        thread::sleep(Duration::from_secs(2));
        if Snapshot::query(ctx).state(&cname) != "running" {
            return Err(anyhow!("failed to start {cname}"));
        }
        ctx.dim(&format!(
            "  {cname} reported an error but is running; continuing"
        ));
    }

    wait_ready(ctx, &cname, svc);
    report_up(ctx, &cname);
    Ok(())
}

fn report_up(ctx: &Ctx, cname: &str) {
    let ip = Snapshot::query(ctx).ip(cname).unwrap_or_default();
    ctx.ok(&format!("{cname} up  {}", style::dim(&ip)));
}

pub fn start(ctx: &Ctx, proj: &Project, services: &[String], recreate: bool) -> Result<()> {
    let targets = proj.target_services(services)?;

    daemon::ensure(ctx)?;

    let vars = Vars::default();
    let images: Vec<String> = proj
        .manifest
        .services
        .iter()
        .map(|s| s.image.clone())
        .collect();
    login(ctx, proj, &vars, &images).ok();

    for svc in &targets {
        start_service(ctx, proj, svc, recreate)?;
    }
    supervisor::ensure(ctx)?;
    Ok(())
}

pub fn run_once(
    ctx: &Ctx,
    proj: &Project,
    service: &str,
    command: &[String],
    keep: bool,
    extra_env: &[String],
    no_volumes: bool,
) -> Result<std::process::ExitStatus> {
    let name = proj.target_services(std::slice::from_ref(&service.to_string()))?[0].clone();
    let svc = proj
        .manifest
        .service(&name)
        .ok_or_else(|| anyhow!("no such service '{name}'"))?;
    let cname = format!(
        "{}-{}-run-{}",
        proj.name,
        name,
        crate::core::ctx::now_stamp()
    );

    daemon::ensure(ctx)?;
    let vars = Vars::default();
    login(ctx, proj, &vars, std::slice::from_ref(&svc.image)).ok();
    if !no_volumes {
        ensure_volumes(ctx, proj, svc);
    }

    let mut args: Vec<String> = vec!["run".into()];
    if !keep {
        args.push("--rm".into());
    }
    args.push("-i".into());
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        args.push("-t".into());
    }
    args.extend(resource_args(proj, svc, &cname, false, !no_volumes));
    for kv in extra_env {
        args.push("--env".into());
        args.push(kv.clone());
    }
    args.push(svc.image.clone());
    if command.is_empty() {
        args.extend(svc.args.iter().cloned());
    } else {
        args.extend(command.iter().cloned());
    }

    ctx.info(&format!("running one-off {cname}"));
    let status = ctx.container(&args).status()?;
    supervisor::settle(ctx)?;
    if keep {
        ctx.dim(&format!(
            "  kept as {cname} (remove with: container rm {cname})"
        ));
    }
    Ok(status)
}

pub fn create(ctx: &Ctx, proj: &Project, services: &[String], recreate: bool) -> Result<()> {
    let targets = proj.target_services(services)?;
    daemon::ensure(ctx)?;

    let vars = Vars::default();
    let images: Vec<String> = proj
        .manifest
        .services
        .iter()
        .map(|s| s.image.clone())
        .collect();
    login(ctx, proj, &vars, &images).ok();

    let snap = Snapshot::query(ctx);
    for name in &targets {
        let Some(svc) = proj.manifest.service(name) else {
            continue;
        };
        let cname = proj.container_name(name);
        let state = snap.state(&cname);
        if state != "absent" {
            if !recreate {
                ctx.dim(&format!("  {cname} already exists ({state})"));
                continue;
            }
            if state == "running" {
                ctx.info(&format!("stopping {cname}"));
                ctx.container(["stop", &cname]).quiet_ok();
            }
            ctx.container(["rm", &cname]).quiet_ok();
        }
        ensure_volumes(ctx, proj, svc);
        ctx.info(&format!("creating {cname}"));
        if ctx.container(create_args(proj, svc, &cname)).quiet_ok() {
            ctx.ok(&format!(
                "{cname} created (start with: ac {} start {name})",
                proj.name
            ));
        } else {
            return Err(anyhow!("failed to create {cname}"));
        }
    }
    supervisor::settle(ctx)
}

pub fn top(ctx: &Ctx, proj: &Project, services: &[String]) -> Result<()> {
    let targets = proj.target_services(services)?;
    daemon::require(ctx)?;
    let snap = Snapshot::query(ctx);

    let mut items: Vec<serde_json::Value> = Vec::new();
    for name in &targets {
        let cname = proj.container_name(name);
        if snap.state(&cname) != "running" {
            if !ctx.json {
                ctx.dim(&format!("  {cname} not running"));
            }
            continue;
        }
        let out = ctx
            .container(["exec", &cname, "sh", "-c", "ps aux 2>/dev/null || ps"])
            .stdout()
            .unwrap_or_default();
        if ctx.json {
            let lines: Vec<&str> = out.lines().collect();
            items.push(serde_json::json!({
                "service": name,
                "container": cname,
                "processes": lines,
            }));
        } else {
            ctx.log(&style::bold(&cname));
            for l in out.lines() {
                ctx.log(&format!("  {l}"));
            }
        }
    }
    if ctx.json {
        return ctx.emit_json(&serde_json::Value::Array(items));
    }
    Ok(())
}

pub fn export(ctx: &Ctx, proj: &Project, service: &str, output: Option<&Path>) -> Result<()> {
    let name = proj.target_services(std::slice::from_ref(&service.to_string()))?[0].clone();
    let cname = proj.container_name(&name);
    daemon::require(ctx)?;

    if Snapshot::query_silent(ctx).state(&cname) == "running" {
        return Err(anyhow!(
            "Apple container can only export a stopped container; run `ac {} stop {name}` first",
            proj.name
        ));
    }

    let path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("{cname}.tar")));
    ctx.info(&format!("exporting {cname} to {}", path.display()));
    let status = ctx
        .container(["export", "-o", &path.to_string_lossy(), &cname])
        .status()?;
    if !status.success() {
        return Err(anyhow!("export of {cname} failed"));
    }
    ctx.ok(&format!("{}", path.display()));
    Ok(())
}

pub fn pull(ctx: &Ctx, proj: &Project, services: &[String]) -> Result<()> {
    let targets = proj.target_services(services)?;
    daemon::ensure(ctx)?;

    let vars = Vars::default();
    let images: Vec<String> = proj
        .manifest
        .services
        .iter()
        .map(|s| s.image.clone())
        .collect();
    login(ctx, proj, &vars, &images).ok();

    for svc in &targets {
        let Some(s) = proj.manifest.service(svc) else {
            continue;
        };
        ctx.info(&format!("pulling {}", s.image));
        if ctx.container(["image", "pull", &s.image]).quiet_ok() {
            ctx.ok(&s.image);
        } else {
            ctx.warn(&format!("failed to pull {}", s.image));
        }
    }
    Ok(())
}

pub fn login(ctx: &Ctx, proj: &Project, vars: &Vars, images: &[String]) -> Result<()> {
    if proj.manifest.registries.is_empty() {
        return Ok(());
    }

    for reg in &proj.manifest.registries {
        let server = interpolate(&reg.server, vars);

        if server.is_empty() || server.starts_with('.') || server.contains("{{") {
            continue;
        }
        if !images.is_empty() && !images.iter().any(|i| i.contains(&server)) {
            continue;
        }

        let argv: Vec<String> = reg
            .password_cmd
            .iter()
            .map(|a| interpolate(a, vars))
            .collect();
        if argv.is_empty() {
            ctx.warn(&format!("registry {server} declares an empty passwordCmd"));
            continue;
        }

        ctx.info(&format!("logging in to {server}"));
        let pass = ctx.exec(&argv[0], &argv[1..]).output();
        let Ok(out) = pass else {
            ctx.warn(&format!(
                "login to {server} failed; passwordCmd could not run"
            ));
            continue;
        };
        if !out.status.success() {
            ctx.warn(&format!(
                "login to {server} failed; pulls of private images will fail"
            ));
            continue;
        }

        let mut child = ctx
            .container([
                "registry",
                "login",
                "--username",
                reg.username.as_str(),
                "--password-stdin",
                server.as_str(),
            ])
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&out.stdout).ok();
        }
        if child.wait()?.success() {
            ctx.ok(&format!("authenticated to {server}"));
        } else {
            ctx.warn(&format!(
                "login to {server} failed; pulls of private images will fail"
            ));
        }
    }
    Ok(())
}

fn stop_args(cname: &str, time: Option<u32>) -> Vec<String> {
    let mut args = vec!["stop".to_string()];
    if let Some(t) = time {
        args.push("--time".into());
        args.push(t.to_string());
    }
    args.push(cname.to_string());
    args
}

fn kill_runtime_shim(cname: &str) -> bool {
    let pattern = format!("container-runtime-linux.*--uuid {cname}$");
    crate::core::ctx::echo_external("pgrep", &["-f", &pattern]);
    let out = std::process::Command::new("pgrep")
        .args(["-f", &pattern])
        .output();
    let Ok(out) = out else {
        return false;
    };
    let pids: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if pids.is_empty() {
        return false;
    }
    for pid in &pids {
        crate::core::ctx::echo_external("/bin/kill", &["-9", pid]);
        std::process::Command::new("/bin/kill")
            .args(["-9", pid])
            .status()
            .ok();
    }
    true
}

pub fn stop_container(ctx: &Ctx, cname: &str, time: Option<u32>) -> bool {
    let grace = u64::from(time.unwrap_or(5));
    let deadline = (grace * 2).max(20);
    if ctx
        .container(stop_args(cname, time))
        .quiet_ok_timeout(deadline)
        == Some(true)
        && Snapshot::query_silent(ctx).state(cname) != "running"
    {
        return true;
    }
    if Snapshot::query_silent(ctx).state(cname) != "running" {
        return true;
    }

    ctx.warn(&format!(
        "{cname} ignored container stop; escalating to SIGKILL"
    ));
    ctx.container(["kill", "--signal", "KILL", cname])
        .quiet_ok_timeout(10);
    if Snapshot::query_silent(ctx).state(cname) != "running" {
        return true;
    }

    ctx.warn(&format!(
        "{cname}'s runtime shim is wedged (known Apple container issue); terminating it"
    ));
    if !kill_runtime_shim(cname) {
        return false;
    }
    thread::sleep(Duration::from_secs(2));
    Snapshot::query_silent(ctx).state(cname) != "running"
}

pub fn stop(ctx: &Ctx, proj: &Project, services: &[String], time: Option<u32>) -> Result<()> {
    let targets = proj.target_services(services)?;
    let snap = Snapshot::query(ctx);

    let mut stopped: Vec<String> = Vec::new();
    for svc in &targets {
        let cname = proj.container_name(svc);
        match snap.state(&cname).as_str() {
            "absent" => ctx.dim(&format!("  {cname} not created")),
            "running" => {
                ctx.info(&format!("stopping {cname}"));
                stop_container(ctx, &cname, time);
                stopped.push(cname);
            }
            other => ctx.dim(&format!("  {cname} already {other}")),
        }
    }

    if stopped.is_empty() {
        ctx.dim("nothing to stop, no service was running");
    } else {
        let after = Snapshot::query_silent(ctx);
        let mut failed = 0;
        for cname in &stopped {
            if after.state(cname) == "running" {
                ctx.warn(&format!("{cname} is still running despite the stop"));
                failed += 1;
            } else {
                ctx.ok(&format!("{cname} stopped"));
            }
        }
        if failed > 0 {
            supervisor::settle(ctx)?;
            return Err(anyhow!("{failed} container(s) did not stop"));
        }
    }

    supervisor::settle(ctx)
}

pub fn down(
    ctx: &Ctx,
    proj: &Project,
    services: &[String],
    time: Option<u32>,
    volumes: bool,
) -> Result<()> {
    let targets = proj.target_services(services)?;
    let snap = Snapshot::query(ctx);

    let mut removed = 0;
    for svc in &targets {
        let cname = proj.container_name(svc);
        let state = snap.state(&cname);
        if state == "absent" {
            ctx.dim(&format!("  {cname} not created"));
            continue;
        }
        if state == "running" {
            ctx.info(&format!("stopping {cname}"));
            stop_container(ctx, &cname, time);
        }
        ctx.container(["rm", &cname]).quiet_ok();
        ctx.ok(&format!("{cname} removed"));
        removed += 1;
    }

    if removed == 0 {
        ctx.dim("nothing to remove, every service was already absent");
    }

    if volumes {
        let mut failed = 0;
        for svc in &targets {
            let Some(s) = proj.manifest.service(svc) else {
                continue;
            };
            for vol in &s.volumes {
                let full = proj.volume_name(&vol.name);
                ctx.info(&format!("deleting volume {full} (data is gone)"));
                if !ctx.container(["volume", "delete", &full]).quiet_ok() {
                    ctx.warn(&format!("could not delete {full}"));
                    failed += 1;
                }
            }
        }
        if failed > 0 {
            supervisor::settle(ctx)?;
            return Err(anyhow!("{failed} volume(s) could not be deleted"));
        }
    }

    supervisor::settle(ctx)
}

pub fn remove(ctx: &Ctx, proj: &Project, services: &[String]) -> Result<()> {
    let targets = proj.target_services(services)?;
    let snap = Snapshot::query(ctx);

    let mut failed = 0;
    let mut removed = 0;
    for svc in &targets {
        let cname = proj.container_name(svc);
        if snap.state(&cname) == "absent" {
            ctx.dim(&format!("  {cname} not created"));
            continue;
        }
        if ctx.container(["rm", "--force", &cname]).status()?.success() {
            ctx.ok(&format!("{cname} removed"));
            removed += 1;
        } else {
            ctx.warn(&format!("could not remove {cname}"));
            failed += 1;
        }
    }
    if removed == 0 && failed == 0 {
        ctx.dim("nothing to remove, every service was already absent");
    }

    supervisor::settle(ctx)?;
    if failed > 0 {
        return Err(anyhow!("{failed} container(s) could not be removed"));
    }
    Ok(())
}

pub struct ServiceStatus {
    pub service: String,
    pub container: String,
    pub state: String,
    pub ip: Option<String>,
    pub ports: Vec<String>,
    pub image: String,
}

pub fn status_rows(ctx: &Ctx, proj: &Project) -> Vec<ServiceStatus> {
    let snap = Snapshot::query(ctx);
    proj.manifest
        .services
        .iter()
        .map(|s| {
            let cname = proj.container_name(&s.name);
            ServiceStatus {
                state: snap.state(&cname),
                ip: snap.ip(&cname),
                service: s.name.clone(),
                ports: s.ports.clone(),
                image: s.image.clone(),
                container: cname,
            }
        })
        .collect()
}

pub fn print_status(ctx: &Ctx, proj: &Project, rows: &[ServiceStatus]) {
    if !daemon::running_silent(ctx) {
        ctx.warn(&format!(
            "container daemon is not running - state unknown (run: ac {} start)",
            proj.name
        ));
    }
    let mut table = Table::new(&["CONTAINER", "STATE", "IP", "PORTS"]);
    for r in rows {
        table.row([
            r.container.clone(),
            r.state.clone(),
            r.ip.clone().unwrap_or_else(|| "-".into()),
            if r.ports.is_empty() {
                "-".to_string()
            } else {
                r.ports.join(",")
            },
        ]);
    }
    table.print(ctx);
}

pub fn status_json(rows: &[ServiceStatus]) -> serde_json::Value {
    serde_json::Value::Array(
        rows.iter()
            .map(|r| {
                serde_json::json!({
                    "service": r.service,
                    "container": r.container,
                    "state": r.state,
                    "ip": r.ip,
                    "ports": r.ports,
                    "image": r.image,
                })
            })
            .collect(),
    )
}

fn spawn_prefixer<R: std::io::Read + Send + 'static>(
    stream: R,
    name: String,
    color: owo_colors::AnsiColors,
    stopping: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if stopping.load(Ordering::SeqCst) {
                break;
            }
            println!("{} | {line}", style::colored(&format!("{name:<11}"), color));
        }
    })
}

pub fn logs_all(ctx: &Ctx, proj: &Project, flags: &[String]) -> Result<()> {
    let palette = style::LOG_PALETTE;

    let mut children = Vec::new();
    for (i, s) in proj.manifest.services.iter().enumerate() {
        let cname = proj.container_name(&s.name);
        let mut argv: Vec<String> = vec!["logs".into()];
        argv.extend(flags.iter().cloned());
        argv.push(cname);
        let child = ctx.container(&argv).spawn_piped()?;
        children.push((s.name.clone(), palette[i % palette.len()], child));
    }

    let stopping = Arc::new(AtomicBool::new(false));
    {
        let stopping = stopping.clone();
        ctrlc::set_handler(move || stopping.store(true, Ordering::SeqCst)).ok();
    }

    let mut handles = Vec::new();
    let mut kill_list = Vec::new();
    for (name, color, mut child) in children {
        if let Some(out) = child.stdout.take() {
            handles.push(spawn_prefixer(out, name.clone(), color, stopping.clone()));
        }
        if let Some(err) = child.stderr.take() {
            handles.push(spawn_prefixer(err, name.clone(), color, stopping.clone()));
        }
        kill_list.push(child);
    }

    let watcher = {
        let stopping = stopping.clone();
        thread::spawn(move || loop {
            if stopping.load(Ordering::SeqCst) {
                for c in kill_list.iter_mut() {
                    c.kill().ok();
                }
                return;
            }
            let all_done = kill_list
                .iter_mut()
                .all(|c| matches!(c.try_wait(), Ok(Some(_))));
            if all_done {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        })
    };

    for h in handles {
        h.join().ok();
    }
    stopping.store(true, Ordering::SeqCst);
    watcher.join().ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj() -> Project {
        let raw = r#"{
            "name": "demo",
            "services": [
              {
                "name": "db",
                "image": "docker.io/library/postgres:16-alpine",
                "cpus": 2,
                "memory": "1g",
                "ports": ["5433:5432"],
                "env": { "POSTGRES_USER": "user" },
                "volumes": [{ "name": "db-data", "target": "/var/lib/postgresql/data" }],
                "args": ["postgres", "-c", "max_connections=50"]
              }
            ]
        }"#;
        Project {
            name: "demo".into(),
            file: std::path::PathBuf::from("/tmp/demo.json"),
            manifest: serde_json::from_str(raw).unwrap(),
            raw: raw.into(),
        }
    }

    #[test]
    fn run_args_carry_everything_detached() {
        let p = proj();
        let svc = p.manifest.service("db").unwrap();
        let args = run_args(&p, svc, "demo-db");
        let joined = args.join(" ");
        assert!(joined.starts_with("run -d --progress none"), "{joined}");
        assert!(joined.contains("--name demo-db"), "{joined}");
        assert!(joined.contains("--label ac.project=demo"), "{joined}");
        assert!(joined.contains("--cpus 2"), "{joined}");
        assert!(joined.contains("--memory 1g"), "{joined}");
        assert!(joined.contains("--env POSTGRES_USER=user"), "{joined}");
        assert!(joined.contains("--publish 5433:5432"), "{joined}");
        assert!(
            joined.contains("--volume demo-db-data:/var/lib/postgresql/data"),
            "{joined}"
        );
        assert!(
            joined.ends_with("docker.io/library/postgres:16-alpine postgres -c max_connections=50"),
            "{joined}"
        );
    }

    #[test]
    fn create_args_match_run_args_without_the_run_preamble() {
        let p = proj();
        let svc = p.manifest.service("db").unwrap();
        let run = run_args(&p, svc, "demo-db");
        let create = create_args(&p, svc, "demo-db");
        assert_eq!(create[0], "create");
        assert_eq!(&run[4..], &create[1..]);
    }

    #[test]
    fn resource_args_can_drop_ports_and_volumes() {
        let p = proj();
        let svc = p.manifest.service("db").unwrap();
        let no_ports = resource_args(&p, svc, "demo-db-run-1", false, true).join(" ");
        assert!(!no_ports.contains("--publish"), "{no_ports}");
        assert!(no_ports.contains("--volume"), "{no_ports}");
        let bare = resource_args(&p, svc, "demo-db-run-1", false, false).join(" ");
        assert!(!bare.contains("--volume"), "{bare}");
        assert!(bare.contains("--env POSTGRES_USER=user"), "{bare}");
    }
}
