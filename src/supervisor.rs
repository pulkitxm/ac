//! The background watchdog that ties daemon lifetime to container lifetime.
//!
//! It is only ever started when ac itself started the daemon. If the daemon was
//! already running when ac was invoked, no supervisor is spawned at all and the
//! daemon is left completely alone.
//!
//! The watchdog exists so the daemon is reaped even when containers go away
//! without `ac <project> stop`: they crash, exit on their own, or are killed
//! with plain `container stop`.

use std::env;
use std::fs;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::ctx::Ctx;
use crate::{daemon, state};

/// Seconds between checks.
fn poll_interval() -> u64 {
    env::var("AC_POLL_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

/// Consecutive idle polls required before the daemon is stopped. The debounce
/// matters because containers momentarily disappear during a restart, and a
/// single idle sample would tear the daemon down under a running `ac restart`.
fn idle_grace() -> u32 {
    env::var("AC_IDLE_GRACE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4)
}

pub fn pid(ctx: &Ctx) -> Option<u32> {
    let text = fs::read_to_string(&ctx.supervisor_pidfile).ok()?;
    let pid: u32 = text.trim().parse().ok()?;
    if process_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

pub fn running(ctx: &Ctx) -> bool {
    pid(ctx).is_some()
}

fn process_alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Spawn the watchdog if (and only if) ac owns the daemon.
pub fn ensure(ctx: &Ctx) -> Result<()> {
    if !daemon::is_ours(ctx) {
        return Ok(());
    }
    if running(ctx) {
        return Ok(());
    }

    let exe = env::current_exe()?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.supervisor_log)?;
    let log2 = log.try_clone()?;

    // `nohup` matches the bash implementation: the child survives a hangup but
    // still shares the terminal's process group, so Ctrl-C behaves the same.
    let child = std::process::Command::new("nohup")
        .arg(&exe)
        .arg("__supervise")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log2))
        .spawn()?;

    fs::write(&ctx.supervisor_pidfile, format!("{}\n", child.id()))?;
    ctx.dim(&format!(
        "supervisor started (pid {}) - daemon will stop when the last container exits",
        child.id()
    ));
    Ok(())
}

pub fn stop(ctx: &Ctx) {
    if let Some(p) = pid(ctx) {
        std::process::Command::new("/bin/kill")
            .arg(p.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok();
    }
    fs::remove_file(&ctx.supervisor_pidfile).ok();
}

/// Synchronous teardown check, used by `stop`, `down`, `rm` and `prune`.
///
/// Refcounts across ALL projects: another project still running means the
/// daemon stays up even though this project is done with it.
pub fn settle(ctx: &Ctx) -> Result<()> {
    let remaining = state::ac_running_containers(ctx, false);
    if !remaining.is_empty() {
        ctx.dim(&format!(
            "{} ac container(s) still running - leaving daemon up",
            remaining.len()
        ));
        return Ok(());
    }
    stop(ctx);
    daemon::release(ctx)
}

/// The watchdog loop itself (`ac __supervise`). Runs detached.
pub fn run_loop(ctx: &Ctx) -> Result<()> {
    let interval = Duration::from_secs(poll_interval());
    let grace = idle_grace();

    // `armed` guarantees the watchdog never acts before containers have
    // actually appeared, otherwise it would race `ac start` and stop the daemon
    // mid-startup. `idle` counts consecutive polls with zero ac containers and
    // is reset the moment any container reappears.
    let mut armed = false;
    let mut idle: u32 = 0;

    loop {
        // If the daemon went away or ownership was dropped, our job is over.
        if !daemon::is_ours(ctx) || !daemon::running_silent(ctx) {
            fs::remove_file(&ctx.supervisor_pidfile).ok();
            return Ok(());
        }

        let count = state::ac_running_containers(ctx, true).len();

        if count > 0 {
            if !armed {
                eprintln!("supervisor: armed, {count} container(s) running");
            }
            armed = true;
            idle = 0;
            thread::sleep(interval);
            continue;
        }

        if !armed {
            thread::sleep(interval);
            continue;
        }

        idle += 1;
        eprintln!("supervisor: idle poll {idle}/{grace}");

        if idle >= grace {
            eprintln!("supervisor: {grace} consecutive idle polls, stopping daemon");
            daemon::release(ctx)?;
            fs::remove_file(&ctx.supervisor_pidfile).ok();
            return Ok(());
        }

        thread::sleep(interval);
    }
}
