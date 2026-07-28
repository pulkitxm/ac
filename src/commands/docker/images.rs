use std::path::Path;

use anyhow::Result;

use crate::cli::BuilderAction;
use crate::core::ctx::Ctx;
use crate::core::util::exit_ok;
use crate::daemon::{self, supervisor};

pub struct BuildRequest<'a> {
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

pub fn build(ctx: &Ctx, b: &BuildRequest) -> Result<()> {
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
