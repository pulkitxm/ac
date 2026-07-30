use std::process::ExitStatus;

use anyhow::{anyhow, Result};

use crate::core::ctx::{shell_quote, Ctx};
use crate::core::util;
use crate::manifest::Project;

pub fn compose(script: &str, args: &[String]) -> String {
    let mut cmd = script.trim().to_string();
    for a in args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(a));
    }
    cmd
}

pub fn run(ctx: &Ctx, proj: &Project, argv: &[String]) -> Result<ExitStatus> {
    let name = argv.first().map(String::as_str).unwrap_or_default();
    let Some(script) = proj.manifest.script(name) else {
        let scripts = proj.manifest.script_names();
        let have = if scripts.is_empty() {
            "(none declared; see: ac schema)".to_string()
        } else {
            scripts.join(" ")
        };
        return Err(anyhow!(
            "no action or script named '{name}' in project '{}'\n  scripts: {have}\n  actions: ac {} --help",
            proj.name,
            proj.name
        ));
    };

    let cmd = compose(script.run(), &argv[1..]);
    let mut envs: Vec<(String, String)> = vec![
        ("AC_PROJECT".into(), proj.name.clone()),
        (
            "AC_PROJECT_FILE".into(),
            proj.file.to_string_lossy().into_owned(),
        ),
    ];
    if let Some(root) = &proj.manifest.root {
        envs.push(("AC_PROJECT_ROOT".into(), root.clone()));
    }
    ctx.exec("sh", ["-c", cmd.as_str()]).envs(envs).status()
}

pub fn list(ctx: &Ctx, proj: &Project) -> Result<()> {
    let scripts = &proj.manifest.scripts.0;
    if ctx.json {
        let mut map = serde_json::Map::new();
        for (name, body) in scripts {
            map.insert(name.clone(), serde_json::to_value(body)?);
        }
        return ctx.emit_json(&serde_json::Value::Object(map));
    }
    if scripts.is_empty() {
        ctx.dim(&format!(
            "project '{}' declares no scripts; add a `scripts` map to {} (see: ac schema)",
            proj.name,
            proj.file.display()
        ));
        return Ok(());
    }
    let mut table = util::Table::new(&["NAME", "RUNS"]);
    for (name, body) in scripts {
        table.row([name.as_str(), body.run()]);
    }
    table.print(ctx);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_arguments_are_appended_shell_quoted() {
        assert_eq!(compose("echo hi", &[]), "echo hi");
        assert_eq!(compose("~/x.sh", &["status".into()]), "~/x.sh status");
        assert_eq!(
            compose(" ./t.sh ", &["logs".into(), "-f".into()]),
            "./t.sh logs -f"
        );
        assert_eq!(
            compose("./t.sh", &["a b".into(), "it's".into()]),
            r#"./t.sh 'a b' 'it'\''s'"#
        );
    }
}
