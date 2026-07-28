use anyhow::Result;

use crate::cli::{ImageAction, NetworkAction, RegistryAction, SystemAction, VolumeAction};
use crate::ctx::Ctx;
use crate::{daemon, manifest, supervisor};

fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

fn short_ref(full: &str) -> (String, String) {
    let (repo, tag) = match full.rfind(':') {
        Some(i) if !full[i + 1..].contains('/') => (&full[..i], &full[i + 1..]),
        _ => (full, "latest"),
    };
    let repo = repo
        .strip_prefix("docker.io/library/")
        .or_else(|| repo.strip_prefix("docker.io/"))
        .unwrap_or(repo);
    (repo.to_string(), tag.to_string())
}

fn fmt_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.1} kB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

fn fmt_date(iso: &str) -> String {
    let mut out: String = iso.chars().take(19).collect();
    if let Some(i) = out.find('T') {
        out.replace_range(i..i + 1, " ");
    }
    out
}

fn print_pretty_json(ctx: &Ctx, args: Vec<String>) -> Result<()> {
    let text = ctx.container(args).stdout()?;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(_) => print!("{text}"),
    }
    Ok(())
}

fn exit_ok(status: std::process::ExitStatus) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("command exited {status}"))
    }
}

fn passthrough_json(ctx: &Ctx, args: &[&str]) -> Result<()> {
    daemon::require(ctx)?;
    if ctx.json {
        let mut argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        argv.extend(["--format".to_string(), "json".to_string()]);
        let text = ctx.container(argv).stdout()?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        ctx.emit_json(&v)
    } else {
        exit_ok(ctx.container(args.to_vec()).status()?)
    }
}

fn passthrough_raw_json(ctx: &Ctx, args: Vec<String>) -> Result<()> {
    daemon::require(ctx)?;
    if ctx.json {
        let text = ctx.container(args).stdout()?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        ctx.emit_json(&v)
    } else {
        print_pretty_json(ctx, args)
    }
}

fn passthrough(ctx: &Ctx, args: Vec<String>) -> Result<()> {
    daemon::ensure(ctx)?;
    let status = ctx.container(args).status()?;
    supervisor::settle(ctx)?;
    exit_ok(status)
}

pub fn ps(ctx: &Ctx, all: bool, ids: bool) -> Result<()> {
    daemon::require(ctx)?;
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

    struct Row {
        id: String,
        project: Option<String>,
        service: Option<String>,
        state: String,
        ip: Option<String>,
        image: Option<String>,
    }

    let rows: Vec<Row> = raw
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
            Some(Row {
                id,
                project,
                service,
                state,
                ip,
                image,
            })
        })
        .collect();

    if ids {
        if ctx.json {
            let names: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
            return ctx.emit_json(&serde_json::json!(names));
        }
        for r in &rows {
            println!("{}", r.id);
        }
        return Ok(());
    }

    if ctx.json {
        let items: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "container": r.id,
                    "project": r.project,
                    "service": r.service,
                    "state": r.state,
                    "ip": r.ip,
                    "image": r.image,
                })
            })
            .collect();
        return ctx.emit_json(&serde_json::Value::Array(items));
    }

    ctx.log(&crate::style::bold(&format!(
        "{:<22} {:<10} {:<12} {:<10} {:<18} {}",
        "CONTAINER", "PROJECT", "SERVICE", "STATE", "IP", "IMAGE"
    )));
    for r in &rows {
        ctx.log(&format!(
            "{:<22} {:<10} {:<12} {:<10} {:<18} {}",
            r.id,
            r.project.as_deref().unwrap_or("-"),
            r.service.as_deref().unwrap_or("-"),
            r.state,
            r.ip.as_deref().unwrap_or("-"),
            r.image.as_deref().unwrap_or("-"),
        ));
    }
    Ok(())
}

pub fn image(ctx: &Ctx, action: Option<&ImageAction>) -> Result<()> {
    let default = ImageAction::Ls {
        verbose: false,
        ids: false,
    };
    match action.unwrap_or(&default) {
        ImageAction::Ls { verbose, ids } => {
            if *ids {
                daemon::require(ctx)?;
                if ctx.json {
                    let text = ctx.container(["image", "ls", "-q"]).stdout()?;
                    let names: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
                    return ctx.emit_json(&serde_json::json!(names));
                }
                return exit_ok(ctx.container(["image", "ls", "-q"]).status()?);
            }
            if ctx.json {
                return passthrough_json(ctx, &["image", "ls"]);
            }
            daemon::require(ctx)?;
            if *verbose {
                return exit_ok(ctx.container(["image", "ls", "--verbose"]).status()?);
            }
            let text = ctx
                .container(["image", "ls", "--format", "json"])
                .stdout()?;
            let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;
            let mut rows: Vec<(String, String, String, u64, String)> = raw
                .iter()
                .filter_map(|e| {
                    let full = e.get("configuration")?.get("name")?.as_str()?;
                    let (repo, tag) = short_ref(full);
                    let variants = e.get("variants")?.as_array()?;
                    let pick = variants
                        .iter()
                        .find(|v| {
                            v.get("platform")
                                .and_then(|p| p.get("architecture"))
                                .and_then(|a| a.as_str())
                                == Some(host_arch())
                        })
                        .or_else(|| variants.first())?;
                    let arch = pick
                        .get("platform")
                        .and_then(|p| p.get("architecture"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("-")
                        .to_string();
                    let size = pick.get("size").and_then(|x| x.as_u64()).unwrap_or(0);
                    let created = pick
                        .get("config")
                        .and_then(|c| c.get("created"))
                        .and_then(|x| x.as_str())
                        .or_else(|| {
                            e.get("configuration")
                                .and_then(|c| c.get("creationDate"))
                                .and_then(|x| x.as_str())
                        })
                        .map(fmt_date)
                        .unwrap_or_default();
                    Some((repo, tag, arch, size, created))
                })
                .collect();
            rows.sort();
            let name_w = rows.iter().map(|r| r.0.len()).max().unwrap_or(4).max(4);
            let tag_w = rows.iter().map(|r| r.1.len()).max().unwrap_or(3).max(3);
            ctx.log(&crate::style::bold(&format!(
                "{:<name_w$}  {:<tag_w$}  {:<7} {:>9}  {}",
                "NAME", "TAG", "ARCH", "SIZE", "CREATED"
            )));
            for (repo, tag, arch, size, created) in &rows {
                ctx.log(&format!(
                    "{repo:<name_w$}  {tag:<tag_w$}  {arch:<7} {:>9}  {created}",
                    fmt_size(*size)
                ));
            }
            ctx.dim("one row per tag, sized for this machine; every variant: ac image ls -v");
            Ok(())
        }
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
        ImageAction::Rm {
            force: _,
            references,
        } => {
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
        VolumeAction::Ls => {
            if ctx.json {
                return passthrough_json(ctx, &["volume", "ls"]);
            }
            daemon::require(ctx)?;
            let text = ctx
                .container(["volume", "ls", "--format", "json"])
                .stdout()?;
            let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;
            let mut rows: Vec<(String, String, String, String)> = raw
                .iter()
                .filter_map(|e| {
                    let c = e.get("configuration")?;
                    Some((
                        c.get("name")?.as_str()?.to_string(),
                        c.get("driver")
                            .and_then(|x| x.as_str())
                            .unwrap_or("-")
                            .to_string(),
                        c.get("format")
                            .and_then(|x| x.as_str())
                            .unwrap_or("-")
                            .to_string(),
                        c.get("creationDate")
                            .and_then(|x| x.as_str())
                            .map(fmt_date)
                            .unwrap_or_default(),
                    ))
                })
                .collect();
            rows.sort();
            let name_w = rows.iter().map(|r| r.0.len()).max().unwrap_or(4).max(4);
            ctx.log(&crate::style::bold(&format!(
                "{:<name_w$}  {:<7} {:<7} {}",
                "NAME", "DRIVER", "FORMAT", "CREATED"
            )));
            for (name, driver, format, created) in &rows {
                ctx.log(&format!(
                    "{name:<name_w$}  {driver:<7} {format:<7} {created}"
                ));
            }
            Ok(())
        }
        VolumeAction::Create { name } => passthrough(
            ctx,
            vec!["volume".to_string(), "create".into(), name.clone()],
        ),
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
        SystemAction::Prune { all } => {
            daemon::ensure(ctx)?;
            ctx.info("removing stopped containers");
            ctx.container(["prune"]).status()?;
            ctx.info("removing unused images");
            let mut args = vec!["image".to_string(), "prune".into()];
            if *all {
                args.push("--all".into());
            }
            ctx.container(args).status()?;
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
        RegistryAction::Ls => passthrough_json(ctx, &["registry", "ls"]),
    }
}
