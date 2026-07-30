use std::ffi::OsStr;

use clap::{Arg, Command, CommandFactory};
use clap_complete::engine::{ArgValueCandidates, ArgValueCompleter, PathCompleter, ValueCompleter};
use clap_complete::CompletionCandidate;

use crate::cli::{Cli, PROJECT_ACTIONS, RESERVED};
use crate::core::ctx::Ctx;
use crate::manifest;

const SIGNALS: &[&str] = &[
    "KILL", "TERM", "INT", "HUP", "QUIT", "USR1", "USR2", "STOP", "CONT",
];

pub fn completion_command() -> Command {
    let base = Cli::command();

    let Some(template) = base
        .get_subcommands()
        .find(|s| s.get_name() == "project")
        .cloned()
    else {
        return base;
    };

    let globals: Vec<String> = base
        .get_subcommands()
        .filter(|s| s.get_name() != "project")
        .map(|s| s.get_name().to_string())
        .collect();

    let mut cmd = base.mut_subcommand("project", |sub| {
        sub.mut_arg("name", |arg| {
            arg.add(ArgValueCandidates::new(|| candidates(project_names())))
        })
    });

    for name in globals {
        cmd = cmd.mut_subcommand(name.clone(), move |s| with_global_candidates(s, &name));
    }

    for name in project_names() {
        if RESERVED.contains(&name.as_str()) {
            continue;
        }
        let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
        let mut sub = Command::new(leaked).about(format!("Actions for {name}"));
        for action in template.get_subcommands() {
            sub = sub.subcommand(with_candidates(action.clone(), &name));
        }
        for (script, words) in scripts(&name) {
            let s: &'static str = Box::leak(script.into_boxed_str());
            let mut c = Command::new(s).about(format!("Script from {name}.json"));
            if !words.is_empty() {
                c = c.arg(
                    Arg::new("args")
                        .num_args(0..)
                        .allow_hyphen_values(true)
                        .add(ArgValueCandidates::new(move || candidates(words.clone()))),
                );
            }
            sub = sub.subcommand(c);
        }
        cmd = cmd.subcommand(sub);
    }
    cmd
}

fn with_global_candidates(cmd: Command, path: &str) -> Command {
    let nested: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    let owned = path.to_string();
    let mut out = cmd.mut_args(|arg| decorate_global(arg, &owned));
    for name in nested {
        let child = format!("{path} {name}");
        out = out.mut_subcommand(name, move |s| with_global_candidates(s, &child));
    }
    out
}

fn decorate_global(arg: Arg, path: &str) -> Arg {
    match arg.get_id().as_str() {
        "containers" | "container" => {
            arg.add(ArgValueCandidates::new(|| candidates(container_names())))
        }
        "image" | "reference" | "references" | "source" => {
            arg.add(ArgValueCandidates::new(|| candidates(image_refs())))
        }
        "src" | "dst" => arg.add(ArgValueCompleter::new(cp_completer)),
        "signal" => arg.add(ArgValueCandidates::new(|| {
            candidates(SIGNALS.iter().map(|s| s.to_string()).collect())
        })),
        "server" => arg.add(ArgValueCandidates::new(|| candidates(registry_names()))),
        "file" => arg.add(ArgValueCompleter::new(PathCompleter::file())),
        "context" => arg.add(ArgValueCompleter::new(PathCompleter::dir())),
        "input" => arg.add(ArgValueCompleter::new(PathCompleter::file())),
        "output" => arg.add(ArgValueCompleter::new(PathCompleter::any())),
        "target" if path != "tag" && path != "image tag" => {
            arg.add(ArgValueCandidates::new(|| candidates(image_refs())))
        }
        _ => arg,
    }
}

fn cp_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(cur) = current.to_str() else {
        return Vec::new();
    };
    if cur.contains(':') {
        return Vec::new();
    }
    container_names()
        .into_iter()
        .filter(|n| n.starts_with(cur))
        .map(|n| CompletionCandidate::new(format!("{n}:/")))
        .chain(PathCompleter::any().complete(current))
        .collect()
}

fn daemon_backed(args: &[&str]) -> Vec<String> {
    if std::env::var_os("AC_COMPLETE_OFFLINE").is_some() {
        return Vec::new();
    }
    let Ok(ctx) = Ctx::new(false, true, true) else {
        return Vec::new();
    };
    if ctx
        .container(["system", "status"])
        .silent()
        .quiet_ok_timeout(1)
        != Some(true)
    {
        return Vec::new();
    }
    ctx.container(args.to_vec())
        .silent()
        .stdout_timeout(2)
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

fn container_names() -> Vec<String> {
    daemon_backed(&["ls", "-a", "-q"])
}

fn image_refs() -> Vec<String> {
    daemon_backed(&["image", "ls", "-q"])
}

fn registry_names() -> Vec<String> {
    daemon_backed(&["registry", "ls", "-q"])
}

fn with_candidates(action: Command, project: &str) -> Command {
    with_candidates_at(action, project, "")
}

fn with_candidates_at(action: Command, project: &str, parent: &str) -> Command {
    let own = action.get_name().to_string();
    let action_name = if parent.is_empty() {
        own.clone()
    } else {
        format!("{parent} {own}")
    };
    let nested: Vec<String> = action
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();

    let mut out = action.mut_args(|arg| decorate(arg, project, &action_name));
    for name in nested {
        let p = project.to_string();
        let path = action_name.clone();
        out = out.mut_subcommand(name, move |s| with_candidates_at(s, &p, &path));
    }
    out
}

fn decorate(arg: Arg, project: &str, action: &str) -> Arg {
    let p = project.to_string();
    match arg.get_id().as_str() {
        "services" | "service" => arg.add(ArgValueCandidates::new(move || {
            candidates(service_names(&p))
        })),
        "profile" => arg.add(ArgValueCandidates::new(move || {
            candidates(profile_names(&p))
        })),
        "names" => match action {
            "build" | "push" => {
                arg.add(ArgValueCandidates::new(move || candidates(build_names(&p))))
            }
            "volumes rm" | "volumes inspect" => arg.add(ArgValueCandidates::new(move || {
                candidates(volume_names(&p))
            })),
            _ => arg.add(ArgValueCandidates::new(move || {
                let mut v = service_names(&p);
                v.extend(build_names(&p));
                candidates(v)
            })),
        },
        _ => arg,
    }
}

fn candidates(values: Vec<String>) -> Vec<CompletionCandidate> {
    values.into_iter().map(CompletionCandidate::new).collect()
}

fn project_names() -> Vec<String> {
    let Ok(ctx) = Ctx::new(false, true, true) else {
        return Vec::new();
    };
    manifest::project_names(&ctx.config_dir, &ctx.ac_home)
}

fn load(project: &str) -> Option<manifest::Project> {
    let ctx = Ctx::new(false, true, true).ok()?;
    manifest::load_project(&ctx.config_dir, &ctx.ac_home, project).ok()
}

fn service_names(project: &str) -> Vec<String> {
    load(project)
        .map(|p| {
            let mut v = p.manifest.service_names();
            let prefixed: Vec<String> = v.iter().map(|s| format!("{project}-{s}")).collect();
            v.extend(prefixed);
            v
        })
        .unwrap_or_default()
}

fn volume_names(project: &str) -> Vec<String> {
    load(project)
        .map(|p| {
            p.manifest
                .services
                .iter()
                .flat_map(|s| s.volumes.iter())
                .map(|v| v.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn build_names(project: &str) -> Vec<String> {
    load(project)
        .map(|p| p.manifest.build_names())
        .unwrap_or_default()
}

fn profile_names(project: &str) -> Vec<String> {
    load(project)
        .map(|p| p.manifest.profile_names())
        .unwrap_or_default()
}

fn scripts(project: &str) -> Vec<(String, Vec<String>)> {
    load(project)
        .map(|p| {
            p.manifest
                .scripts
                .0
                .iter()
                .filter(|(name, _)| !PROJECT_ACTIONS.contains(&name.as_str()))
                .map(|(name, s)| (name.clone(), s.complete().to_vec()))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub<'a>(cmd: &'a Command, name: &str) -> Option<&'a Command> {
        cmd.get_subcommands().find(|s| s.get_name() == name)
    }

    #[test]
    fn projects_become_top_level_subcommands() {
        let cmd = completion_command();
        assert!(
            sub(&cmd, "shop").is_some(),
            "shop should be completable as a top level subcommand"
        );
        assert!(
            sub(&cmd, "project").is_some(),
            "the explicit project form must survive"
        );
    }

    #[test]
    fn every_action_is_reachable_under_a_project() {
        let cmd = completion_command();
        let template = sub(&cmd, "project").expect("project subcommand");
        let expected: Vec<String> = template
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        assert!(!expected.is_empty(), "project must declare actions");

        let proj = sub(&cmd, "shop").expect("shop subcommand");
        for action in &expected {
            assert!(
                sub(proj, action).is_some(),
                "action {action} missing under the bare project form"
            );
        }
    }

    #[test]
    fn service_args_carry_candidates() {
        let cmd = completion_command();
        let proj = sub(&cmd, "shop").expect("shop");
        for action in ["start", "stop", "down", "restart", "rm", "logs", "exec"] {
            let a = sub(proj, action).unwrap_or_else(|| panic!("{action} missing"));
            let has = a
                .get_arguments()
                .any(|arg| matches!(arg.get_id().as_str(), "services" | "service"));
            assert!(has, "{action} should take a service argument");
        }
    }

    #[test]
    fn build_carries_names_and_profile() {
        let cmd = completion_command();
        let proj = sub(&cmd, "shop").expect("shop");
        let build = sub(proj, "build").expect("build action");
        let ids: Vec<String> = build
            .get_arguments()
            .map(|a| a.get_id().to_string())
            .collect();
        assert!(ids.iter().any(|i| i == "names"), "build takes build names");
        assert!(ids.iter().any(|i| i == "profile"), "build takes a profile");
    }

    #[test]
    fn manifest_scripts_complete_as_project_subcommands() {
        let cmd = completion_command();
        let proj = sub(&cmd, "shop").expect("shop");
        assert!(
            sub(proj, "psql").is_some(),
            "scripts declared in the manifest must complete under the project"
        );
        assert!(sub(proj, "tunnels").is_some());
        assert!(scripts("does-not-exist").is_empty());
    }

    #[test]
    fn script_complete_words_become_argument_candidates() {
        let cmd = completion_command();
        let proj = sub(&cmd, "shop").expect("shop");
        let tunnels = sub(proj, "tunnels").expect("tunnels script");
        assert!(
            tunnels.get_arguments().any(|a| a.get_id() == "args"),
            "a script with `complete` words must take an argument that offers them"
        );
        let psql = sub(proj, "psql").expect("psql script");
        assert!(
            psql.get_arguments().next().is_none(),
            "a plain string script declares no completion words, so no argument"
        );
        let declared = scripts("shop");
        let tunnel_words = &declared.iter().find(|(n, _)| n == "tunnels").unwrap().1;
        assert!(
            tunnel_words.iter().any(|w| w == "status"),
            "{tunnel_words:?}"
        );
    }

    #[test]
    fn container_name_form_is_offered() {
        let names = service_names("shop");
        assert!(names.iter().any(|n| n == "postgres"));
        assert!(
            names.iter().any(|n| n == "shop-postgres"),
            "the container name form printed by ls must complete too"
        );
    }

    #[test]
    fn global_container_verbs_take_a_container_argument() {
        let cmd = completion_command();
        for verb in [
            "start", "stop", "restart", "rm", "exec", "sh", "logs", "inspect", "kill", "export",
            "top", "port",
        ] {
            let c = sub(&cmd, verb).unwrap_or_else(|| panic!("{verb} missing"));
            let has = c
                .get_arguments()
                .any(|a| matches!(a.get_id().as_str(), "containers" | "container"));
            assert!(has, "{verb} should take a container argument");
        }
    }

    #[test]
    fn offline_completion_never_touches_the_daemon() {
        std::env::set_var("AC_COMPLETE_OFFLINE", "1");
        assert!(container_names().is_empty());
        assert!(image_refs().is_empty());
        std::env::remove_var("AC_COMPLETE_OFFLINE");
    }

    #[test]
    fn unknown_project_yields_no_candidates_without_panicking() {
        assert!(service_names("does-not-exist").is_empty());
        assert!(build_names("does-not-exist").is_empty());
        assert!(profile_names("does-not-exist").is_empty());
    }
}
