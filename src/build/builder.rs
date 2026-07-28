use std::thread;
use std::time::Duration;

use crate::core::ctx::Ctx;

#[derive(serde::Deserialize)]
struct BuilderResources {
    cpus: Option<u32>,
    #[serde(rename = "memoryInBytes")]
    memory_in_bytes: Option<u64>,
}

#[derive(serde::Deserialize)]
struct BuilderConfiguration {
    resources: Option<BuilderResources>,
}

#[derive(serde::Deserialize)]
struct BuilderEntry {
    configuration: Option<BuilderConfiguration>,
}

pub fn memory_to_mb(s: &str) -> Option<u64> {
    let t = s.trim().to_ascii_lowercase();
    let (num, mult) = if let Some(n) = t.strip_suffix("gb") {
        (n, 1024)
    } else if let Some(n) = t.strip_suffix('g') {
        (n, 1024)
    } else if let Some(n) = t.strip_suffix("mb") {
        (n, 1)
    } else if let Some(n) = t.strip_suffix('m') {
        (n, 1)
    } else {
        (t.as_str(), 1)
    };
    num.trim().parse::<u64>().ok().map(|n| n * mult)
}

pub fn ensure_builder(ctx: &Ctx, want_cpus: Option<u32>, want_mem: Option<&str>) {
    if want_cpus.is_none() && want_mem.is_none() {
        return;
    }

    let Ok(text) = ctx
        .container(["builder", "status", "--format", "json"])
        .stdout()
    else {
        return;
    };
    let Ok(entries) = serde_json::from_str::<Vec<BuilderEntry>>(&text) else {
        return;
    };
    let Some(res) = entries
        .first()
        .and_then(|e| e.configuration.as_ref())
        .and_then(|c| c.resources.as_ref())
    else {
        return;
    };

    let cur_cpus = res.cpus;
    let cur_mb = res.memory_in_bytes.map(|b| b / (1024 * 1024));
    let want_mb = want_mem.and_then(memory_to_mb);

    let cpus_ok = want_cpus.is_none() || want_cpus == cur_cpus;
    let mem_ok = want_mb.is_none() || want_mb == cur_mb;
    if cpus_ok && mem_ok {
        return;
    }

    let show = |v: Option<u64>| v.map(|x| x.to_string()).unwrap_or_else(|| "?".into());
    ctx.warn(&format!(
        "resizing buildkit builder from {} cpus / {} MB to {} cpus / {} MB. \
The builder only reads these values when it is created, so it is being stopped \
first and its layer cache is discarded.",
        show(cur_cpus.map(u64::from)),
        show(cur_mb),
        want_cpus
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unchanged".into()),
        want_mb
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unchanged".into()),
    ));
    ctx.container(["builder", "stop"]).quiet_ok();
    thread::sleep(Duration::from_secs(2));
}
