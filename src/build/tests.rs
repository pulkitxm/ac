use crate::build::builder::memory_to_mb;
use crate::build::vars::{hook_env, interpolate, vars_for, Vars};
use crate::manifest;

#[test]
fn build_interpolation_covers_every_placeholder() {
    let v = Vars {
        profile: "dev".into(),
        account: "123".into(),
        tag: "t".into(),
        region: "us-east-1".into(),
        registry: "r/".into(),
        version: "1.2.3".into(),
        git_sha: "abc".into(),
        git_short_sha: "ab".into(),
        git_branch: "main".into(),
        git_dirty_suffix: "-local-1".into(),
        timestamp: "20260101000000".into(),
        images: [("web".to_string(), vec!["r/web:t".to_string()])]
            .into_iter()
            .collect(),
    };
    let s = "{{profile}}|{{account}}|{{tag}}|{{region}}|{{registry}}|{{version}}|\
             {{git.sha}}|{{git.shortSha}}|{{git.branch}}|{{git.dirtySuffix}}|{{timestamp}}|\
             {{image.web}}";
    assert_eq!(
        interpolate(s, &v),
        "dev|123|t|us-east-1|r/|1.2.3|abc|ab|main|-local-1|20260101000000|r/web:t"
    );
    assert!(!interpolate(s, &v).contains("{{"));
}

#[test]
fn rollout_hook_env_carries_image_refs() {
    let dir = std::env::temp_dir();
    let text = r#"{
        "name": "demo",
        "profiles": { "prod": { "push": true, "tag": "latest", "registry": "reg/",
                      "rollout": { "run": [["./deploy.sh"]] } } },
        "builds": [
          { "name": "web", "dockerfile": "Dockerfile", "image": "{{registry}}web",
            "tags": ["{{tag}}"] },
          { "name": "api-workers", "dockerfile": "Dockerfile", "image": "{{registry}}wrk",
            "tags": ["{{tag}}", "pinned"] }
        ]
    }"#;
    let m: manifest::Manifest = serde_json::from_str(text).expect("manifest parses");
    let proj = manifest::Project {
        name: "demo".into(),
        file: dir.join("demo.json"),
        manifest: m,
        raw: text.to_string(),
    };
    let v = vars_for(&proj, "prod", &dir);
    let env = hook_env(&proj, &v, &dir, &["web".to_string()]);
    let get = |k: &str| {
        env.iter()
            .find(|(n, _)| n == k)
            .map(|(_, x)| x.clone())
            .unwrap_or_default()
    };

    assert_eq!(get("AC_IMAGE_WEB"), "reg/web:latest");
    assert_eq!(get("AC_IMAGE_API_WORKERS"), "reg/wrk:latest");
    assert_eq!(
        get("AC_IMAGES_API_WORKERS"),
        "reg/wrk:latest reg/wrk:pinned"
    );
    assert_eq!(get("AC_IMAGES"), "reg/web:latest");
    assert_eq!(get("AC_BUILDS"), "web");
    assert_eq!(get("AC_PROFILE"), "prod");
}

#[test]
fn rollout_is_rejected_for_a_profile_that_declares_none() {
    let text = r#"{
        "name": "demo",
        "profiles": { "local": { "push": false } },
        "builds": [{ "name": "web", "dockerfile": "Dockerfile", "image": "web",
                     "tags": ["dev"] }]
    }"#;
    let m: manifest::Manifest = serde_json::from_str(text).expect("manifest parses");
    assert!(m.profiles.get("local").expect("profile").rollout.is_none());
}

#[test]
fn a_profile_rollout_block_parses_and_rejects_typos() {
    let ok = r#"{ "push": true, "rollout": {
        "description": "ship it", "auto": true,
        "preflight": [["./pre.sh"]], "run": [["./go.sh", "{{profile}}"]] } }"#;
    let p: manifest::Profile = serde_json::from_str(ok).expect("rollout parses");
    let r = p.rollout.expect("rollout present");
    assert!(r.auto);
    assert_eq!(r.run[0], vec!["./go.sh", "{{profile}}"]);

    let typo = r#"{ "push": true, "rollout": { "runn": [["./go.sh"]] } }"#;
    let err = serde_json::from_str::<manifest::Profile>(typo)
        .expect_err("unknown field must be rejected");
    assert!(err.to_string().contains("runn"), "got: {err}");
}

#[test]
fn memory_parsing() {
    assert_eq!(memory_to_mb("8g"), Some(8192));
    assert_eq!(memory_to_mb("8G"), Some(8192));
    assert_eq!(memory_to_mb("8gb"), Some(8192));
    assert_eq!(memory_to_mb("512m"), Some(512));
    assert_eq!(memory_to_mb("512"), Some(512));
    assert_eq!(memory_to_mb("nope"), None);
}
