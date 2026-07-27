//! Turn a declarative project manifest into running containers.
//!
//! Adding a new project is dropping another JSON file into `projects/` or
//! `~/.config/ac/projects/`; no code changes are needed.
//!
//! Container naming convention: `<project>-<service>`. That is also how ac
//! recognises its own containers when deciding whether the daemon can be shut
//! down, so it stays stable and predictable.

use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::build::{interpolate, Vars};
use crate::ctx::Ctx;
use crate::manifest::{json_scalar, Project, Service};
use crate::state::Snapshot;
use crate::style;
use crate::{daemon, supervisor};

// ------------------------------------------------------------------ start ---

/// Create any named volumes a service declares that do not exist yet.
fn ensure_volumes(ctx: &Ctx, proj: &Project, svc: &Service) {
    if svc.volumes.is_empty() {
        return;
    }
    let existing = ctx
        .container(["volume", "ls", "--format", "json"])
        .silent()
        .stdout()
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.get("id").and_then(|x| x.as_str()).map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();

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

/// Poll a service's `readyCmd` until it succeeds. Apple `container` has no
/// healthcheck primitive, so readiness is implemented here.
fn wait_ready(ctx: &Ctx, cname: &str, svc: &Service) {
    if svc.ready_cmd.is_empty() {
        return;
    }
    let mut waited = 0u64;
    if !ctx.json {
        print!("  waiting for {cname} ");
        std::io::stdout().flush().ok();
    }
    while waited < svc.ready_timeout {
        let mut argv: Vec<String> = vec!["exec".into(), cname.to_string()];
        argv.extend(svc.ready_cmd.iter().cloned());
        if ctx.container(&argv).silent().quiet_ok() {
            if !ctx.json {
                println!(" {}", style::green("ready"));
            }
            return;
        }
        if !ctx.json {
            print!(".");
            std::io::stdout().flush().ok();
        }
        thread::sleep(Duration::from_secs(2));
        waited += 2;
    }
    if !ctx.json {
        println!(" {}", style::yellow("timeout"));
    }
    ctx.warn(&format!(
        "{cname} did not become ready within {}s (continuing)",
        svc.ready_timeout
    ));
}

fn run_args(proj: &Project, svc: &Service, cname: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "--progress".into(),
        "none".into(),
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
    for p in &svc.ports {
        args.push("--publish".into());
        args.push(p.clone());
    }
    for v in &svc.volumes {
        args.push("--volume".into());
        args.push(format!("{}:{}", proj.volume_name(&v.name), v.target));
    }
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
        ctx.ok(&format!("{cname} already running"));
        return Ok(());
    }

    // A stopped container still has its filesystem: restart it in place rather
    // than recreating, unless --recreate was asked for.
    if state == "stopped" || state == "exited" {
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
        // `container run` sometimes reports an error while the container is in
        // fact created and running, so trust observed state over the exit code.
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
    // Resolve and validate before touching the daemon, so a typo does not leave
    // a daemon started for nothing.
    let targets = proj.target_services(services)?;

    daemon::ensure(ctx)?;

    // Only the images we are about to pull can justify a registry login.
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

/// Pre-pull every image in the manifest so a later start is fast.
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

// ------------------------------------------------------------------ login ---

/// Authenticate to the project's private registries.
///
/// Credentials are never stored in the manifest: `passwordCmd` is an argv that
/// is executed and piped to `--password-stdin`, which suits tokens that expire
/// (AWS ECR tokens last 12 hours, so this re-runs on every start).
///
/// `images` acts as a filter: a registry is only contacted when one of those
/// images actually comes from it. That keeps `ac <proj> start` from trying to
/// authenticate to ECR just to pull postgres from docker.io. Pass an empty
/// slice (an explicit `ac <proj> login`) to use every declared registry.
pub fn login(ctx: &Ctx, proj: &Project, vars: &Vars, images: &[String]) -> Result<()> {
    if proj.manifest.registries.is_empty() {
        return Ok(());
    }

    for reg in &proj.manifest.registries {
        let server = interpolate(&reg.server, vars);

        // Skip malformed servers: an uninterpolated {{account}} leaves a
        // leading dot, and a leftover brace means nothing was substituted.
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

// ------------------------------------------------------------------- stop ---

/// Stop containers WITHOUT removing them. The container keeps its filesystem
/// and can be restarted in place, which is both faster and non-destructive.
/// Use `down` when you actually want them gone.
pub fn stop(ctx: &Ctx, proj: &Project, services: &[String]) -> Result<()> {
    let targets = proj.target_services(services)?;
    let snap = Snapshot::query(ctx);

    for svc in &targets {
        let cname = proj.container_name(svc);
        match snap.state(&cname).as_str() {
            "absent" => ctx.dim(&format!("  {cname} not created")),
            "running" => {
                ctx.info(&format!("stopping {cname}"));
                ctx.container(["stop", &cname]).quiet_ok();
                ctx.ok(&format!("{cname} stopped"));
            }
            other => ctx.dim(&format!("  {cname} already {other}")),
        }
    }

    supervisor::settle(ctx)
}

/// Stop AND remove the containers. Named volumes are untouched, so data
/// survives.
pub fn down(ctx: &Ctx, proj: &Project, services: &[String]) -> Result<()> {
    let targets = proj.target_services(services)?;
    let snap = Snapshot::query(ctx);

    for svc in &targets {
        let cname = proj.container_name(svc);
        let state = snap.state(&cname);
        if state == "absent" {
            continue;
        }
        if state == "running" {
            ctx.info(&format!("stopping {cname}"));
            ctx.container(["stop", &cname]).quiet_ok();
        }
        ctx.container(["rm", &cname]).quiet_ok();
        ctx.ok(&format!("{cname} removed"));
    }

    supervisor::settle(ctx)
}

// ----------------------------------------------------------------- status ---

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
    // Without a running daemon nothing can be queried and every container would
    // be reported as "absent", which is a lie: they are merely unreachable.
    if !daemon::running_silent(ctx) {
        ctx.warn(&format!(
            "container daemon is not running - state unknown (run: ac {} start)",
            proj.name
        ));
    }
    ctx.log(&style::bold(&format!(
        "{:<22} {:<10} {:<18} {}",
        "CONTAINER", "STATE", "IP", "PORTS"
    )));
    for r in rows {
        ctx.log(&format!(
            "{:<22} {:<10} {:<18} {}",
            r.container,
            r.state,
            r.ip.clone().unwrap_or_else(|| "-".into()),
            if r.ports.is_empty() {
                "-".to_string()
            } else {
                r.ports.join(",")
            }
        ));
    }
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

// ------------------------------------------------------------------- logs ---

/// Copy one child stream to stdout, prefixed and coloured by service name.
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

/// Follow (or dump) every service at once, prefixing each line with the service
/// name. `container logs` only handles a single container, so the fan-out and
/// the interleaving are done here, the way `docker compose logs` behaves.
/// Ctrl-C tears down the whole group rather than just the foreground wait.
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
        // Best effort: the flag makes the reader threads stop printing, and the
        // children receive SIGINT from the terminal's process group anyway.
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

    // Wait for the readers, then reap every child so Ctrl-C leaves nothing
    // behind.
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
