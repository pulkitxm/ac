use std::process::ExitStatus;

use anyhow::{anyhow, Result};

use crate::core::ctx::Ctx;

pub fn exit_ok(status: ExitStatus) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("command exited {status}"))
    }
}

pub fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

pub fn short_ref(full: &str) -> (String, String) {
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

pub fn fmt_size(bytes: u64) -> String {
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

pub fn fmt_date(iso: &str) -> String {
    let mut out: String = iso.chars().take(19).collect();
    if let Some(i) = out.find('T') {
        out.replace_range(i..i + 1, " ");
    }
    out
}

pub fn print_pretty_json(ctx: &Ctx, args: Vec<String>) -> Result<()> {
    let text = ctx.container(args).stdout()?;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(_) => print!("{text}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ref_strips_the_docker_hub_prefixes() {
        assert_eq!(
            short_ref("docker.io/library/redis:7-alpine"),
            ("redis".into(), "7-alpine".into())
        );
        assert_eq!(
            short_ref("ghcr.io/owner/app:1.2"),
            ("ghcr.io/owner/app".into(), "1.2".into())
        );
    }

    #[test]
    fn a_reference_with_no_tag_is_latest() {
        assert_eq!(short_ref("alpine"), ("alpine".into(), "latest".into()));
    }

    #[test]
    fn a_registry_port_is_not_mistaken_for_a_tag() {
        assert_eq!(
            short_ref("localhost:5000/app"),
            ("localhost:5000/app".into(), "latest".into())
        );
    }

    #[test]
    fn sizes_scale_by_unit() {
        assert_eq!(fmt_size(512), "512 B");
        assert_eq!(fmt_size(2_500), "2.5 kB");
        assert_eq!(fmt_size(1_500_000), "1.5 MB");
        assert_eq!(fmt_size(3_000_000_000), "3.00 GB");
    }
}
