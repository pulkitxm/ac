#![recursion_limit = "512"]

mod build;
mod cli;
mod commands;
mod completions;
mod core;
mod daemon;
mod dispatch;
mod manifest;
mod progress;

use std::process::ExitCode;

use anyhow::{anyhow, Result};
use clap::Parser;

use crate::cli::{Cli, RESERVED};
use crate::core::ctx::Ctx;
use crate::core::style;
use crate::dispatch::run;

fn main() -> ExitCode {
    clap_complete::CompleteEnv::with_factory(completions::completion_command).complete();

    let argv: Vec<String> = std::env::args().collect();
    let rewritten = match rewrite_argv(&argv) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} {e}", style::red("err"));
            return ExitCode::FAILURE;
        }
    };

    let cli = Cli::parse_from(&rewritten);

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", style::red("err"));
            ExitCode::FAILURE
        }
    }
}

fn rewrite_argv(argv: &[String]) -> Result<Vec<String>> {
    let prog = argv.first().cloned().unwrap_or_else(|| "ac".into());
    let rest = &argv[1..];

    let is_global_flag = |s: &str| matches!(s, "--json" | "--quiet" | "--no-color");

    let rest = map_format_json(rest)?;
    let rest = &rest[..];

    let mut lead: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() && is_global_flag(&rest[i]) {
        lead.push(rest[i].clone());
        i += 1;
    }

    if rest[i..].is_empty() {
        return Ok(vec![prog]);
    }

    let mut out = vec![prog];
    out.extend(lead.clone());

    if i < rest.len() {
        let tok = rest[i].as_str();
        let (name, skip) = if tok == "-p" || tok == "--project" {
            match rest.get(i + 1) {
                Some(n) => (Some(n.clone()), 2),
                None => return Err(anyhow!("-p requires a project name")),
            }
        } else if let Some(n) = tok.strip_prefix("--project=") {
            (Some(n.to_string()), 1)
        } else {
            (None, 0)
        };
        if let Some(name) = name {
            out.push("project".into());
            out.push(name);
            let tail = &rest[i + skip..];
            if tail.is_empty() {
                out.push("status".into());
            } else {
                out.extend(tail.iter().cloned());
            }
            return Ok(out);
        }
    }

    let Some(first) = rest.get(i) else {
        out.extend(rest[i..].iter().cloned());
        return Ok(out);
    };

    if first.starts_with('-') || RESERVED.contains(&first.as_str()) {
        out.extend(rest[i..].iter().cloned());
        return Ok(out);
    }

    let probe = Ctx::new(false, true, true)?;
    let known = manifest::project_names(&probe.config_dir, &probe.ac_home);
    if !known.iter().any(|p| p == first) {
        let commands: Vec<&str> = RESERVED
            .iter()
            .copied()
            .filter(|c| !c.starts_with("__"))
            .collect();
        let hint = if cli::PROJECT_ACTIONS.contains(&first.as_str()) {
            format!("\n  '{first}' is a project action: try `ac <project> {first} ...`")
        } else {
            String::new()
        };
        return Err(anyhow!(
            "unknown project or command: {first}{hint}\n  projects: {}\n  commands: {}\n  try: ac --help",
            if known.is_empty() {
                "(none)".to_string()
            } else {
                known.join(" ")
            },
            commands.join(" ")
        ));
    }

    out.push("project".into());
    out.push(first.clone());
    let tail = &rest[i + 1..];
    if tail.is_empty() {
        out.push("status".into());
    } else {
        out.extend(tail.iter().cloned());
    }
    Ok(out)
}

fn map_format_json(rest: &[String]) -> Result<Vec<String>> {
    let takes_user_argv = |s: &str| matches!(s, "exec" | "run" | "cp" | "copy" | "machine");

    let mut out: Vec<String> = Vec::with_capacity(rest.len());
    let mut i = 0;
    let mut word = 0usize;
    let mut first_word: Option<String> = None;
    let mut passthrough_zone = false;
    while i < rest.len() {
        let tok = rest[i].as_str();
        if !tok.starts_with('-') {
            let is_verb = match word {
                0 => takes_user_argv(tok),
                1 => {
                    takes_user_argv(tok)
                        && first_word
                            .as_deref()
                            .is_some_and(|w| !RESERVED.contains(&w))
                }
                _ => false,
            };
            if is_verb {
                passthrough_zone = true;
            }
            if word == 0 {
                first_word = Some(tok.to_string());
            }
            word += 1;
        }
        if !passthrough_zone {
            if tok == "--format" {
                match rest.get(i + 1).map(|s| s.as_str()) {
                    Some("json") => {
                        out.push("--json".into());
                        i += 2;
                        continue;
                    }
                    other => {
                        return Err(anyhow!(
                            "--format {} is not supported; ac emits JSON only, use --json",
                            other.unwrap_or("")
                        ));
                    }
                }
            }
            if let Some(v) = tok.strip_prefix("--format=") {
                if v == "json" {
                    out.push("--json".into());
                    i += 1;
                    continue;
                }
                return Err(anyhow!(
                    "--format={v} is not supported; ac emits JSON only, use --json"
                ));
            }
        }
        out.push(rest[i].clone());
        i += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn global_flags_before_a_project_are_kept_once() {
        let argv: Vec<String> = ["ac", "--json", "shop", "ls"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = rewrite_argv(&argv).expect("rewrite");
        assert_eq!(out, vec!["ac", "--json", "project", "shop", "ls"]);
    }

    #[test]
    fn format_json_maps_to_json_outside_passthrough_zones() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            map_format_json(&a(&["ps", "--format", "json"])).unwrap(),
            a(&["ps", "--json"])
        );
        assert_eq!(
            map_format_json(&a(&["image", "ls", "--format=json"])).unwrap(),
            a(&["image", "ls", "--json"])
        );
        assert_eq!(
            map_format_json(&a(&["demo", "exec", "web", "cmd", "--format", "json"])).unwrap(),
            a(&["demo", "exec", "web", "cmd", "--format", "json"])
        );
        let err = map_format_json(&a(&["ps", "--format", "table"])).unwrap_err();
        assert!(err.to_string().contains("--json"), "{err}");
    }

    #[test]
    fn a_passthrough_verb_used_as_a_container_name_is_not_a_passthrough_zone() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            map_format_json(&a(&["stop", "run", "--format", "json"])).unwrap(),
            a(&["stop", "run", "--json"])
        );
        assert_eq!(
            map_format_json(&a(&["logs", "cp", "--format", "json"])).unwrap(),
            a(&["logs", "cp", "--json"])
        );
        assert_eq!(
            map_format_json(&a(&["exec", "web", "cmd", "--format", "json"])).unwrap(),
            a(&["exec", "web", "cmd", "--format", "json"])
        );
        assert_eq!(
            map_format_json(&a(&["--json", "run", "img", "--format", "json"])).unwrap(),
            a(&["--json", "run", "img", "--format", "json"])
        );
    }

    #[test]
    fn every_top_level_command_is_reserved() {
        let cmd = Cli::command();
        let missing: Vec<String> = cmd
            .get_subcommands()
            .flat_map(|s| {
                std::iter::once(s.get_name().to_string())
                    .chain(s.get_all_aliases().map(|a| a.to_string()))
            })
            .filter(|n| !RESERVED.contains(&n.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "these subcommands parse as project names because RESERVED is stale: {missing:?}"
        );
    }

    #[test]
    fn every_project_action_is_listed_in_project_actions() {
        let cmd = Cli::command();
        let project = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "project")
            .expect("project subcommand");
        let missing: Vec<String> = project
            .get_subcommands()
            .flat_map(|s| {
                std::iter::once(s.get_name().to_string())
                    .chain(s.get_all_aliases().map(|a| a.to_string()))
            })
            .filter(|n| !cli::PROJECT_ACTIONS.contains(&n.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "a manifest script named after these would be silently shadowed, so manifest \
validation must know them; add to PROJECT_ACTIONS: {missing:?}"
        );
    }

    #[test]
    fn bare_invocations_collapse_to_help() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(rewrite_argv(&a(&["ac", "--json"])).unwrap(), a(&["ac"]));
        assert_eq!(
            rewrite_argv(&a(&["ac", "--json", "--quiet"])).unwrap(),
            a(&["ac"])
        );
    }

    #[test]
    fn argv_rewrite_leaves_reserved_words_alone() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            rewrite_argv(&a(&["ac", "status"])).unwrap(),
            a(&["ac", "status"])
        );
        assert_eq!(
            rewrite_argv(&a(&["ac", "--json", "daemon", "status"])).unwrap(),
            a(&["ac", "--json", "daemon", "status"])
        );
        assert_eq!(
            rewrite_argv(&a(&["ac", "-p", "status", "start"])).unwrap(),
            a(&["ac", "project", "status", "start"])
        );
        assert_eq!(
            rewrite_argv(&a(&["ac", "-p", "weird"])).unwrap(),
            a(&["ac", "project", "weird", "status"])
        );
        assert_eq!(
            rewrite_argv(&a(&["ac", "--help"])).unwrap(),
            a(&["ac", "--help"])
        );
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
