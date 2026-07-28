use crate::cli::RunOpts;
use crate::commands::docker::opts::run_opts_argv;
use crate::commands::docker::target::normalize;

#[test]
fn slash_form_becomes_dashed_container_name() {
    assert_eq!(normalize("shop/redis"), "shop-redis");
    assert_eq!(normalize("shop-redis"), "shop-redis");
    assert_eq!(normalize("web"), "web");
    assert_eq!(normalize("/leading"), "/leading");
}

#[test]
fn run_argv_always_carries_the_managed_label() {
    let opts = RunOpts::default();
    let argv = run_opts_argv(&opts, true);
    let joined = argv.join(" ");
    assert!(joined.contains("--label ac.managed=1"), "{joined}");
}

#[test]
fn create_never_gets_progress_because_container_create_rejects_it() {
    let opts = RunOpts {
        progress: Some("plain".into()),
        ..RunOpts::default()
    };
    assert!(run_opts_argv(&opts, true).join(" ").contains("--progress"));
    assert!(!run_opts_argv(&opts, false).join(" ").contains("--progress"));
}

#[test]
fn run_argv_maps_repeatable_and_scalar_flags() {
    let opts = RunOpts {
        name: Some("web".into()),
        publish: vec!["3000:3000".into(), "9229:9229".into()],
        env: vec!["A=1".into(), "B=2".into()],
        cpus: Some(4),
        memory: Some("2g".into()),
        read_only: true,
        ..RunOpts::default()
    };
    let joined = run_opts_argv(&opts, true).join(" ");
    assert!(joined.contains("--name web"), "{joined}");
    assert!(joined.contains("--publish 3000:3000"), "{joined}");
    assert!(joined.contains("--publish 9229:9229"), "{joined}");
    assert!(joined.contains("--env A=1"), "{joined}");
    assert!(joined.contains("--cpus 4"), "{joined}");
    assert!(joined.contains("--memory 2g"), "{joined}");
    assert!(joined.contains("--read-only"), "{joined}");
}

#[test]
fn unset_flags_are_absent() {
    let joined = run_opts_argv(&RunOpts::default(), true).join(" ");
    assert!(!joined.contains("--name"), "{joined}");
    assert!(!joined.contains("--cpus"), "{joined}");
    assert!(!joined.contains("--rosetta"), "{joined}");
}
