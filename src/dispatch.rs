use std::collections::HashMap;
use std::io::IsTerminal;

use anyhow::{anyhow, Result};
use clap::CommandFactory;

use crate::build::{vars_for, BuildOverrides};
use crate::cli::{
    Action, Cli, CompletionShell, DaemonAction, ImagesAction, TopCommand, VolumesAction,
};
use crate::commands::{docker, groups, project, script};
use crate::core::ctx::Ctx;
use crate::core::state::Snapshot;
use crate::core::{ctx, style, util};
use crate::daemon;
use crate::daemon::supervisor;
use crate::manifest::schema;
use crate::manifest::Project;
use crate::{build, cli, manifest};

pub fn run(cli: Cli) -> Result<()> {
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
        TopCommand::Ps { all, ids } => groups::ps(&ctx, *all, *ids),
        TopCommand::Image {
            verbose,
            ids,
            action,
        } => match action {
            Some(a) => groups::image(&ctx, Some(a)),
            None => groups::image(
                &ctx,
                Some(&cli::ImageAction::Ls {
                    verbose: *verbose,
                    ids: *ids,
                }),
            ),
        },
        TopCommand::Volume { action } => groups::volume(&ctx, action.as_ref()),
        TopCommand::Network { action } => groups::network(&ctx, action.as_ref()),
        TopCommand::System { action } => groups::system(&ctx, action.as_ref()),
        TopCommand::Registry { action } => groups::registry(&ctx, action.as_ref()),
        TopCommand::Rmi { references } => groups::image(
            &ctx,
            Some(&cli::ImageAction::Rm {
                force: false,
                references: references.clone(),
            }),
        ),
        TopCommand::Df => groups::system(&ctx, Some(&cli::SystemAction::Df)),
        TopCommand::Prune => groups::system(&ctx, Some(&cli::SystemAction::Prune { all: false })),

        TopCommand::Run {
            opts,
            rm,
            image,
            command,
        } => exit_like(docker::run(&ctx, opts, *rm, image, command)?),
        TopCommand::Create {
            opts,
            rm,
            image,
            command,
        } => exit_like(docker::create(&ctx, opts, *rm, image, command)?),
        TopCommand::Build {
            tags,
            file,
            target,
            platform,
            arch,
            os,
            build_args,
            labels,
            secrets,
            no_cache,
            pull,
            progress,
            output,
            cpus,
            memory,
            build_quiet,
            context,
        } => docker::build(
            &ctx,
            &docker::BuildRequest {
                tags,
                file: file.as_deref(),
                target: target.as_deref(),
                platform: platform.as_deref(),
                arch: arch.as_deref(),
                os: os.as_deref(),
                build_args,
                labels,
                secrets,
                no_cache: *no_cache,
                pull: *pull,
                progress: progress.as_deref(),
                output: output.as_deref(),
                cpus: *cpus,
                memory: memory.as_deref(),
                build_quiet: *build_quiet,
                context,
            },
        ),
        TopCommand::Start {
            attach,
            interactive,
            containers,
        } => docker::start(&ctx, containers, *attach, *interactive),
        TopCommand::Stop {
            time,
            signal,
            all,
            containers,
        } => docker::stop(&ctx, containers, *time, signal.as_deref(), *all),
        TopCommand::Restart { time, containers } => docker::restart(&ctx, containers, *time),
        TopCommand::Rm {
            force,
            all,
            containers,
        } => docker::rm(&ctx, containers, *force, *all),
        TopCommand::Exec {
            tty,
            detach,
            env,
            workdir,
            user,
            container,
            command,
            ..
        } => exit_like(docker::exec(
            &ctx,
            container,
            command,
            *tty,
            *detach,
            env,
            workdir.as_deref(),
            user.as_deref(),
        )?),
        TopCommand::Sh { container } => exit_like(docker::sh(&ctx, container)?),
        TopCommand::Logs {
            follow,
            tail,
            boot,
            container,
        } => exit_like(docker::logs(&ctx, container, *follow, *tail, *boot)?),
        TopCommand::Inspect { containers } => docker::inspect(&ctx, containers),
        TopCommand::Kill {
            signal,
            all,
            containers,
        } => docker::kill(&ctx, containers, signal, *all),
        TopCommand::Cp { src, dst } => docker::cp(&ctx, src, dst),
        TopCommand::Export { container, output } => {
            docker::export(&ctx, container, output.as_deref())
        }
        TopCommand::Stats {
            no_stream,
            containers,
        } => docker::stats(&ctx, containers, *no_stream),
        TopCommand::Top { containers } => docker::top(&ctx, containers),
        TopCommand::Port { container } => docker::port(&ctx, container),
        TopCommand::Pull {
            reference,
            platform,
        } => docker::pull(&ctx, reference, platform.as_deref()),
        TopCommand::Push {
            reference,
            platform,
        } => docker::push(&ctx, reference, platform.as_deref()),
        TopCommand::Tag { source, target } => docker::tag(&ctx, source, target),
        TopCommand::Save { reference, output } => docker::save(&ctx, reference, output),
        TopCommand::Load { input } => docker::load(&ctx, input),
        TopCommand::Login {
            server,
            username,
            password,
            password_stdin,
        } => docker::login(
            &ctx,
            server,
            username.as_deref(),
            password.as_deref(),
            *password_stdin,
        ),
        TopCommand::Logout { server } => docker::logout(&ctx, server),
        TopCommand::Builder { action } => docker::builder(&ctx, action.as_ref()),
        TopCommand::Machine { args } => docker::machine(&ctx, args),

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

        Action::Scripts => script::list(ctx, proj),

        Action::Script(argv) => {
            let status = script::run(ctx, proj, argv)?;
            exit_like(status)
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
                    let local = local_image_sizes(ctx);
                    let sized: Vec<(String, String, Option<u64>)> = rows
                        .iter()
                        .map(|(n, i)| {
                            let size = local.as_ref().and_then(|m| lookup_image(m, i));
                            (n.clone(), i.clone(), size)
                        })
                        .collect();

                    if ctx.json {
                        let items: Vec<serde_json::Value> = sized
                            .iter()
                            .map(|(n, i, size)| {
                                serde_json::json!({
                                    "name": n,
                                    "image": i,
                                    "present": local.is_some().then_some(size.is_some()),
                                    "size": size,
                                })
                            })
                            .collect();
                        return ctx.emit_json(&serde_json::Value::Array(items));
                    }

                    let mut table =
                        util::Table::new(&["NAME", "IMAGE", "SIZE", "LOCAL"]).right(&[2]);
                    for (n, i, size) in &sized {
                        let (size_col, local_col) = match (local.is_some(), size) {
                            (false, _) => ("-".to_string(), "?".to_string()),
                            (true, Some(b)) => (util::fmt_size(*b), "yes".to_string()),
                            (true, None) => ("-".to_string(), "no".to_string()),
                        };
                        table.row([n.clone(), i.clone(), size_col, local_col]);
                    }
                    for line in table.lines() {
                        println!("{line}");
                    }
                    if local.is_none() {
                        ctx.dim("daemon not running, so local presence is unknown");
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
                    let mut table = util::Table::new(&["NAME", "VOLUME", "STATE"]);
                    for (short, full) in &declared {
                        table.row([short.as_str(), full.as_str(), state(full)]);
                    }
                    for line in table.lines() {
                        println!("{line}");
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

fn local_image_sizes(ctx: &Ctx) -> Option<HashMap<String, u64>> {
    if !daemon::running_silent(ctx) {
        return None;
    }
    let text = ctx
        .container(["image", "ls", "--format", "json"])
        .stdout()
        .ok()?;
    let raw: Vec<serde_json::Value> = serde_json::from_str(&text).ok()?;
    let mut map = HashMap::new();
    for e in &raw {
        let Some(full) = e
            .get("configuration")
            .and_then(|c| c.get("name"))
            .and_then(|n| n.as_str())
        else {
            continue;
        };
        let variants = e.get("variants").and_then(|v| v.as_array());
        let size = variants
            .and_then(|vs| {
                vs.iter()
                    .find(|v| {
                        v.get("platform")
                            .and_then(|p| p.get("architecture"))
                            .and_then(|a| a.as_str())
                            == Some(util::host_arch())
                    })
                    .or_else(|| vs.first())
            })
            .and_then(|v| v.get("size"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let (repo, tag) = util::short_ref(full);
        map.insert(full.to_string(), size);
        map.insert(format!("{repo}:{tag}"), size);
        map.entry(repo).or_insert(size);
    }
    Some(map)
}

fn lookup_image(map: &HashMap<String, u64>, image: &str) -> Option<u64> {
    if let Some(s) = map.get(image) {
        return Some(*s);
    }
    let (repo, _) = util::short_ref(image);
    map.get(&repo).copied()
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
}
