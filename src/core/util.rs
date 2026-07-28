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

pub struct Table {
    headers: Vec<String>,
    right: Vec<bool>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: &[&str]) -> Self {
        Table {
            headers: headers.iter().map(|h| (*h).to_string()).collect(),
            right: vec![false; headers.len()],
            rows: Vec::new(),
        }
    }

    pub fn right(mut self, columns: &[usize]) -> Self {
        for &c in columns {
            if let Some(slot) = self.right.get_mut(c) {
                *slot = true;
            }
        }
        self
    }

    pub fn row<I, S>(&mut self, cells: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rows.push(cells.into_iter().map(Into::into).collect());
    }

    pub fn lines(&self) -> Vec<String> {
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.chars().count()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate().take(widths.len()) {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
        let mut out = vec![crate::core::style::bold(
            &self.render(&self.headers, &widths),
        )];
        out.extend(self.rows.iter().map(|r| self.render(r, &widths)));
        out
    }

    pub fn print(&self, ctx: &Ctx) {
        for line in self.lines() {
            ctx.log(&line);
        }
    }

    fn render(&self, cells: &[String], widths: &[usize]) -> String {
        let mut out = String::new();
        for (i, width) in widths.iter().enumerate() {
            let cell = cells.get(i).map(String::as_str).unwrap_or("");
            let pad = width.saturating_sub(cell.chars().count());
            if self.right[i] {
                out.push_str(&" ".repeat(pad));
                out.push_str(cell);
            } else {
                out.push_str(cell);
                out.push_str(&" ".repeat(pad));
            }
            if i + 1 < widths.len() {
                out.push_str("  ");
            }
        }
        while out.ends_with(' ') {
            out.pop();
        }
        out
    }
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
    fn columns_widen_to_the_longest_cell() {
        let mut t = Table::new(&["NAME", "STATE"]);
        t.row(["a-very-long-container-name", "running"]);
        t.row(["short", "stopped"]);
        let lines = t.lines();
        assert_eq!(lines[1], "a-very-long-container-name  running");
        assert_eq!(lines[2], "short                       stopped");
    }

    #[test]
    fn the_header_is_never_narrower_than_its_own_label() {
        let mut t = Table::new(&["CONTAINER", "IP"]);
        t.row(["web", "-"]);
        assert_eq!(t.lines()[1], "web        -");
    }

    #[test]
    fn a_right_aligned_column_pads_on_the_left() {
        let mut t = Table::new(&["NAME", "SIZE", "TAGS"]).right(&[1]);
        t.row(["api", "1.5 MB", "dev"]);
        t.row(["worker", "12 B", "dev"]);
        let lines = t.lines();
        assert_eq!(lines[1], "api     1.5 MB  dev");
        assert_eq!(lines[2], "worker    12 B  dev");
    }

    #[test]
    fn missing_trailing_cells_do_not_panic() {
        let mut t = Table::new(&["A", "B", "C"]);
        t.row(["only"]);
        assert_eq!(t.lines()[1], "only");
    }

    #[test]
    fn sizes_scale_by_unit() {
        assert_eq!(fmt_size(512), "512 B");
        assert_eq!(fmt_size(2_500), "2.5 kB");
        assert_eq!(fmt_size(1_500_000), "1.5 MB");
        assert_eq!(fmt_size(3_000_000_000), "3.00 GB");
    }
}
