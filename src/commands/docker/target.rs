use std::io::IsTerminal;

use anyhow::{anyhow, Result};

use crate::core::ctx::Ctx;
use crate::core::state::Snapshot;
use crate::manifest;

pub(crate) fn wants_tty(asked: bool) -> bool {
    asked && std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

pub(crate) fn auto_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

pub(crate) fn normalize(name: &str) -> String {
    match name.split_once('/') {
        Some((p, s)) if !p.is_empty() && !s.is_empty() => format!("{p}-{s}"),
        _ => name.to_string(),
    }
}

pub(crate) fn resolve(ctx: &Ctx, name: &str) -> Result<String> {
    let want = normalize(name);
    let snap = Snapshot::query_silent(ctx);
    if snap.get(&want).is_some() {
        return Ok(want);
    }

    let projects = manifest::project_names(&ctx.config_dir, &ctx.ac_home);
    if projects.iter().any(|p| p == &want) {
        return Err(anyhow!(
            "'{want}' is a project, not a container\n  try: ac {want} status, or name a service directly (ac logs {want}-<service>)"
        ));
    }

    let mut known: Vec<&str> = snap.items.iter().map(|c| c.id.as_str()).collect();
    known.sort_unstable();
    if known.is_empty() {
        Err(anyhow!("no such container: {want}; none exist right now"))
    } else {
        Err(anyhow!(
            "no such container: {want}\n  containers: {}",
            known.join(" ")
        ))
    }
}

pub(crate) fn resolve_all(ctx: &Ctx, names: &[String]) -> Result<Vec<String>> {
    names.iter().map(|n| resolve(ctx, n)).collect()
}

pub(crate) fn require_targets(all: bool, containers: &[String], verb: &str) -> Result<()> {
    if all || !containers.is_empty() {
        return Ok(());
    }
    Err(anyhow!(
        "{verb} needs a container name, or --all\n  for a whole project stack: ac <project> {verb}"
    ))
}
