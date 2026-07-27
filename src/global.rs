use anyhow::Result;

use crate::cli::{ImageAction, NetworkAction, RegistryAction, SystemAction, VolumeAction};
use crate::ctx::Ctx;
use crate::{daemon, manifest, supervisor};

fn passthrough_json(ctx: &Ctx, args: &[&str]) -> Result<()> {
    daemon::require(ctx)?;
    if ctx.json {
        let mut argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        argv.extend(["--format".to_string(), "json".to_string()]);
        let text = ctx.container(argv).stdout()?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        ctx.emit_json(&v)
    } else {
        ctx.container(args.to_vec()).status()?;
        Ok(())
    }
}

fn passthrough_raw_json(ctx: &Ctx, args: Vec<String>) -> Result<()> {
    daemon::require(ctx)?;
    if ctx.json {
        let text = ctx.container(args).stdout()?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        ctx.emit_json(&v)
    } else {
        ctx.container(args).status()?;
        Ok(())
    }
}

fn passthrough(ctx: &Ctx, args: Vec<String>) -> Result<()> {
    daemon::ensure(ctx)?;
    let status = ctx.container(args).status()?;
    supervisor::settle(ctx)?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("command exited {status}"))
    }
}

pub fn ps(ctx: &Ctx, all: bool) -> Result<()> {
    daemon::require(ctx)?;
    if !ctx.json {
        let mut args = vec!["ls".to_string()];
        if all {
            args.push("-a".into());
        }
        ctx.container(args).status()?;
        return Ok(());
    }

    let text = ctx.container(["ls", "-a", "--format", "json"]).stdout()?;
    let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;
    let projects = manifest::load_all(&ctx.config_dir, &ctx.ac_home);

    let attribute = |cname: &str, label: Option<&str>| -> (Option<String>, Option<String>) {
        for p in &projects {
            if label == Some(p.name.as_str()) || cname.starts_with(&format!("{}-", p.name)) {
                for s in &p.manifest.services {
                    if p.container_name(&s.name) == cname {
                        return (Some(p.name.clone()), Some(s.name.clone()));
                    }
                }
                if label == Some(p.name.as_str()) {
                    return (Some(p.name.clone()), None);
                }
            }
        }
        (None, None)
    };

    let items: Vec<serde_json::Value> = raw
        .iter()
        .filter_map(|c| {
            let id = c.get("configuration")?.get("id")?.as_str()?.to_string();
            let state = c
                .get("status")
                .and_then(|s| s.get("state"))
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string();
            if !all && state != "running" {
                return None;
            }
            let ip = c
                .get("status")
                .and_then(|s| s.get("networks"))
                .and_then(|n| n.get(0))
                .and_then(|n| n.get("ipv4Address"))
                .and_then(|x| x.as_str())
                .map(String::from);
            let image = c
                .get("configuration")
                .and_then(|cfg| cfg.get("image"))
                .and_then(|i| i.get("reference"))
                .and_then(|x| x.as_str())
                .map(String::from);
            let label = c
                .get("configuration")
                .and_then(|cfg| cfg.get("labels"))
                .and_then(|l| l.get("ac.project"))
                .and_then(|x| x.as_str());
            let (project, service) = attribute(&id, label);
            Some(serde_json::json!({
                "container": id,
                "project": project,
                "service": service,
                "state": state,
                "ip": ip,
                "image": image,
            }))
        })
        .collect();
    ctx.emit_json(&serde_json::Value::Array(items))
}

pub fn image(ctx: &Ctx, action: Option<&ImageAction>) -> Result<()> {
    match action.unwrap_or(&ImageAction::Ls) {
        ImageAction::Ls => passthrough_json(ctx, &["image", "ls"]),
        ImageAction::Pull {
            reference,
            platform,
        } => {
            let mut args = vec!["image".to_string(), "pull".into()];
            if let Some(p) = platform {
                args.extend(["--platform".to_string(), p.clone()]);
            }
            args.push(reference.clone());
            passthrough(ctx, args)
        }
        ImageAction::Push {
            reference,
            platform,
        } => {
            let mut args = vec!["image".to_string(), "push".into()];
            if let Some(p) = platform {
                args.extend(["--platform".to_string(), p.clone()]);
            }
            args.push(reference.clone());
            passthrough(ctx, args)
        }
        ImageAction::Rm { references } => {
            let mut args = vec!["image".to_string(), "rm".into()];
            args.extend(references.iter().cloned());
            passthrough(ctx, args)
        }
        ImageAction::Tag { source, target } => passthrough(
            ctx,
            vec![
                "image".to_string(),
                "tag".into(),
                source.clone(),
                target.clone(),
            ],
        ),
        ImageAction::Inspect { references } => {
            let mut args = vec!["image".to_string(), "inspect".into()];
            args.extend(references.iter().cloned());
            passthrough_raw_json(ctx, args)
        }
        ImageAction::Prune { all } => {
            let mut args = vec!["image".to_string(), "prune".into()];
            if *all {
                args.push("--all".into());
            }
            passthrough(ctx, args)
        }
        ImageAction::Save {
            references,
            output,
            platform,
        } => {
            let mut args = vec![
                "image".to_string(),
                "save".into(),
                "-o".into(),
                output.to_string_lossy().to_string(),
            ];
            if let Some(p) = platform {
                args.extend(["--platform".to_string(), p.clone()]);
            }
            args.extend(references.iter().cloned());
            passthrough(ctx, args)
        }
        ImageAction::Load { input } => passthrough(
            ctx,
            vec![
                "image".to_string(),
                "load".into(),
                "-i".into(),
                input.to_string_lossy().to_string(),
            ],
        ),
    }
}

pub fn volume(ctx: &Ctx, action: Option<&VolumeAction>) -> Result<()> {
    match action.unwrap_or(&VolumeAction::Ls) {
        VolumeAction::Ls => passthrough_json(ctx, &["volume", "ls"]),
        VolumeAction::Create { name } => {
            passthrough(ctx, vec!["volume".to_string(), "create".into(), name.clone()])
        }
        VolumeAction::Rm { names } => {
            let mut args = vec!["volume".to_string(), "rm".into()];
            args.extend(names.iter().cloned());
            passthrough(ctx, args)
        }
        VolumeAction::Inspect { names } => {
            let mut args = vec!["volume".to_string(), "inspect".into()];
            args.extend(names.iter().cloned());
            passthrough_raw_json(ctx, args)
        }
        VolumeAction::Prune => passthrough(ctx, vec!["volume".to_string(), "prune".into()]),
    }
}

pub fn network(ctx: &Ctx, action: Option<&NetworkAction>) -> Result<()> {
    match action.unwrap_or(&NetworkAction::Ls) {
        NetworkAction::Ls => passthrough_json(ctx, &["network", "ls"]),
        NetworkAction::Create {
            name,
            internal,
            subnet,
        } => {
            let mut args = vec!["network".to_string(), "create".into()];
            if *internal {
                args.push("--internal".into());
            }
            if let Some(s) = subnet {
                args.extend(["--subnet".to_string(), s.clone()]);
            }
            args.push(name.clone());
            passthrough(ctx, args)
        }
        NetworkAction::Rm { names } => {
            let mut args = vec!["network".to_string(), "rm".into()];
            args.extend(names.iter().cloned());
            passthrough(ctx, args)
        }
        NetworkAction::Inspect { names } => {
            let mut args = vec!["network".to_string(), "inspect".into()];
            args.extend(names.iter().cloned());
            passthrough_raw_json(ctx, args)
        }
        NetworkAction::Prune => passthrough(ctx, vec!["network".to_string(), "prune".into()]),
    }
}

pub fn system(ctx: &Ctx, action: Option<&SystemAction>) -> Result<()> {
    match action.unwrap_or(&SystemAction::Info) {
        SystemAction::Info => {
            let d = daemon::status(ctx);
            let sup = supervisor::pid(ctx);
            if ctx.json {
                return ctx.emit_json(&serde_json::json!({
                    "daemon": d.to_json(),
                    "supervisor": { "running": sup.is_some(), "pid": sup },
                }));
            }
            println!("{}  {}", crate::style::bold("daemon"), d.line());
            match sup {
                Some(p) => println!("{}  running (pid {p})", crate::style::bold("supervisor")),
                None => println!("{}  not running", crate::style::bold("supervisor")),
            }
            Ok(())
        }
        SystemAction::Df => passthrough_json(ctx, &["system", "df"]),
        SystemAction::Start => daemon::ensure(ctx),
        SystemAction::Stop => {
            supervisor::stop(ctx);
            daemon::release(ctx)
        }
        SystemAction::Prune => {
            daemon::ensure(ctx)?;
            ctx.info("removing stopped containers");
            ctx.container(["prune"]).status()?;
            ctx.info("removing unused images");
            ctx.container(["image", "prune"]).status()?;
            supervisor::settle(ctx)
        }
        SystemAction::Logs { follow, last } => {
            let mut args = vec!["system".to_string(), "logs".into()];
            if *follow {
                args.push("-f".into());
            }
            if let Some(l) = last {
                args.extend(["--last".to_string(), l.clone()]);
            }
            ctx.container(args).status()?;
            Ok(())
        }
    }
}

pub fn registry(ctx: &Ctx, action: Option<&RegistryAction>) -> Result<()> {
    match action.unwrap_or(&RegistryAction::Ls) {
        RegistryAction::Login {
            server,
            username,
            password_stdin,
        } => {
            daemon::ensure(ctx)?;
            let mut args = vec!["registry".to_string(), "login".into()];
            if let Some(u) = username {
                args.extend(["-u".to_string(), u.clone()]);
            }
            if *password_stdin {
                args.push("--password-stdin".into());
            }
            args.push(server.clone());
            let status = ctx.container(args).status()?;
            supervisor::settle(ctx)?;
            if status.success() {
                ctx.ok(&format!("authenticated to {server}"));
                Ok(())
            } else {
                Err(anyhow::anyhow!("login to {server} failed"))
            }
        }
        RegistryAction::Logout { server } => passthrough(
            ctx,
            vec!["registry".to_string(), "logout".into(), server.clone()],
        ),
        RegistryAction::Ls => passthrough(ctx, vec!["registry".to_string(), "ls".into()]),
    }
}
