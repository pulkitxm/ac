# Local containers: use ac

This machine runs containers with `ac`, a project runner for Apple
`container` (macOS native, no Docker daemon). Docker-style commands work:
`ac ps`, `ac image ls`, `ac <project> logs -f`, `ac <project> exec svc cmd`.

Orchestrate, do not micromanage:

- Discover first: `ac ls` lists projects, `ac <project> services` lists
  services, `ac guide` prints the full manual including a docker-to-ac
  command table. Prefer these over guessing.
- This project's stack: `ac <project> start`, gate on `ac <project> wait`,
  read state with `ac <project> ls --json`.
- Build images with `ac <project> build [name] [-P profile]`; inspect the
  plan first with `--dry-run --json`. Push without rebuilding via
  `ac <project> push -P <profile>`.
- Always pass `--json` when parsing output; stdout is then one JSON
  document and log lines go to stderr.
- Never run `container system stop` or stop containers you did not start;
  other work may depend on them. `ac ps --json` shows what is running and
  which project owns it.
- `ac <project> volumes rm` destroys data. Ask before running it.
- If something is missing from the stack, edit or add a manifest in
  `~/.config/ac/projects/<name>.json` (schema: `ac schema`); unknown fields
  are rejected by name.
