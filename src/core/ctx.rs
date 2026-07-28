use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use anyhow::{anyhow, Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::core::style;

pub const AC_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub app_root: String,
    pub sparse_bundle: String,
    pub image_mount: String,
    pub start_timeout: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app_root: String::new(),
            sparse_bundle: String::new(),
            image_mount: String::new(),
            start_timeout: 90,
        }
    }
}

impl Config {
    fn from_json(v: &serde_json::Value) -> Self {
        let d = Config::default();
        Config {
            app_root: v
                .get("appRoot")
                .and_then(|x| x.as_str())
                .unwrap_or(&d.app_root)
                .to_string(),
            sparse_bundle: v
                .get("sparseBundle")
                .and_then(|x| x.as_str())
                .unwrap_or(&d.sparse_bundle)
                .to_string(),
            image_mount: v
                .get("imageMount")
                .and_then(|x| x.as_str())
                .unwrap_or(&d.image_mount)
                .to_string(),
            start_timeout: v
                .get("startTimeout")
                .and_then(|x| x.as_u64())
                .unwrap_or(d.start_timeout),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "appRoot": self.app_root,
            "sparseBundle": self.sparse_bundle,
            "imageMount": self.image_mount,
            "startTimeout": self.start_timeout,
        })
    }
}

pub struct Ctx {
    pub json: bool,
    pub quiet: bool,
    pub color: bool,
    echoed: Mutex<HashSet<String>>,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub owner_file: PathBuf,
    pub supervisor_pidfile: PathBuf,
    pub supervisor_log: PathBuf,
    pub ac_home: PathBuf,
    pub config: Config,
}

impl Ctx {
    pub fn new(json: bool, quiet: bool, no_color: bool) -> Result<Self> {
        let home = home_dir()?;
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let state_home = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"));

        let config_dir = config_home.join("ac");
        let state_dir = state_home.join("ac");
        fs::create_dir_all(&config_dir).ok();
        fs::create_dir_all(&state_dir).ok();

        let quiet = quiet || json || env::var_os("AC_QUIET").is_some();
        let color =
            !no_color && !json && env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        owo_colors::set_override(color);
        set_quiet(quiet);

        let ctx = Ctx {
            json,
            quiet,
            color,
            echoed: Mutex::new(HashSet::new()),
            config_file: config_dir.join("config.json"),
            owner_file: state_dir.join("daemon.owned"),
            supervisor_pidfile: state_dir.join("supervisor.pid"),
            supervisor_log: state_dir.join("supervisor.log"),
            ac_home: ac_home(),
            config_dir,
            config: Config::default(),
        };
        let mut ctx = ctx;
        ctx.config = ctx.load_or_seed_config()?;
        Ok(ctx)
    }

    fn load_or_seed_config(&self) -> Result<Config> {
        if self.config_file.exists() {
            let text = fs::read_to_string(&self.config_file)
                .with_context(|| format!("reading {}", self.config_file.display()))?;
            let v: serde_json::Value = serde_json::from_str(&text)
                .with_context(|| format!("parsing {}", self.config_file.display()))?;
            return Ok(Config::from_json(&v));
        }

        let mut cfg = Config::default();
        if let Some(root) = probe_running_app_root() {
            cfg.app_root = root;
        }
        fs::write(
            &self.config_file,
            format!("{}\n", serde_json::to_string_pretty(&cfg.to_json())?),
        )
        .with_context(|| format!("writing {}", self.config_file.display()))?;
        self.dim(&format!("created {}", self.config_file.display()));
        Ok(cfg)
    }

    fn out(&self, line: &str) {
        if self.json {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }

    pub fn log(&self, msg: &str) {
        self.out(msg);
    }
    pub fn info(&self, msg: &str) {
        self.out(&format!("{} {msg}", style::blue("==>")));
    }
    pub fn ok(&self, msg: &str) {
        self.out(&format!("{} {msg}", style::green("ok")));
    }
    pub fn dim(&self, msg: &str) {
        self.out(&style::dim(msg));
    }
    pub fn warn(&self, msg: &str) {
        eprintln!("{} {msg}", style::yellow("warn"));
    }
    pub fn err(&self, msg: &str) {
        eprintln!("{} {msg}", style::red("err"));
    }

    pub fn emit_json(&self, v: &serde_json::Value) -> Result<()> {
        let mut stdout = io::stdout();
        writeln!(stdout, "{}", serde_json::to_string_pretty(v)?)?;
        Ok(())
    }

    pub fn container<I, S>(&self, args: I) -> Runner<'_>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Runner::new(
            self,
            "container",
            args.into_iter().map(Into::into).collect::<Vec<String>>(),
        )
    }

    pub fn exec<I, S>(&self, prog: &str, args: I) -> Runner<'_>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Runner::new(
            self,
            prog,
            args.into_iter().map(Into::into).collect::<Vec<String>>(),
        )
    }
}

pub struct Runner<'a> {
    ctx: &'a Ctx,
    prog: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    envs: Vec<(String, String)>,
    silent: bool,
    once: bool,
}

impl<'a> Runner<'a> {
    fn new(ctx: &'a Ctx, prog: &str, args: Vec<String>) -> Self {
        Runner {
            ctx,
            prog: prog.to_string(),
            args,
            cwd: None,
            envs: Vec::new(),
            silent: false,
            once: false,
        }
    }

    pub fn cwd(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.envs
            .extend(vars.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    pub fn silent(mut self) -> Self {
        self.silent = true;
        self
    }

    pub fn echo_once(mut self) -> Self {
        self.once = true;
        self
    }

    pub fn display(&self) -> String {
        let mut s = self.prog.clone();
        for a in &self.args {
            s.push(' ');
            s.push_str(&shell_quote(a));
        }
        s
    }

    fn echo(&self) {
        if self.silent || self.ctx.quiet {
            return;
        }
        let line = self.display();
        if self.once {
            let Ok(mut seen) = self.ctx.echoed.lock() else {
                return;
            };
            if !seen.insert(line.clone()) {
                return;
            }
        }
        eprintln!("{}", style::dim_err(&format!("$ {line}")));
    }

    fn build(&self) -> Command {
        let mut c = Command::new(&self.prog);
        c.args(&self.args);
        if let Some(d) = &self.cwd {
            c.current_dir(d);
        }
        for (k, v) in &self.envs {
            c.env(k, v);
        }
        c
    }

    pub fn status(&self) -> Result<ExitStatus> {
        self.echo();
        self.build()
            .status()
            .with_context(|| format!("running: {}", self.display()))
    }

    pub fn quiet_ok(&self) -> bool {
        self.echo();
        self.build()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn output(&self) -> Result<Output> {
        self.echo();
        self.build()
            .output()
            .with_context(|| format!("running: {}", self.display()))
    }

    pub fn stdout(&self) -> Result<String> {
        let out = {
            self.echo();
            self.build()
                .stderr(Stdio::null())
                .output()
                .with_context(|| format!("running: {}", self.display()))?
        };
        if !out.status.success() {
            return Err(anyhow!("{} exited {}", self.display(), out.status));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    pub fn quiet_ok_timeout(&self, secs: u64) -> Option<bool> {
        self.echo();
        let mut child = self
            .build()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status.success()),
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        child.kill().ok();
                        child.wait().ok();
                        return None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => return Some(false),
            }
        }
    }

    pub fn stdout_timeout(&self, secs: u64) -> Result<String> {
        self.echo();
        let mut child = self
            .build()
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawning: {}", self.display()))?;
        let mut out = child.stdout.take();
        let reader = std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = String::new();
            if let Some(o) = out.as_mut() {
                o.read_to_string(&mut buf).ok();
            }
            buf
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let text = reader.join().unwrap_or_default();
                    if !status.success() {
                        return Err(anyhow!("{} exited {status}", self.display()));
                    }
                    return Ok(text);
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        child.kill().ok();
                        child.wait().ok();
                        reader.join().ok();
                        return Err(anyhow!(
                            "{} did not finish within {secs}s and was killed",
                            self.display()
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(anyhow!("waiting on {}: {e}", self.display())),
            }
        }
    }

    pub fn spawn_piped(&self) -> Result<std::process::Child> {
        self.echo();
        self.build()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawning: {}", self.display()))
    }

    pub fn command(&self) -> Command {
        self.echo();
        self.build()
    }
}

static QUIET: AtomicBool = AtomicBool::new(false);

pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub fn echo_external<S: AsRef<str>>(prog: &str, args: &[S]) {
    if QUIET.load(Ordering::Relaxed) {
        return;
    }
    let mut line = prog.to_string();
    for a in args {
        line.push(' ');
        line.push_str(&shell_quote(a.as_ref()));
    }
    eprintln!("{}", style::dim_err(&format!("$ {line}")));
}

pub fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/:=@+,".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn ac_home() -> PathBuf {
    if let Some(h) = env::var_os("AC_HOME") {
        return PathBuf::from(h);
    }
    if let Ok(exe) = env::current_exe() {
        let exe = fs::canonicalize(&exe).unwrap_or(exe);
        for anc in exe.ancestors() {
            if anc.join("projects").is_dir() {
                return anc.to_path_buf();
            }
        }
    }
    home_dir()
        .map(|h| h.join("scripts/ac"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn probe_running_app_root() -> Option<String> {
    echo_external("container", &["system", "status"]);
    let out = Command::new("container")
        .args(["system", "status"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_app_root(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_app_root(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("appRoot") {
            let v = rest.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub fn now_stamp() -> String {
    echo_external("date", &["+%Y%m%d%H%M%S"]);
    Command::new("date")
        .arg("+%Y%m%d%H%M%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
