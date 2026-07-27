#![recursion_limit = "512"]

mod build;
mod cli;
mod completions;
mod ctx;
mod daemon;
mod manifest;
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
use crate::cli::{Action, Cli, CompletionShell, DaemonAction, TopCommand, RESERVED};
use crate::ctx::Ctx;
use crate::manifest::Project;
use crate::state::Snapshot;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let rewritten = match rewrite_argv(&argv) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} {e}", style::red(" err"));
            return ExitCode::FAILURE;
        }
    };

    let cli = Cli::parse_from(&rewritten);

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", style::red(" err"));
            ExitCode::FAILURE
        }
    }
}

fn rewrite_argv(argv: &[String]) -> Result<Vec<String>> {
    let prog = argv.first().cloned().unwrap_or_else(|| "ac".into());
    let rest = &argv[1..];

    let is_global_flag = |s: &str| matches!(s, "--json" | "--quiet" | "-q" | "--no-color");

    let mut lead: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() && is_global_flag(&rest[i]) {
        lead.push(rest[i].clone());
        i += 1;
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
        out.extend(rest.iter().cloned());
        return Ok(out);
    };

    if first.starts_with('-') || RESERVED.contains(&first.as_str()) {
        out.extend(rest[i..].iter().cloned());
        return Ok(out);
    }

    let probe = Ctx::new(false, true, true)?;
    let known = manifest::project_names(&probe.config_dir, &probe.ac_home);
    if !known.iter().any(|p| p == first) {
        return Err(anyhow!(
            "unknown project or command: {first}\n  projects: {}\n  commands: {}\n  try: ac --help",
            if known.is_empty() {
                "(none)".to_string()
            } else {
                known.join(" ")
            },
            RESERVED.join(" ")
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

fn run(cli: Cli) -> Result<()> {
    let ctx = Ctx::new(cli.json, cli.quiet, cli.no_color)?;

    match &cli.command {
        TopCommand::Version => {
            println!("ac {}", ctx::AC_VERSION);
            Ok(())
        }
        TopCommand::Schema => ctx.emit_json(&schema::manifest_schema()),
        TopCommand::Completions { shell } => {
            let mut cmd = Cli::command();
            let sh = match shell {
                CompletionShell::Bash => clap_complete::Shell::Bash,
                CompletionShell::Zsh => clap_complete::Shell::Zsh,
                CompletionShell::Fish => clap_complete::Shell::Fish,
                CompletionShell::Elvish => clap_complete::Shell::Elvish,
                CompletionShell::PowerShell => clap_complete::Shell::PowerShell,
            };
            let mut generated = Vec::new();
            clap_complete::generate(sh, &mut cmd, "ac", &mut generated);
            let generated = String::from_utf8_lossy(&generated).into_owned();
            print!("{}", completions::with_dynamic_projects(shell, &generated));
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
        TopCommand::Images => {
            daemon::ensure(&ctx)?;
            if ctx.json {
                let text = ctx
                    .container(["image", "ls", "--format", "json"])
                    .stdout()?;
                let v: serde_json::Value = serde_json::from_str(&text)?;
                ctx.emit_json(&v)
            } else {
                ctx.container(["image", "ls"]).status()?;
                Ok(())
            }
        }
        TopCommand::Df => {
            daemon::ensure(&ctx)?;
            if ctx.json {
                let text = ctx
                    .container(["system", "df", "--format", "json"])
                    .stdout()?;
                let v: serde_json::Value = serde_json::from_str(&text)?;
                ctx.emit_json(&v)
            } else {
                ctx.container(["system", "df"]).status()?;
                Ok(())
            }
        }
        TopCommand::Prune => {
            daemon::ensure(&ctx)?;
            ctx.info("removing stopped containers");
            ctx.container(["prune"]).status()?;
            ctx.info("removing unused images");
            ctx.container(["image", "prune"]).status()?;
            supervisor::settle(&ctx)
        }
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
        Action::Start { recreate, services } => project::start(ctx, proj, services, *recreate),

        Action::Stop { services } => project::stop(ctx, proj, services),

        Action::Down { services } => project::down(ctx, proj, services),

        Action::Restart { recreate, services } => {
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
            project::start(ctx, proj, services, *recreate)
        }

        Action::Ls => {
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

        Action::Exec { service, command } => {
            let svc = proj.target_services(std::slice::from_ref(service))?;
            let mut argv = vec!["exec".to_string()];
            argv.extend(tty_flags());
            argv.push(proj.container_name(&svc[0]));
            argv.extend(command.iter().cloned());
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Sh { service } => {
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

        Action::Stats { services } => {
            let mut argv = vec!["stats".to_string()];
            argv.extend(proj.target_container_names(services)?);
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Inspect { services } => {
            let mut argv = vec!["inspect".to_string()];
            argv.extend(proj.target_container_names(services)?);
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Kill { signal, services } => {
            let mut argv = vec!["kill".to_string(), "--signal".into(), signal.clone()];
            argv.extend(proj.target_container_names(services)?);
            let status = ctx.container(&argv).status()?;
            exit_like(status)
        }

        Action::Rm { services } => {
            let mut argv = vec!["rm".to_string(), "--force".into()];
            argv.extend(proj.target_container_names(services)?);
            ctx.container(&argv).status()?;
            supervisor::settle(ctx)
        }

        Action::Cp { src, dst } => {
            let status = ctx
                .container(["cp", &cp_path(proj, src), &cp_path(proj, dst)])
                .status()?;
            exit_like(status)
        }

        Action::Pull { services } => project::pull(ctx, proj, services),

        Action::Images => {
            if ctx.json {
                let items: Vec<serde_json::Value> = proj
                    .manifest
                    .services
                    .iter()
                    .map(|s| serde_json::json!({ "service": s.name, "image": s.image }))
                    .collect();
                return ctx.emit_json(&serde_json::Value::Array(items));
            }
            println!("{}", style::bold(&format!("{:<14} {}", "SERVICE", "IMAGE")));
            for s in &proj.manifest.services {
                println!("{:<14} {}", s.name, s.image);
            }
            Ok(())
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
            };
            build::project_build(ctx, proj, &args.names, &ov)
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

fn cp_path(proj: &Project, arg: &str) -> String {
    if arg.starts_with('/') {
        return arg.to_string();
    }
    let Some((head, tail)) = arg.split_once(':') else {
        return arg.to_string();
    };
    if head.contains('/') {
        return arg.to_string();
    }
    if proj.has_service(head) {
        return format!(
            "{}:{tail}",
            proj.container_name(&proj.normalize_service(head))
        );
    }
    arg.to_string()
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
    fn cp_rewrites_only_service_refs() {
        let p = proj_fixture();
        assert_eq!(cp_path(&p, "redis:/data"), "demo-redis:/data");
        assert_eq!(cp_path(&p, "demo-redis:/data"), "demo-redis:/data");
        assert_eq!(cp_path(&p, "/etc/hosts"), "/etc/hosts");
        assert_eq!(cp_path(&p, "./a/b:c"), "./a/b:c");
        assert_eq!(cp_path(&p, "unknown:/x"), "unknown:/x");
        assert_eq!(cp_path(&p, "plain.txt"), "plain.txt");
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
        };
        let s = "{{profile}}|{{account}}|{{tag}}|{{region}}|{{registry}}|{{version}}|\
                 {{git.sha}}|{{git.shortSha}}|{{git.branch}}|{{git.dirtySuffix}}|{{timestamp}}";
        assert_eq!(
            build::interpolate(s, &v),
            "dev|123|t|us-east-1|r/|1.2.3|abc|ab|main|-local-1|20260101000000"
        );
        assert!(!build::interpolate(s, &v).contains("{{"));
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
