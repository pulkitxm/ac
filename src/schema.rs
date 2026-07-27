//! JSON Schema for a project manifest, emitted by `ac schema`.
//!
//! Kept in lockstep with `manifest.rs` by the round-trip test at the bottom of
//! this file, which checks that every field named in the schema also exists on
//! the typed manifest.

use serde_json::{json, Value};

pub fn manifest_schema() -> Value {
    json!({
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "$id": "https://github.com/pulkitxm/ac/manifest.schema.json",
      "title": "ac project manifest",
      "description":
        "A project ac can run. Drop this file into ~/.config/ac/projects/<name>.json \
         (user, wins) or <repo>/projects/<name>.json (bundled). Containers are named \
         <project>-<service> and named volumes <project>-<volume>.",
      "type": "object",
      "additionalProperties": false,
      "required": ["name"],
      "properties": {
        "name": {
          "type": "string",
          "description": "Project name. Conventionally matches the file name."
        },
        "description": {
          "type": "string",
          "description": "One line shown by `ac ls` and `ac status`."
        },
        "root": {
          "type": "string",
          "description":
            "Default directory builds run from. Overridden by --root, $AC_ROOT, and by \
             the git worktree containing $PWD when that tree holds the first declared \
             dockerfile."
        },
        "region": {
          "type": "string",
          "default": "us-east-1",
          "description": "Default value of the {{region}} placeholder."
        },
        "builder": {
          "type": "object",
          "additionalProperties": false,
          "description":
            "Sizing for the shared buildkit builder container. These values are only \
             read when the builder is CREATED, so changing them makes ac stop the \
             builder first, discarding its layer cache.",
          "properties": {
            "cpus": { "type": "integer", "minimum": 1 },
            "memory": { "type": "string", "examples": ["8g", "4096m"] }
          }
        },
        "profiles": {
          "type": "object",
          "description":
            "Named build targets, selected with --profile. `local` is the default. \
             Profile values override build entries and project defaults.",
          "additionalProperties": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "platform": { "type": "string", "examples": ["linux/arm64", "linux/amd64"] },
              "push":     { "type": "boolean", "default": false },
              "tag":      { "type": "string", "description": "Value of {{tag}}." },
              "account":  { "type": "string", "description": "Value of {{account}}." },
              "region":   { "type": "string", "description": "Value of {{region}}, overriding the project default." },
              "registry": {
                "type": "string",
                "description":
                  "Value of {{registry}}: a host plus trailing slash, or empty for purely \
                   local profiles. May itself contain {{account}} and {{region}}.",
                "examples": ["", "{{account}}.dkr.ecr.{{region}}.amazonaws.com/"]
              }
            }
          }
        },
        "registries": {
          "type": "array",
          "description":
            "Private registries to authenticate against before pulling or pushing. ac \
             only contacts a registry that an image actually comes from.",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["server", "passwordCmd"],
            "properties": {
              "server":   { "type": "string", "description": "Registry host. Supports {{...}}." },
              "username": { "type": "string", "default": "AWS" },
              "passwordCmd": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "description":
                  "argv executed and piped to `container registry login --password-stdin`. \
                   Credentials are never stored in the manifest.",
                "examples": [["aws", "ecr", "get-login-password", "--region", "{{region}}"]]
              }
            }
          }
        },
        "builds": {
          "type": "array",
          "description": "Image builds, run by `ac <project> build [name...]`.",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["name", "dockerfile", "image"],
            "properties": {
              "name":       { "type": "string", "description": "Build name, used on the command line." },
              "dockerfile": { "type": "string", "description": "Path relative to the resolved build root." },
              "context":    { "type": "string", "default": ".", "description": "Build context relative to the build root." },
              "image":      { "type": "string", "description": "Image repository. Supports {{...}}.", "examples": ["{{registry}}my-app"] },
              "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Tags appended to `image`. Each supports {{...}}.",
                "examples": [["{{tag}}"], ["{{version}}-{{git.shortSha}}{{git.dirtySuffix}}", "latest"]]
              },
              "target":    { "type": "string", "description": "Dockerfile stage to stop at." },
              "platform":  { "type": "string", "description": "Platform, when it differs from the profile's." },
              "buildArgs": { "type": "object", "additionalProperties": { "type": ["string", "number", "boolean"] } },
              "labels":    { "type": "object", "additionalProperties": { "type": ["string", "number", "boolean"] } },
              "secrets": {
                "type": "array",
                "items": {
                  "type": "object",
                  "additionalProperties": false,
                  "required": ["id"],
                  "properties": {
                    "id":  { "type": "string" },
                    "env": { "type": "string", "description": "Host environment variable to read the secret from." },
                    "src": { "type": "string", "description": "Host file to read the secret from." }
                  }
                }
              },
              "preflight": {
                "type": "array",
                "description": "argv arrays run from the build root before building. A failure aborts the build.",
                "items": { "type": "array", "items": { "type": "string" } }
              },
              "postPush": {
                "type": "array",
                "description": "argv arrays run from the build root after a successful push. A failure aborts and is reported as an error.",
                "items": { "type": "array", "items": { "type": "string" } }
              }
            }
          }
        },
        "services": {
          "type": "array",
          "description": "Containers this project runs. They start in array order, each gated on the previous one's readyCmd.",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["name", "image"],
            "properties": {
              "name":  { "type": "string", "description": "Service name. The container is <project>-<name>." },
              "image": { "type": "string", "description": "Full OCI reference, including the registry host.", "examples": ["docker.io/library/postgres:16-alpine"] },
              "cpus":  { "type": "integer", "minimum": 1, "description": "Sizes the container's VM, not a cgroup. Each container is its own VM." },
              "memory":{ "type": "string", "examples": ["1g"], "description": "Memory for the container's VM." },
              "ports": { "type": "array", "items": { "type": "string" }, "description": "host:container, same as Docker. Optional: every container also gets its own routable IP.", "examples": [["5433:5432"]] },
              "env":   { "type": "object", "additionalProperties": { "type": ["string", "number", "boolean"] } },
              "volumes": {
                "type": "array",
                "description":
                  "Named volumes. The real volume is <project>-<name>, created on demand. \
                   Apple container volumes are ext4 block devices, so a fresh one contains \
                   lost+found; point PGDATA and friends at a subdirectory.",
                "items": {
                  "type": "object",
                  "additionalProperties": false,
                  "required": ["name", "target"],
                  "properties": {
                    "name":   { "type": "string" },
                    "target": { "type": "string", "description": "Mount point inside the container." }
                  }
                }
              },
              "args": { "type": "array", "items": { "type": "string" }, "description": "Extra argv appended after the image reference." },
              "readyCmd": {
                "type": "array",
                "items": { "type": "string" },
                "description":
                  "Polled with `container exec` until it exits 0. Apple container has no \
                   healthcheck primitive, so readiness is implemented by ac.",
                "examples": [["pg_isready", "-U", "user"]]
              },
              "readyTimeout": { "type": "integer", "default": 90, "description": "Seconds before giving up. Start continues anyway, with a warning." }
            }
          }
        }
      },
      "$comment":
        "Interpolation placeholders usable in registries.server, builds.image, builds.tags, \
         builds.buildArgs, builds.labels and hook argv: {{profile}} {{account}} {{tag}} \
         {{region}} {{registry}} {{version}} {{git.sha}} {{git.shortSha}} {{git.branch}} \
         {{git.dirtySuffix}} {{timestamp}}."
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest built straight from the schema's own examples must parse
    /// against the typed manifest, which keeps the two from drifting.
    #[test]
    fn schema_example_parses() {
        let s = manifest_schema();
        assert_eq!(s["type"], "object");
        let doc = json!({
            "name": "demo",
            "description": "two tiny services",
            "region": "us-east-1",
            "builder": { "cpus": 4, "memory": "4g" },
            "profiles": { "local": { "platform": "linux/arm64", "push": false, "tag": "dev", "registry": "" } },
            "registries": [{ "server": "ghcr.io", "username": "me", "passwordCmd": ["gh", "auth", "token"] }],
            "builds": [{
                "name": "app", "dockerfile": "Dockerfile", "context": ".",
                "image": "{{registry}}app", "tags": ["{{tag}}"], "target": "runner",
                "buildArgs": { "A": "1" }, "labels": { "l": "v" },
                "secrets": [{ "id": "TOK", "env": "TOK" }],
                "preflight": [["true"]], "postPush": [["true"]]
            }],
            "services": [{
                "name": "redis", "image": "docker.io/library/redis:7-alpine",
                "cpus": 1, "memory": "256m", "ports": ["6379:6379"],
                "env": { "K": "v" },
                "volumes": [{ "name": "data", "target": "/data" }],
                "args": ["redis-server"],
                "readyCmd": ["redis-cli", "ping"], "readyTimeout": 30
            }]
        });
        let parsed: Result<crate::manifest::Manifest, _> = serde_json::from_value(doc);
        assert!(parsed.is_ok(), "{:?}", parsed.err());
    }

    #[test]
    fn unknown_field_is_rejected_with_alternatives() {
        let doc = json!({ "name": "x", "services": [{ "name": "a", "image": "i", "portz": [] }] });
        let err = serde_json::from_value::<crate::manifest::Manifest>(doc).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("portz"), "{msg}");
        assert!(msg.contains("ports"), "{msg}");
    }
}
