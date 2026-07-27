use clap::{Arg, Command, CommandFactory};
use clap_complete::engine::ArgValueCandidates;
use clap_complete::CompletionCandidate;

use crate::cli::Cli;
use crate::ctx::Ctx;
use crate::manifest;

pub fn completion_command() -> Command {
    let base = Cli::command();

    let Some(template) = base
        .get_subcommands()
        .find(|s| s.get_name() == "project")
        .cloned()
    else {
        return base;
    };

    let names = project_names();
    if names.is_empty() {
        return base;
    }

    let mut cmd = base;
    for name in names {
        let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
        let mut sub = Command::new(leaked).about(format!("Actions for {name}"));
        for action in template.get_subcommands() {
            sub = sub.subcommand(with_candidates(action.clone(), &name));
        }
        cmd = cmd.subcommand(sub);
    }
    cmd
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
        "services" | "service" => {
            arg.add(ArgValueCandidates::new(move || candidates(service_names(&p))))
        }
        "profile" => arg.add(ArgValueCandidates::new(move || {
            candidates(profile_names(&p))
        })),
        "names" => match action {
            "build" => arg.add(ArgValueCandidates::new(move || candidates(build_names(&p)))),
            "volumes rm" | "volumes inspect" => {
                arg.add(ArgValueCandidates::new(move || candidates(volume_names(&p))))
            }
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
            sub(&cmd, "noveum").is_some(),
            "noveum should be completable as a top level subcommand"
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

        let proj = sub(&cmd, "noveum").expect("noveum subcommand");
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
        let proj = sub(&cmd, "noveum").expect("noveum");
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
        let proj = sub(&cmd, "noveum").expect("noveum");
        let build = sub(proj, "build").expect("build action");
        let ids: Vec<String> = build
            .get_arguments()
            .map(|a| a.get_id().to_string())
            .collect();
        assert!(ids.iter().any(|i| i == "names"), "build takes build names");
        assert!(ids.iter().any(|i| i == "profile"), "build takes a profile");
    }

    #[test]
    fn container_name_form_is_offered() {
        let names = service_names("noveum");
        assert!(names.iter().any(|n| n == "postgres"));
        assert!(
            names.iter().any(|n| n == "noveum-postgres"),
            "the container name form printed by ls must complete too"
        );
    }

    #[test]
    fn unknown_project_yields_no_candidates_without_panicking() {
        assert!(service_names("does-not-exist").is_empty());
        assert!(build_names("does-not-exist").is_empty());
        assert!(profile_names("does-not-exist").is_empty());
    }
}
