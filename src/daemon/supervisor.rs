use std::env;
use std::fs;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::core::ctx::Ctx;
use crate::core::state;
use crate::daemon;

fn poll_interval() -> u64 {
    env::var("AC_POLL_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

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
    crate::core::ctx::echo_external("/bin/kill", &["-0", &pid.to_string()]);
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

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
        crate::core::ctx::echo_external("/bin/kill", &[p.to_string()]);
        std::process::Command::new("/bin/kill")
            .arg(p.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok();
    }
    fs::remove_file(&ctx.supervisor_pidfile).ok();
}

pub fn settle(ctx: &Ctx) -> Result<()> {
    let remaining = state::ac_running_containers(ctx, false);
    if !remaining.is_empty() {
        ctx.dim(&format!(
            "{} ac container(s) still running across all projects - leaving daemon up",
            remaining.len()
        ));
        return Ok(());
    }
    stop(ctx);
    daemon::release(ctx)
}

pub fn run_loop(ctx: &Ctx) -> Result<()> {
    let interval = Duration::from_secs(poll_interval());
    let grace = idle_grace();

    let mut armed = false;
    let mut idle: u32 = 0;

    loop {
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
