# Local containers: use ac

This machine runs containers with `ac`, which drives Apple `container` (macOS
native, no Docker daemon). It replaces both docker and docker compose.

Two forms, pick by whether a manifest declares the thing:

- **`ac <verb> <container|image>` is the docker CLI.** No manifest needed.
  `ac build -t app:dev .`, `ac run -d -p 3000:3000 app:dev`, `ac ps`,
  `ac logs -f <container>`, `ac exec <container> cmd`, `ac stop <container>`.
  Use this for anything ad hoc. Do NOT write a manifest just to run one
  container.
- **`ac <project> <verb> [services]` is docker compose.** Needs a manifest.
  Use it when the thing is a declared stack, because only this form does
  service ordering, `readyCmd` gating, named volumes and per-profile registry
  login.

Orchestrate, do not micromanage:

- Discover first: `ac ps` shows every container on the daemon whoever started
  it, `ac ls` lists projects, `ac <project> services` lists services,
  `ac guide` prints the full manual including a docker-to-ac table. Prefer
  these over guessing.
- This project's stack: `ac <project> start`, gate on `ac <project> wait`,
  read state with `ac <project> ls --json`.
- Build a project's declared images with `ac <project> build [name]
  [-P profile]` (profiles, interpolation, rollout hooks); inspect the plan
  first with `--dry-run --json`. For a bare Dockerfile use `ac build -t ref .`.
- Always pass `--json` when parsing output; stdout is then one JSON document
  and log lines go to stderr. Put it before the verb: `ac --json ps`.
- Never run `container system stop` or stop containers you did not start;
  other work may depend on them. `ac ps --json` shows what is running and,
  where a manifest declares it, which project owns it.
- `--all` is not a convenience: `ac stop -a`, `ac rm -a` and `ac kill -a` hit
  every container on the daemon, including other people's. Name your targets.
- `ac volume rm` and `ac <project> volumes rm` destroy data. Ask first.
- Add a manifest in `~/.config/ac/projects/<name>.json` (schema: `ac schema`)
  when you need a multi-service stack with readiness gating; unknown fields
  are rejected by name.
- A manifest may declare custom `scripts` (name to shell string). `ac
  <project> <name> [args...]` runs the string with the args appended; the
  script owns its own subcommands. Discover them with `ac <project> scripts`.
