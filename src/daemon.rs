//! Apple `container` daemon lifecycle, with strict ownership rules.
//!
//! THE CONTRACT:
//!   * Daemon already running when ac needs it -> ac NEVER touches it. Not on
//!     start, not on stop, not from the supervisor. It is someone else's.
//!   * Daemon not running -> ac starts it, records ownership, and is then
//!     responsible for stopping it once the last ac-managed container is gone.
//!
//! Ownership is recorded on disk (`~/.local/state/ac/daemon.owned`) rather than
//! inferred, so a new `ac` invocation in a different shell still knows what the
//! first one did.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};

use crate::ctx::{epoch_secs, parse_app_root, Ctx};

pub fn running(ctx: &Ctx) -> bool {
    ctx.container(["system", "status"]).quiet_ok()
}

/// Same check without echoing, for polling loops and status lines that already
/// print the interesting information.
pub fn running_silent(ctx: &Ctx) -> bool {
    ctx.container(["system", "status"]).silent().quiet_ok()
}

pub fn is_ours(ctx: &Ctx) -> bool {
    ctx.owner_file.exists()
}

pub fn app_root(ctx: &Ctx) -> Option<String> {
    let text = ctx.container(["system", "status"]).silent().stdout().ok()?;
    parse_app_root(&text)
}

/// Attach the APFS sparse bundle backing the app root, when one is configured.
///
/// Needed because a volume mounted `noowners` makes container-apiserver abort
/// with "XPC connection error: Connection invalid".
fn mount_backing_store(ctx: &Ctx) -> Result<()> {
    let bundle = ctx.config.sparse_bundle.trim();
    let mount = ctx.config.image_mount.trim();
    if bundle.is_empty() || mount.is_empty() {
        return Ok(());
    }
    if Path::new(mount).is_dir() {
        return Ok(());
    }
    if !Path::new(bundle).exists() {
        return Err(anyhow!("configured sparseBundle not found: {bundle}"));
    }

    let base = Path::new(bundle)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| bundle.to_string());
    ctx.info(&format!("attaching backing store {base}"));
    if !ctx
        .exec("hdiutil", ["attach", "-owners", "on", bundle])
        .quiet_ok()
    {
        return Err(anyhow!("failed to attach {bundle}"));
    }
    Ok(())
}

/// Start the daemon only if it is not already up. Safe to call repeatedly.
pub fn ensure(ctx: &Ctx) -> Result<()> {
    if running(ctx) {
        if is_ours(ctx) {
            ctx.dim("daemon already running (started by ac)");
        } else {
            ctx.dim("daemon already running (external) - ac will not manage it");
        }
        return Ok(());
    }

    // Nothing running: this invocation becomes the owner.
    mount_backing_store(ctx)?;

    let timeout = ctx.config.start_timeout.to_string();
    ctx.info("starting container daemon");

    let started = if !ctx.config.app_root.trim().is_empty() {
        ctx.dim(&format!("  app root: {}", ctx.config.app_root));
        ctx.container([
            "system",
            "start",
            "--app-root",
            ctx.config.app_root.as_str(),
            "--timeout",
            timeout.as_str(),
        ])
        .quiet_ok()
    } else {
        ctx.container(["system", "start", "--timeout", timeout.as_str()])
            .quiet_ok()
    };

    if !started {
        return Err(anyhow!("failed to start container daemon"));
    }

    fs::write(&ctx.owner_file, format!("{}\n", epoch_secs()))?;
    ctx.ok("daemon started (owned by ac)");
    Ok(())
}

/// Stop the daemon, but ONLY if ac started it.
pub fn release(ctx: &Ctx) -> Result<()> {
    if !is_ours(ctx) {
        ctx.dim("daemon was not started by ac - leaving it running");
        return Ok(());
    }
    if !running(ctx) {
        fs::remove_file(&ctx.owner_file).ok();
        return Ok(());
    }

    ctx.info("stopping container daemon (ac owned it)");
    ctx.container(["system", "stop"]).quiet_ok();
    fs::remove_file(&ctx.owner_file).ok();
    ctx.ok("daemon stopped");
    Ok(())
}

pub struct DaemonStatus {
    pub running: bool,
    pub owned_by_ac: bool,
    pub app_root: Option<String>,
}

pub fn status(ctx: &Ctx) -> DaemonStatus {
    let up = running_silent(ctx);
    DaemonStatus {
        running: up,
        owned_by_ac: is_ours(ctx),
        app_root: if up { app_root(ctx) } else { None },
    }
}

impl DaemonStatus {
    pub fn line(&self) -> String {
        if !self.running {
            return "stopped".to_string();
        }
        let owner = if self.owned_by_ac {
            "(owned by ac)"
        } else {
            "(external, untouched)"
        };
        format!(
            "running {}  appRoot={}",
            crate::style::dim(owner),
            self.app_root.clone().unwrap_or_default()
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "running": self.running,
            "ownedByAc": self.owned_by_ac,
            "appRoot": self.app_root,
        })
    }
}
