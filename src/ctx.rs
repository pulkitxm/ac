//! Shared state: paths, configuration, logging and the command runner.
//!
//! Everything that the bash implementation kept in `lib/common.sh` lives here.
//! The one rule worth remembering: every `container` invocation goes through
//! [`Runner`], which echoes the command before running it, so any step can be
//! copied and re-run by hand and an agent reading the output can see exactly
//! what was executed.

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

use anyhow::{anyhow, Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::style;

pub const AC_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `~/.config/ac/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Passed as `--app-root` every time `ac` starts the daemon. Apple
    /// `container` does not remember it, so it must be supplied on each start.
    pub app_root: String,
    /// APFS sparse bundle backing `app_root`, attached before the daemon starts.
    pub sparse_bundle: String,
    /// Mount point the sparse bundle appears at once attached.
    pub image_mount: String,
    /// Seconds `container system start` is allowed to take.
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

// The on-disk file uses camelCase keys, matching the bash implementation.
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

/// Process-wide context handed to every subsystem.
pub struct Ctx {
    /// Emit machine readable JSON instead of a human table. Implies `quiet`.
    pub json: bool,
    /// Suppress the `$ container ...` echo. Set by `--quiet` or `AC_QUIET=1`.
    pub quiet: bool,
    /// Whether ANSI colour is allowed on stdout.
    pub color: bool,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    /// Presence of this file is the single source of truth for "ac started the
    /// daemon, so ac is allowed to stop it".
    pub owner_file: PathBuf,
    pub supervisor_pidfile: PathBuf,
    pub supervisor_log: PathBuf,
    /// Repository root, used to find the bundled `projects/` directory.
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

        // JSON output must stay parseable, so it forces the echo off.
        let quiet = quiet || json || env::var_os("AC_QUIET").is_some();
        let color =
            !no_color && !json && env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        owo_colors::set_override(color);

        let ctx = Ctx {
            json,
            quiet,
            color,
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

    /// Seed a config file on first run. If the daemon happens to be running we
    /// adopt its current appRoot, so `ac` keeps using the image store the user
    /// already has rather than silently starting a second one.
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

    // ------------------------------------------------------------ logging ---

    /// Human log lines go to stdout normally, but to stderr in `--json` mode so
    /// that stdout stays a single parseable document.
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
        self.out(&format!("{} {msg}", style::green("  ok")));
    }
    pub fn dim(&self, msg: &str) {
        self.out(&style::dim(msg));
    }
    pub fn warn(&self, msg: &str) {
        eprintln!("{} {msg}", style::yellow("warn"));
    }
    pub fn err(&self, msg: &str) {
        eprintln!("{} {msg}", style::red(" err"));
    }

    /// Print a JSON document to stdout.
    pub fn emit_json(&self, v: &serde_json::Value) -> Result<()> {
        let mut stdout = io::stdout();
        writeln!(stdout, "{}", serde_json::to_string_pretty(v)?)?;
        Ok(())
    }

    // ------------------------------------------------------------ running ---

    /// A `container` command, echoed before it runs.
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

    /// Any other external command, echoed the same way.
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

/// A single external command invocation.
pub struct Runner<'a> {
    ctx: &'a Ctx,
    prog: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    /// Suppress the echo for this one command (used by hot polling loops).
    silent: bool,
}

impl<'a> Runner<'a> {
    fn new(ctx: &'a Ctx, prog: &str, args: Vec<String>) -> Self {
        Runner {
            ctx,
            prog: prog.to_string(),
            args,
            cwd: None,
            silent: false,
        }
    }

    pub fn cwd(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Do not echo this invocation. Reserved for the supervisor's poll loop and
    /// for readiness probes, which would otherwise flood the output.
    pub fn silent(mut self) -> Self {
        self.silent = true;
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
        eprintln!("{}", style::dim_err(&format!("   $ {}", self.display())));
    }

    fn build(&self) -> Command {
        let mut c = Command::new(&self.prog);
        c.args(&self.args);
        if let Some(d) = &self.cwd {
            c.current_dir(d);
        }
        c
    }

    /// Run with stdio inherited, returning the exit status.
    pub fn status(&self) -> Result<ExitStatus> {
        self.echo();
        self.build()
            .status()
            .with_context(|| format!("running: {}", self.display()))
    }

    /// Run with stdout and stderr discarded. Returns true on exit code 0.
    pub fn quiet_ok(&self) -> bool {
        self.echo();
        self.build()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Run capturing stdout and stderr.
    pub fn output(&self) -> Result<Output> {
        self.echo();
        self.build()
            .output()
            .with_context(|| format!("running: {}", self.display()))
    }

    /// Run capturing stdout only; stderr is discarded. Returns stdout as a
    /// string, or an error when the command fails.
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

    /// Spawn without waiting, with stdio inherited.
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

// ------------------------------------------------------------------ helpers ---

pub fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

/// Quote an argument only when it needs it, so the echoed line stays readable
/// but remains copy-pasteable into a shell.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/:=@+,".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Where the bundled `projects/` directory lives.
///
/// `$AC_HOME` wins. Otherwise walk up from the resolved executable looking for
/// a directory that contains `projects/`, which covers both
/// `<repo>/target/release/ac` and a symlink into `~/.local/bin`.
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

/// `appRoot` of a daemon that is already up, used only to seed the config file.
fn probe_running_app_root() -> Option<String> {
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

/// Pull the `appRoot` value out of `container system status` table output.
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

/// `date +<fmt>`, matching what the bash implementation produced. Shelling out
/// avoids pulling in a date library for two format strings.
pub fn now_stamp() -> String {
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
