use crate::cli::RunOpts;
use crate::core::ctx::Ctx;

pub const MANAGED_LABEL: &str = "ac.managed=1";

pub(crate) fn run_opts_argv(opts: &RunOpts, progress: bool) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();

    let mut flag = |name: &str, val: &Option<String>| {
        if let Some(v) = val {
            a.push(name.to_string());
            a.push(v.clone());
        }
    };
    flag("--name", &opts.name);
    flag("--shm-size", &opts.shm_size);
    flag("--user", &opts.user);
    flag("--uid", &opts.uid);
    flag("--gid", &opts.gid);
    flag("--workdir", &opts.workdir);
    flag("--entrypoint", &opts.entrypoint);
    flag("--network", &opts.network);
    flag("--platform", &opts.platform);
    flag("--arch", &opts.arch);
    flag("--os", &opts.os);
    flag("--init-image", &opts.init_image);
    flag("--kernel", &opts.kernel);
    flag("--runtime", &opts.runtime);
    flag("--dns-domain", &opts.dns_domain);
    flag("--cidfile", &opts.cidfile);
    flag("--scheme", &opts.scheme);
    if progress {
        flag("--progress", &opts.progress);
    }

    if let Some(c) = opts.cpus {
        a.push("--cpus".into());
        a.push(c.to_string());
    }
    if let Some(m) = &opts.memory {
        a.push("--memory".into());
        a.push(m.clone());
    }
    if let Some(n) = opts.max_concurrent_downloads {
        a.push("--max-concurrent-downloads".into());
        a.push(n.to_string());
    }

    let mut repeat = |name: &str, vals: &Vec<String>| {
        for v in vals {
            a.push(name.to_string());
            a.push(v.clone());
        }
    };
    repeat("--env", &opts.env);
    repeat("--env-file", &opts.env_file);
    repeat("--publish", &opts.publish);
    repeat("--publish-socket", &opts.publish_socket);
    repeat("--volume", &opts.volume);
    repeat("--mount", &opts.mount);
    repeat("--tmpfs", &opts.tmpfs);
    repeat("--label", &opts.label);
    repeat("--ulimit", &opts.ulimit);
    repeat("--cap-add", &opts.cap_add);
    repeat("--cap-drop", &opts.cap_drop);
    repeat("--dns", &opts.dns);
    repeat("--dns-option", &opts.dns_option);
    repeat("--dns-search", &opts.dns_search);

    for (on, name) in [
        (opts.read_only, "--read-only"),
        (opts.init, "--init"),
        (opts.no_dns, "--no-dns"),
        (opts.ssh, "--ssh"),
        (opts.rosetta, "--rosetta"),
        (opts.virtualization, "--virtualization"),
    ] {
        if on {
            a.push(name.to_string());
        }
    }

    a.push("--label".into());
    a.push(MANAGED_LABEL.to_string());
    a
}

pub(crate) fn published_urls(ctx: &Ctx, cname: &str) -> Vec<String> {
    let Ok(text) = ctx.container(["inspect", cname]).silent().stdout() else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(ports) = v
        .get(0)
        .and_then(|c| c.get("configuration"))
        .and_then(|c| c.get("publishedPorts"))
        .and_then(|p| p.as_array())
    else {
        return Vec::new();
    };
    ports
        .iter()
        .filter(|p| p.get("proto").and_then(|x| x.as_str()) != Some("udp"))
        .filter_map(|p| p.get("hostPort").and_then(|x| x.as_u64()))
        .map(|port| format!("http://localhost:{port}"))
        .collect()
}

pub(crate) fn report_urls(ctx: &Ctx, cname: &str) {
    for url in published_urls(ctx, cname) {
        ctx.dim(&format!("  {url}"));
    }
}
