#![recursion_limit = "512"]

mod build;
mod cli;
mod completions;
mod ctx;
mod daemon;
mod global;
mod manifest;
mod progress;
mod project;
mod schema;
mod state;
mod style;
mod supervisor;

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::{anyhow, Result};
use clap::{CommandFactory, Parser};

use crate::build::{vars_for, BuildOverrides};
use crate::cli::{
    Action, Cli, CompletionShell, DaemonAction, ImagesAction, TopCommand, VolumesAction, RESERVED,
};
use crate::ctx::Ctx;
use crate::manifest::Project;
use crate::state::Snapshot;

fn main() -> ExitCode {
    clap_complete::CompleteEnv::with_factory(completions::completion_command).complete();

    let argv: Vec<String> = std::env::args().collect();
    let rewritten = match rewrite_argv(&argv) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} {e}", style::red("err"));
            return ExitCode::FAILURE;
        }
    };

    let cli = Cli::parse_from(&rewritten);

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", style::red("err"));
            ExitCode::FAILURE
        }
    }
}

fn rewrite_argv(argv: &[String]) -> Result<Vec<String>> {
    let prog = argv.first().cloned().unwrap_or_else(|| "ac".into());
    let rest = &argv[1..];

    let is_global_flag = |s: &str| matches!(s, "--json" | "--quiet" | "--no-color");

    let rest = map_format_json(rest)?;
    let rest = &rest[..];

    let mut lead: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() && is_global_flag(&rest[i]) {
        lead.push(rest[i].clone());
        i += 1;
    }

    if rest[i..].is_empty() {
        return Ok(vec![prog]);
    }

    let mut out = vec![prog];
    out.extend(lead.clone());

    if i < rest.len() {
        let tok = rest[i].as_str();
        let (name, skip) = if tok == "-p" || tok == "--project" {
            match rest.get(i + 1) {
                Some(n) => (Some(n.clone()), 2),
                None => return Err(anyhow!("-p requires a project name")),
            }
        } else if let Some(n) = tok.strip_prefix("--project=") {
            (Some(n.to_string()), 1)
        } else {
            (None, 0)
        };
        if let Some(name) = name {
            out.push("project".into());
            out.push(name);
            let tail = &rest[i + skip..];
            if tail.is_empty() {
                out.push("status".into());
            } else {
                out.extend(tail.iter().cloned());
            }
            return Ok(out);
        }
    }

    let Some(first) = rest.get(i) else {
        out.extend(rest[i..].iter().cloned());
        return Ok(out);
    };

    if first.starts_with('-') || RESERVED.contains(&first.as_str()) {
        out.extend(rest[i..].iter().cloned());
        return Ok(out);
    }

    let probe = Ctx::new(false, true, true)?;
    let known = manifest::project_names(&probe.config_dir, &probe.ac_home);
    if !known.iter().any(|p| p == first) {
        let commands: Vec<&str> = RESERVED
            .iter()
            .copied()
            .filter(|c| !c.starts_with("__"))
            .collect();
        let hint = if cli::PROJECT_ACTIONS.contains(&first.as_str()) {
            format!("\n  '{first}' is a project action: try `ac <project> {first} ...`")
        } else {
            String::new()
        };
        return Err(anyhow!(
            "unknown project or command: {first}{hint}\n  projects: {}\n  commands: {}\n  try: ac --help",
            if known.is_empty() {
                "(none)".to_string()
            } else {
                known.join(" ")
            },
            commands.join(" ")
        ));
    }

    out.push("project".into());
    out.push(first.clone());
    let tail = &rest[i + 1..];
    if tail.is_empty() {
        out.push("status".into());
    } else {
        out.extend(tail.iter().cloned());
    }
    Ok(out)
}

fn map_format_json(rest: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::with_capacity(rest.len());
    let mut i = 0;
    let mut passthrough_zone = false;
    while i < rest.len() {
        let tok = rest[i].as_str();
        if matches!(tok, "exec" | "run" | "cp") {
            passthrough_zone = true;
        }
        if !passthrough_zone {
            if tok == "--format" {
                match rest.get(i + 1).map(|s| s.as_str()) {
                    Some("json") => {
                        out.push("--json".into());
                        i += 2;
                        continue;
                    }
                    other => {
                        return Err(anyhow!(
                            "--format {} is not supported; ac emits JSON only, use --json",
                            other.unwrap_or("")
                        ));
                    }
                }
            }
            if let Some(v) = tok.strip_prefix("--format=") {
                if v == "json" {
                    out.push("--json".into());
                    i += 1;
                    continue;
                }
                return Err(anyhow!(
                    "--format={v} is not supported; ac emits JSON only, use --json"
                ));
            }
        }
        out.push(rest[i].clone());
        i += 1;
    }
    Ok(out)
}

fn run(cli: Cli) -> Result<()> {
    let ctx = Ctx::new(cli.json, cli.quiet, cli.no_color)?;

    match &cli.command {
        TopCommand::Version => {
            if ctx.json {
                return ctx.emit_json(&serde_json::json!({ "version": ctx::AC_VERSION }));
            }
            println!("ac {}", ctx::AC_VERSION);
            Ok(())
        }
        TopCommand::Schema => ctx.emit_json(&schema::manifest_schema()),
        TopCommand::Guide { topic } => {
            let text = match topic {
                Some(cli::GuideTopic::Claude) => include_str!("../docs/claude-snippet.md"),
                None => include_str!("../docs/guide.md"),
            };
            print!("{text}");
            Ok(())
        }
        TopCommand::Completions { shell } => {
            let mut cmd = Cli::command();
            let sh = match shell {
                CompletionShell::Bash => clap_complete::Shell::Bash,
                CompletionShell::Zsh => clap_complete::Shell::Zsh,
                CompletionShell::Fish => clap_complete::Shell::Fish,
                CompletionShell::Elvish => clap_complete::Shell::Elvish,
                CompletionShell::PowerShell => clap_complete::Shell::PowerShell,
            };
            clap_complete::generate(sh, &mut cmd, "ac", &mut std::io::stdout());
            Ok(())
        }
        TopCommand::Ls => cmd_ls(&ctx),
        TopCommand::Status => cmd_global_status(&ctx),
        TopCommand::Config => {
            if ctx.json {
                ctx.emit_json(&ctx.config.to_json())
            } else {
                println!("{}", serde_json::to_string_pretty(&ctx.config.to_json())?);
                Ok(())
            }
        }
        TopCommand::Daemon { action } => match action.as_ref().unwrap_or(&DaemonAction::Status) {
            DaemonAction::Status => {
                let s = daemon::status(&ctx);
                if ctx.json {
                    ctx.emit_json(&s.to_json())
                } else {
                    println!("{}", s.line());
                    Ok(())
                }
            }
            DaemonAction::Stop => {
                supervisor::stop(&ctx);
                daemon::release(&ctx)
            }
        },
        TopCommand::Ps { all, ids } => global::ps(&ctx, *all, *ids),
        TopCommand::Image {
            verbose,
            ids,
            action,
        } => match action {
            Some(a) => global::image(&ctx, Some(a)),
            None => global::image(
                &ctx,
                Some(&cli::ImageAction::Ls {
                    verbose: *verbose,
                    ids: *ids,
                }),
            ),
        },
        TopCommand::Volume { action } => global::volume(&ctx, action.as_ref()),
        TopCommand::Network { action } => global::network(&ctx, action.as_ref()),
        TopCommand::System { action } => global::system(&ctx, action.as_ref()),
        TopCommand::Registry { action } => global::registry(&ctx, action.as_ref()),
        TopCommand::Rmi { references } => global::image(
            &ctx,
            Some(&cli::ImageAction::Rm {
                force: false,
                references: references.clone(),
            }),
        ),
        TopCommand::Df => global::system(&ctx, Some(&cli::SystemAction::Df)),
        TopCommand::Prune => global::system(&ctx, Some(&cli::SystemAction::Prune { all: false })),
        TopCommand::Supervise => supervisor::run_loop(&ctx),
        TopCommand::Project { name, action } => {
            let proj = manifest::load_project(&ctx.config_dir, &ctx.ac_home, name)?;
            run_action(&ctx, &proj, action)
        }
    }
}

fn cmd_ls(ctx: &Ctx) -> Result<()> {
    let names = manifest::project_names(&ctx.config_dir, &ctx.ac_home);
    if !ctx.json {
        for n in &names {
            println!("{n}");
        }
        return Ok(());
    }
    let items: Vec<serde_json::Value> = names
        .iter()
        .map(
            |n| match manifest::load_project(&ctx.config_dir, &ctx.ac_home, n) {
                Ok(p) => serde_json::json!({
                    "name": n,
                    "description": p.manifest.description,
                    "file": p.file.to_string_lossy(),
                    "services": p.manifest.service_names(),
                    "builds": p.manifest.build_names(),
                }),
                Err(e) => serde_json::json!({ "name": n, "error": e.to_string() }),
            },
        )
        .collect();
    ctx.emit_json(&serde_json::Value::Array(items))
}

fn cmd_global_status(ctx: &Ctx) -> Result<()> {
    let d = daemon::status(ctx);
    let sup_pid = supervisor::pid(ctx);
    let projects = manifest::load_all(&ctx.config_dir, &ctx.ac_home);

    if ctx.json {
        let snap = Snapshot::query(ctx);
        let items: Vec<serde_json::Value> = projects
            .iter()
            .map(|p| {
                let services: Vec<serde_json::Value> = p
                    .manifest
                    .services
                    .iter()
                    .map(|s| {
                        let cname = p.container_name(&s.name);
                        serde_json::json!({
                            "service": s.name,
                            "container": cname,
                            "state": snap.state(&cname),
                            "ip": snap.ip(&cname),
                            "ports": s.ports,
                            "image": s.image,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "name": p.name,
                    "description": p.manifest.description,
                    "services": services,
                })
            })
            .collect();
        return ctx.emit_json(&serde_json::json!({
            "daemon": d.to_json(),
            "supervisor": { "running": sup_pid.is_some(), "pid": sup_pid },
            "projects": items,
        }));
    }

    println!("{}  {}", style::bold("daemon"), d.line());
    match sup_pid {
        Some(p) => println!("{}  running (pid {p})", style::bold("supervisor")),
        None => println!("{}  not running", style::bold("supervisor")),
    }
    println!();
    for p in &projects {
        println!("{} - {}", style::bold(&p.name), p.manifest.description);
        let rows = project::status_rows(ctx, p);
        project::print_status(ctx, p, &rows);
        println!();
    }
    Ok(())
}

fn run_action(ctx: &Ctx, proj: &Project, action: &Action) -> Result<()> {
    match action {
        Action::Start {
            recreate,
            detach: _,
            services,
        } => project::start(ctx, proj, services, *recreate),

        Action::Run {
            keep,
            rm_noop: _,
            interactive: _,
            tty: _,
            env,
            no_volumes,
            service,
            command,
        } => {
            let status = project::run_once(ctx, proj, service, command, *keep, env, *no_volumes)?;
            exit_like(status)
        }

        Action::Create { recreate, services } => project::create(ctx, proj, services, *recreate),

        Action::Top { services } => project::top(ctx, proj, services),

        Action::Wait { timeout, services } => project::wait(ctx, proj, services, *timeout),

        Action::Push { profile, names } => {
            build::project_push(ctx, proj, names, profile.as_deref())
        }

        Action::Export { service, output } => {
            project::export(ctx, proj, service, output.as_deref())
        }

        Action::Stop { time, services } => project::stop(ctx, proj, services, *time),

        Action::Down {
            volumes,
            time,
            services,
        } => project::down(ctx, proj, services, *time, *volumes),

        Action::Restart { recreate, services } => {
            let targets = proj.target_services(services)?;
            let snap = Snapshot::query(ctx);
            for svc in &targets {
                let cname = proj.container_name(svc);
                match snap.state(&cname).as_str() {
                    "absent" => ctx.dim(&format!("  {cname} not created")),
                    "running" => {
                        ctx.info(&format!("stopping {cname}"));
                        if project::stop_container(ctx, &cname, None) {
                            ctx.ok(&format!("{cname} stopped"));
                        } else {
                            ctx.warn(&format!("{cname} would not stop"));
                        }
                    }
                    other => ctx.dim(&format!("  {cname} already {other}")),
                }
            }
            project::start(ctx, proj, services, *recreate)
        }

        Action::Services => {
            let names = proj.manifest.service_names();
            if ctx.json {
                ctx.emit_json(&serde_json::json!(names))
            } else {
                for n in &names {
                    println!("{n}");
                }
                Ok(())
            }
        }

        Action::Builds => {
            let names = proj.manifest.build_names();
            if ctx.json {
                ctx.emit_json(&serde_json::json!(names))
            } else {
                for n in &names {
                    println!("{n}");
                }
                Ok(())
            }
        }

        Action::Profiles => {
            let names = proj.manifest.profile_names();
            if ctx.json {
                ctx.emit_json(&serde_json::json!(names))
            } else {
                for n in &names {
                    println!("{n}");
                }
                Ok(())
            }
        }

        Action::Ls { all: _ } => {
            let rows = project::status_rows(ctx, proj);
            if ctx.json {
                ctx.emit_json(&project::status_json(&rows))
            } else {
                project::print_status(ctx, proj, &rows);
                Ok(())
            }
        }

        Action::Logs {
            follow,
            tail,
            boot,
            service,
        } => {
            let mut flags: Vec<String> = Vec::new();
            if *follow {
                flags.push("-f".into());
            }
            if let Some(n) = tail {
                flags.push("-n".into());
                flags.push(n.to_string());
            }
            if *boot {
                flags.push("--boot".into());
            }
            match service {
                Some(s) => {
                    if !proj.has_service(s) {
                        return Err(anyhow!(
                            "no such service '{s}' in project '{}' (have: {})",
                            proj.name,
                            proj.manifest.service_names().join(" ")
                        ));
                    }
                    let cname = proj.container_name(&proj.normalize_service(s));
                    let mut argv = vec!["logs".to_string()];
                    argv.extend(flags);
                    argv.push(cname);
                    ctx.container(&argv).status()?;
                    Ok(())
                }
                None => project::logs_all(ctx, proj, &flags),
            }
        }

        Action::Exec {
            interactive: _,
            tty: _,
            service,
            command,
        } => {
            let svc = proj.target_services(std::slice::from_ref(service))?;
            let mut argv = vec!["exec".to_string()];
            argv.extend(tty_flags());
            argv.push(proj.container_name(&svc[0]));
            argv.extend(command.iter().cloned());
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Sh {
            interactive: _,
            tty: _,
            service,
        } => {
            let name = match service {
                Some(s) => proj.target_services(std::slice::from_ref(s))?[0].clone(),
                None => proj
                    .manifest
                    .services
                    .first()
                    .map(|s| s.name.clone())
                    .ok_or_else(|| anyhow!("project '{}' declares no services", proj.name))?,
            };
            let mut argv = vec!["exec".to_string()];
            argv.extend(tty_flags());
            argv.push(proj.container_name(&name));
            argv.push("sh".into());
            argv.push("-c".into());
            argv.push("command -v bash >/dev/null && exec bash || exec sh".into());
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Stats {
            no_stream,
            services,
        } => {
            let names = proj.target_container_names(services)?;
            if ctx.json {
                let mut argv = vec![
                    "stats".to_string(),
                    "--no-stream".into(),
                    "--format".into(),
                    "json".into(),
                ];
                argv.extend(names);
                let text = ctx.container(&argv).stdout_timeout(20)?;
                let v: serde_json::Value = serde_json::from_str(&text)?;
                return ctx.emit_json(&v);
            }
            let mut argv = vec!["stats".to_string()];
            if *no_stream {
                argv.push("--no-stream".into());
            }
            argv.extend(names);
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Inspect { services } => {
            let mut argv = vec!["inspect".to_string()];
            argv.extend(proj.target_container_names(services)?);
            if ctx.json {
                let text = ctx.container(&argv).stdout()?;
                let v: serde_json::Value = serde_json::from_str(&text)?;
                return ctx.emit_json(&v);
            }
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Kill { signal, services } => {
            let mut argv = vec!["kill".to_string(), "--signal".into(), signal.clone()];
            argv.extend(proj.target_container_names(services)?);
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Rm { services } => project::remove(ctx, proj, services),

        Action::Cp { src, dst } => {
            let src_r = cp_path(proj, src)?;
            let dst_r = cp_path(proj, dst)?;

            let container_side = |rewritten: &str| -> Option<(String, String)> {
                let (head, tail) = rewritten.split_once(':')?;
                if tail.starts_with('/') && head.starts_with(&format!("{}-", proj.name)) {
                    Some((head.to_string(), tail.to_string()))
                } else {
                    None
                }
            };

            if let Some((cname, path)) = container_side(&src_r) {
                match ctx
                    .container(["exec", &cname, "sh", "-c", "test -e \"$1\"", "_", &path])
                    .quiet_ok_timeout(10)
                {
                    Some(true) => {}
                    Some(false) => {
                        return Err(anyhow!("'{path}' does not exist in {cname}"));
                    }
                    None => {
                        return Err(anyhow!(
                            "{cname} is not answering exec probes; container cp would hang \
(known Apple container issue), aborting"
                        ));
                    }
                }
            }

            let status = ctx.container(["cp", &src_r, &dst_r]).status()?;
            if status.success() {
                if let Some((cname, path)) = container_side(&dst_r) {
                    let base = std::path::Path::new(&src_r)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let landed = ctx
                        .container([
                            "exec",
                            &cname,
                            "sh",
                            "-c",
                            "test -e \"$1\" || test -e \"$1/$2\"",
                            "_",
                            &path,
                            &base,
                        ])
                        .quiet_ok_timeout(10);
                    if landed == Some(false) {
                        return Err(anyhow!(
                            "container cp reported success but nothing appeared at {cname}:{path} \
(known Apple container issue; use exec with redirection instead)"
                        ));
                    }
                }
            }
            exit_like(status)
        }

        Action::Pull { services } => project::pull(ctx, proj, services),

        Action::Images { action } => {
            let list = || -> Vec<(String, String)> {
                let render = |image: &str| -> String {
                    if !image.contains("{{") {
                        return image.to_string();
                    }
                    let profile = std::env::var("AC_PROFILE")
                        .ok()
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            proj.manifest
                                .profiles
                                .get("local")
                                .map(|_| "local".to_string())
                        })
                        .or_else(|| proj.manifest.profile_names().first().cloned());
                    match profile {
                        Some(p) => {
                            let root = std::env::current_dir().unwrap_or_default();
                            build::interpolate(image, &vars_for(proj, &p, &root))
                        }
                        None => image.to_string(),
                    }
                };
                let mut v: Vec<(String, String)> = proj
                    .manifest
                    .services
                    .iter()
                    .map(|s| (s.name.clone(), s.image.clone()))
                    .collect();
                v.extend(
                    proj.manifest
                        .builds
                        .iter()
                        .map(|b| (b.name.clone(), render(&b.image))),
                );
                v
            };

            match action.as_ref().unwrap_or(&ImagesAction::Ls) {
                ImagesAction::Ls => {
                    let rows = list();
                    if ctx.json {
                        let items: Vec<serde_json::Value> = rows
                            .iter()
                            .map(|(n, i)| serde_json::json!({ "name": n, "image": i }))
                            .collect();
                        return ctx.emit_json(&serde_json::Value::Array(items));
                    }
                    println!("{}", style::bold(&format!("{:<14} {}", "NAME", "IMAGE")));
                    for (n, i) in rows {
                        println!("{n:<14} {i}");
                    }
                    Ok(())
                }

                ImagesAction::Rm { names } => {
                    daemon::ensure(ctx)?;
                    let rows = list();
                    let targets: Vec<(String, String)> = if names.is_empty() {
                        rows
                    } else {
                        let mut out = Vec::new();
                        for want in names {
                            let hit = rows.iter().find(|(n, _)| {
                                n == want || format!("{}-{}", proj.name, n) == *want
                            });
                            match hit {
                                Some(r) => out.push(r.clone()),
                                None => {
                                    let known: Vec<&str> =
                                        rows.iter().map(|(n, _)| n.as_str()).collect();
                                    return Err(anyhow!(
                                        "no service or build named '{want}' in project '{}'\n  have: {}",
                                        proj.name,
                                        known.join(" ")
                                    ));
                                }
                            }
                        }
                        out
                    };

                    let mut failed = 0;
                    for (name, image) in &targets {
                        ctx.info(&format!("removing image for {name}"));
                        let ok = ctx
                            .container(["image", "rm", image])
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false);
                        if !ok {
                            ctx.warn(&format!("could not remove {image}"));
                            failed += 1;
                        }
                    }
                    if failed > 0 {
                        return Err(anyhow!("{failed} image(s) could not be removed"));
                    }
                    supervisor::settle(ctx)
                }

                ImagesAction::Prune => {
                    daemon::ensure(ctx)?;
                    ctx.info("removing unused images");
                    ctx.container(["image", "prune"]).status()?;
                    supervisor::settle(ctx)
                }
            }
        }

        Action::Volumes { action } => {
            let declared: Vec<(String, String)> = proj
                .manifest
                .services
                .iter()
                .flat_map(|s| s.volumes.iter())
                .map(|v| (v.name.clone(), proj.volume_name(&v.name)))
                .collect();

            let resolve = |names: &Vec<String>| -> Result<Vec<(String, String)>> {
                if names.is_empty() {
                    return Ok(declared.clone());
                }
                let mut out = Vec::new();
                for want in names {
                    match declared
                        .iter()
                        .find(|(short, full)| short == want || full == want)
                    {
                        Some(v) => out.push(v.clone()),
                        None => {
                            let known: Vec<&str> =
                                declared.iter().map(|(s, _)| s.as_str()).collect();
                            return Err(anyhow!(
                                "no volume named '{want}' in project '{}'\n  have: {}",
                                proj.name,
                                known.join(" ")
                            ));
                        }
                    }
                }
                Ok(out)
            };

            match action.as_ref().unwrap_or(&VolumesAction::Ls) {
                VolumesAction::Ls => {
                    let present = daemon::running(ctx);
                    let existing: Vec<String> = if present {
                        project::existing_volumes(ctx)
                    } else {
                        Vec::new()
                    };

                    let state = |full: &str| -> &'static str {
                        if !present {
                            "unknown"
                        } else if existing.iter().any(|e| e == full) {
                            "present"
                        } else {
                            "absent"
                        }
                    };

                    if ctx.json {
                        let items: Vec<serde_json::Value> = declared
                            .iter()
                            .map(|(short, full)| {
                                serde_json::json!({
                                    "name": short,
                                    "volume": full,
                                    "state": state(full),
                                })
                            })
                            .collect();
                        return ctx.emit_json(&serde_json::Value::Array(items));
                    }
                    if !present {
                        ctx.warn("container daemon is not running, existence is unknown");
                    }
                    println!(
                        "{}",
                        style::bold(&format!("{:<16} {:<26} {}", "NAME", "VOLUME", "STATE"))
                    );
                    for (short, full) in &declared {
                        println!("{short:<16} {full:<26} {}", state(full));
                    }
                    Ok(())
                }

                VolumesAction::Rm { names } => {
                    daemon::ensure(ctx)?;
                    let targets = resolve(names)?;
                    let mut failed = 0;
                    for (short, full) in &targets {
                        ctx.info(&format!("deleting volume {short} ({full})"));
                        let ok = ctx
                            .container(["volume", "delete", full])
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false);
                        if !ok {
                            ctx.warn(&format!("could not delete {full} (still attached to a container? remove it first)"));
                            failed += 1;
                        }
                    }
                    if failed > 0 {
                        return Err(anyhow!("{failed} volume(s) could not be deleted"));
                    }
                    supervisor::settle(ctx)
                }

                VolumesAction::Inspect { names } => {
                    daemon::ensure(ctx)?;
                    let targets = resolve(names)?;
                    let mut args: Vec<String> = vec!["volume".into(), "inspect".into()];
                    args.extend(targets.iter().map(|(_, full)| full.clone()));
                    ctx.container(args).status()?;
                    Ok(())
                }

                VolumesAction::Prune => {
                    daemon::ensure(ctx)?;
                    ctx.info("removing volumes with no container references");
                    ctx.container(["volume", "prune"]).status()?;
                    supervisor::settle(ctx)
                }
            }
        }

        Action::Port { services } => {
            let targets = proj.target_services(services)?;
            if ctx.json {
                let items: Vec<serde_json::Value> = targets
                    .iter()
                    .filter_map(|n| proj.manifest.service(n))
                    .map(|s| {
                        let mappings: Vec<serde_json::Value> = s
                            .ports
                            .iter()
                            .map(|p| {
                                let (h, c) = p.split_once(':').unwrap_or((p.as_str(), p.as_str()));
                                serde_json::json!({ "host": h, "container": c, "raw": p })
                            })
                            .collect();
                        serde_json::json!({ "service": s.name, "ports": mappings })
                    })
                    .collect();
                return ctx.emit_json(&serde_json::Value::Array(items));
            }
            for n in &targets {
                let ports = proj
                    .manifest
                    .service(n)
                    .map(|s| s.ports.join(", "))
                    .unwrap_or_default();
                println!("{n:<14} {ports}");
            }
            Ok(())
        }

        Action::Ip { services } => {
            let targets = proj.target_services(services)?;
            let snap = Snapshot::query(ctx);
            if ctx.json {
                let items: Vec<serde_json::Value> = targets
                    .iter()
                    .map(|n| {
                        let cname = proj.container_name(n);
                        serde_json::json!({
                            "service": n,
                            "container": cname,
                            "ip": snap.ip(&cname),
                            "state": snap.state(&cname),
                        })
                    })
                    .collect();
                return ctx.emit_json(&serde_json::Value::Array(items));
            }
            if targets.len() == 1 && !services.is_empty() {
                println!(
                    "{}",
                    snap.ip(&proj.container_name(&targets[0]))
                        .unwrap_or_default()
                );
                return Ok(());
            }
            for n in &targets {
                println!(
                    "{n:<14} {}",
                    snap.ip(&proj.container_name(n)).unwrap_or_default()
                );
            }
            Ok(())
        }

        Action::Env { service } => {
            let svc = proj.target_services(std::slice::from_ref(service))?;
            let s = proj
                .manifest
                .service(&svc[0])
                .ok_or_else(|| anyhow!("no such service '{service}'"))?;
            if ctx.json {
                return ctx.emit_json(&serde_json::Value::Object(s.env.clone()));
            }
            for (k, v) in &s.env {
                println!("{k}={}", manifest::json_scalar(v));
            }
            Ok(())
        }

        Action::Config => {
            if ctx.json {
                let v: serde_json::Value = serde_json::from_str(&proj.raw)?;
                return ctx.emit_json(&v);
            }
            print!("{}", proj.raw);
            if !proj.raw.ends_with('\n') {
                println!();
            }
            Ok(())
        }

        Action::Build(args) => {
            let ov = BuildOverrides {
                profile: args.profile.clone(),
                root: args.root.clone(),
                platform: args.platform.clone(),
                push: args.push_override(),
                no_cache: args.no_cache,
                progress: args.progress.clone(),
                target: args.target.clone(),
                builder_cpus: args.builder_cpus,
                builder_memory: args.builder_memory.clone(),
                sequential: args.sequential,
                dry_run: args.dry_run,
                rollout: args.rollout_override(),
            };
            build::project_build(ctx, proj, &args.names, &ov)
        }

        Action::Rollout(args) => {
            let ov = BuildOverrides {
                profile: args.profile.clone(),
                root: args.root.clone(),
                dry_run: args.dry_run,
                rollout: Some(true),
                ..Default::default()
            };
            build::project_rollout(ctx, proj, &args.names, &ov)
        }

        Action::Login { profile } => {
            let name = profile
                .clone()
                .or_else(|| std::env::var("AC_PROFILE").ok().filter(|s| !s.is_empty()))
                .unwrap_or_else(|| "local".to_string());
            daemon::ensure(ctx)?;
            let ov = BuildOverrides {
                profile: Some(name.clone()),
                ..Default::default()
            };
            let root = build::resolve_root(ctx, proj, &ov)?;
            let vars = vars_for(proj, &name, &root);
            project::login(ctx, proj, &vars, &[])
        }
    }
}

fn tty_flags() -> Vec<String> {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        vec!["-i".into(), "-t".into()]
    } else {
        vec!["-i".into()]
    }
}

fn cp_path(proj: &Project, arg: &str) -> Result<String> {
    if arg.starts_with('/') {
        return Ok(arg.to_string());
    }
    let Some((head, tail)) = arg.split_once(':') else {
        return Ok(arg.to_string());
    };
    if head.contains('/') || !tail.starts_with('/') {
        return Ok(arg.to_string());
    }
    if proj.has_service(head) {
        return Ok(format!(
            "{}:{tail}",
            proj.container_name(&proj.normalize_service(head))
        ));
    }
    Err(anyhow!(
        "no such service '{head}' in project '{}' (have: {})",
        proj.name,
        proj.manifest.service_names().join(" ")
    ))
}

fn exit_like(status: std::process::ExitStatus) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn global_flags_before_a_project_are_kept_once() {
        let argv: Vec<String> = ["ac", "--json", "shop", "ls"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = rewrite_argv(&argv).expect("rewrite");
        assert_eq!(out, vec!["ac", "--json", "project", "shop", "ls"]);
    }

    use super::*;

    fn proj_fixture() -> Project {
        let raw = r#"{
            "name": "demo",
            "services": [
              { "name": "redis", "image": "docker.io/library/redis:7-alpine" },
              { "name": "web", "image": "docker.io/library/nginx:alpine" }
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
    fn cp_rewrites_service_refs_and_rejects_unknown_ones() {
        let p = proj_fixture();
        assert_eq!(cp_path(&p, "redis:/data").unwrap(), "demo-redis:/data");
        assert_eq!(cp_path(&p, "demo-redis:/data").unwrap(), "demo-redis:/data");
        assert_eq!(cp_path(&p, "/etc/hosts").unwrap(), "/etc/hosts");
        assert_eq!(cp_path(&p, "./a/b:c").unwrap(), "./a/b:c");
        assert_eq!(cp_path(&p, "plain.txt").unwrap(), "plain.txt");
        assert_eq!(cp_path(&p, "local:file").unwrap(), "local:file");
        let err = cp_path(&p, "unknown:/x").unwrap_err();
        assert!(err.to_string().contains("redis web"), "{err}");
    }

    #[test]
    fn format_json_maps_to_json_outside_passthrough_zones() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            map_format_json(&a(&["ps", "--format", "json"])).unwrap(),
            a(&["ps", "--json"])
        );
        assert_eq!(
            map_format_json(&a(&["image", "ls", "--format=json"])).unwrap(),
            a(&["image", "ls", "--json"])
        );
        assert_eq!(
            map_format_json(&a(&["demo", "exec", "web", "cmd", "--format", "json"])).unwrap(),
            a(&["demo", "exec", "web", "cmd", "--format", "json"])
        );
        let err = map_format_json(&a(&["ps", "--format", "table"])).unwrap_err();
        assert!(err.to_string().contains("--json"), "{err}");
    }

    #[test]
    fn bare_invocations_collapse_to_help() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(rewrite_argv(&a(&["ac", "--json"])).unwrap(), a(&["ac"]));
        assert_eq!(
            rewrite_argv(&a(&["ac", "--json", "--quiet"])).unwrap(),
            a(&["ac"])
        );
    }

    #[test]
    fn services_resolve_in_either_form() {
        let p = proj_fixture();
        assert!(p.has_service("redis"));
        assert!(p.has_service("demo-redis"));
        assert!(!p.has_service("nope"));
        assert_eq!(p.target_services(&[]).unwrap(), vec!["redis", "web"]);
        assert_eq!(
            p.target_services(&["demo-web".to_string()]).unwrap(),
            vec!["web"]
        );
        let err = p.target_services(&["nope".to_string()]).unwrap_err();
        assert!(err.to_string().contains("redis web"), "{err}");
    }

    #[test]
    fn argv_rewrite_leaves_reserved_words_alone() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            rewrite_argv(&a(&["ac", "status"])).unwrap(),
            a(&["ac", "status"])
        );
        assert_eq!(
            rewrite_argv(&a(&["ac", "--json", "daemon", "status"])).unwrap(),
            a(&["ac", "--json", "daemon", "status"])
        );
        assert_eq!(
            rewrite_argv(&a(&["ac", "-p", "status", "start"])).unwrap(),
            a(&["ac", "project", "status", "start"])
        );
        assert_eq!(
            rewrite_argv(&a(&["ac", "-p", "weird"])).unwrap(),
            a(&["ac", "project", "weird", "status"])
        );
        assert_eq!(
            rewrite_argv(&a(&["ac", "--help"])).unwrap(),
            a(&["ac", "--help"])
        );
    }

    #[test]
    fn build_interpolation_covers_every_placeholder() {
        let v = build::Vars {
            profile: "dev".into(),
            account: "123".into(),
            tag: "t".into(),
            region: "us-east-1".into(),
            registry: "r/".into(),
            version: "1.2.3".into(),
            git_sha: "abc".into(),
            git_short_sha: "ab".into(),
            git_branch: "main".into(),
            git_dirty_suffix: "-local-1".into(),
            timestamp: "20260101000000".into(),
            images: [("web".to_string(), vec!["r/web:t".to_string()])]
                .into_iter()
                .collect(),
        };
        let s = "{{profile}}|{{account}}|{{tag}}|{{region}}|{{registry}}|{{version}}|\
                 {{git.sha}}|{{git.shortSha}}|{{git.branch}}|{{git.dirtySuffix}}|{{timestamp}}|\
                 {{image.web}}";
        assert_eq!(
            build::interpolate(s, &v),
            "dev|123|t|us-east-1|r/|1.2.3|abc|ab|main|-local-1|20260101000000|r/web:t"
        );
        assert!(!build::interpolate(s, &v).contains("{{"));
    }

    #[test]
    fn rollout_hook_env_carries_image_refs() {
        let dir = std::env::temp_dir();
        let text = r#"{
            "name": "demo",
            "profiles": { "prod": { "push": true, "tag": "latest", "registry": "reg/",
                          "rollout": { "run": [["./deploy.sh"]] } } },
            "builds": [
              { "name": "web", "dockerfile": "Dockerfile", "image": "{{registry}}web",
                "tags": ["{{tag}}"] },
              { "name": "api-workers", "dockerfile": "Dockerfile", "image": "{{registry}}wrk",
                "tags": ["{{tag}}", "pinned"] }
            ]
        }"#;
        let m: manifest::Manifest = serde_json::from_str(text).expect("manifest parses");
        let proj = manifest::Project {
            name: "demo".into(),
            file: dir.join("demo.json"),
            manifest: m,
            raw: text.to_string(),
        };
        let v = build::vars_for(&proj, "prod", &dir);
        let env = build::hook_env(&proj, &v, &dir, &["web".to_string()]);
        let get = |k: &str| {
            env.iter()
                .find(|(n, _)| n == k)
                .map(|(_, x)| x.clone())
                .unwrap_or_default()
        };

        assert_eq!(get("AC_IMAGE_WEB"), "reg/web:latest");
        assert_eq!(get("AC_IMAGE_API_WORKERS"), "reg/wrk:latest");
        assert_eq!(
            get("AC_IMAGES_API_WORKERS"),
            "reg/wrk:latest reg/wrk:pinned"
        );
        assert_eq!(get("AC_IMAGES"), "reg/web:latest");
        assert_eq!(get("AC_BUILDS"), "web");
        assert_eq!(get("AC_PROFILE"), "prod");
    }

    #[test]
    fn rollout_is_rejected_for_a_profile_that_declares_none() {
        let text = r#"{
            "name": "demo",
            "profiles": { "local": { "push": false } },
            "builds": [{ "name": "web", "dockerfile": "Dockerfile", "image": "web",
                         "tags": ["dev"] }]
        }"#;
        let m: manifest::Manifest = serde_json::from_str(text).expect("manifest parses");
        assert!(m.profiles.get("local").expect("profile").rollout.is_none());
    }

    #[test]
    fn a_profile_rollout_block_parses_and_rejects_typos() {
        let ok = r#"{ "push": true, "rollout": {
            "description": "ship it", "auto": true,
            "preflight": [["./pre.sh"]], "run": [["./go.sh", "{{profile}}"]] } }"#;
        let p: manifest::Profile = serde_json::from_str(ok).expect("rollout parses");
        let r = p.rollout.expect("rollout present");
        assert!(r.auto);
        assert_eq!(r.run[0], vec!["./go.sh", "{{profile}}"]);

        let typo = r#"{ "push": true, "rollout": { "runn": [["./go.sh"]] } }"#;
        let err = serde_json::from_str::<manifest::Profile>(typo)
            .expect_err("unknown field must be rejected");
        assert!(err.to_string().contains("runn"), "got: {err}");
    }

    #[test]
    fn memory_parsing() {
        assert_eq!(build::memory_to_mb("8g"), Some(8192));
        assert_eq!(build::memory_to_mb("8G"), Some(8192));
        assert_eq!(build::memory_to_mb("8gb"), Some(8192));
        assert_eq!(build::memory_to_mb("512m"), Some(512));
        assert_eq!(build::memory_to_mb("512"), Some(512));
        assert_eq!(build::memory_to_mb("nope"), None);
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
