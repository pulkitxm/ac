#!/usr/bin/env bash
# build.sh - declarative image builds, pushes and rollout hooks.
#
# `ac` owns the container mechanics only. Anything organisation-specific
# (verifying an AWS account, rolling out k8s deployments) is expressed as a
# hook: an argv array that is executed and checked for exit status. That keeps
# this file free of any AWS or Kubernetes knowledge.
#
# Precedence for every setting, highest first:
#   1. CLI flag           (--platform, --builder-cpus, ...)
#   2. profile            (.profiles.<name>)
#   3. build entry        (.builds[])
#   4. project default    (.builder, .region)

# Overrides set by bin/ac from CLI flags; empty means "not given".
AC_F_PLATFORM=""; AC_F_PUSH=""; AC_F_NOCACHE=""
AC_F_BCPUS=""; AC_F_BMEM=""; AC_F_TARGET=""; AC_F_SEQUENTIAL=""; AC_F_ROOT=""

# Resolve the directory builds run from. Order, highest priority first:
#
#   1. --root <path>           explicit, wins always
#   2. $AC_ROOT                for scripts and agents
#   3. the git worktree containing $PWD, when it looks like this project
#   4. .root from the manifest
#   5. $PWD
#
# Step 3 is what makes git worktrees work: running `ac <proj> build` from
# inside a worktree builds THAT tree, not the path baked into the manifest,
# without needing a second manifest per worktree. "Looks like this project"
# means the tree actually contains the dockerfile the manifest references, so
# an unrelated repo never hijacks the build.
proj_root() {
  local proj="$1"
  local abs

  if [ -n "${AC_F_ROOT:-}" ]; then
    abs=$( cd "$AC_F_ROOT" 2>/dev/null && pwd ) || die "--root does not exist: $AC_F_ROOT"
    printf '%s' "$abs"; return 0
  fi
  if [ -n "${AC_ROOT:-}" ]; then
    abs=$( cd "$AC_ROOT" 2>/dev/null && pwd ) || die "AC_ROOT does not exist: $AC_ROOT"
    printf '%s' "$abs"; return 0
  fi

  local manifest_root top marker
  manifest_root=$(proj_json "$proj" | jq -r '.root // ""')

  top=$(git rev-parse --show-toplevel 2>/dev/null)
  if [ -n "$top" ]; then
    marker=$(proj_json "$proj" | jq -r '(.builds // [])[0].dockerfile // empty')
    if [ -n "$marker" ] && [ -e "$top/$marker" ]; then
      printf '%s' "$top"; return 0
    fi
    # No builds declared: any git tree matching the manifest root's basename.
    if [ -z "$marker" ] && [ -n "$manifest_root" ] \
       && [ "$(basename "$top")" = "$(basename "$manifest_root")" ]; then
      printf '%s' "$top"; return 0
    fi
  fi

  if [ -n "$manifest_root" ]; then
    abs=$( cd "$manifest_root" 2>/dev/null && pwd ) && { printf '%s' "$abs"; return 0; }
    warn "manifest root does not exist: $manifest_root"
  fi
  pwd
}

profile_get() {
  proj_json "$1" | jq -r --arg p "$2" --arg k "$3" '.profiles[$p][$k] // empty'
}

build_get() {
  proj_json "$1" | jq -r --arg b "$2" --arg k "$3" '(.builds[] | select(.name==$b))[$k] // empty'
}

build_names() { proj_json "$1" | jq -r '.builds[].name'; }

# ------------------------------------------------------------ interpolation ---

# Expand {{...}} placeholders. This is what keeps manifests generic: tagging
# rules live in the manifest as templates rather than in this script.
build_interpolate() {
  local proj="$1" profile="$2" str="$3"
  case "$str" in *'{{'*) ;; *) printf '%s' "$str"; return 0 ;; esac

  local root acct tag region sha short branch dirty version ts
  root=$(proj_root "$proj")
  acct=$(profile_get "$proj" "$profile" account)
  tag=$(profile_get "$proj" "$profile" tag)
  region=$(profile_get "$proj" "$profile" region)
  [ -n "$region" ] || region=$(proj_json "$proj" | jq -r '.region // "us-east-1"')

  if git -C "$root" rev-parse --git-dir >/dev/null 2>&1; then
    sha=$(git -C "$root" rev-parse HEAD 2>/dev/null)
    short=$(git -C "$root" rev-parse --short HEAD 2>/dev/null)
    branch=$(git -C "$root" rev-parse --abbrev-ref HEAD 2>/dev/null)
    # A dirty tree must never overwrite the image CI built for that commit.
    [ -n "$(git -C "$root" status --porcelain 2>/dev/null)" ] \
      && dirty="-local-$(date +%Y%m%d%H%M%S)"
  fi
  # {{registry}} is the host+slash prefix, empty for purely local profiles so
  # the same image template yields "app:tag" locally and
  # "<acct>.dkr.ecr.<region>.amazonaws.com/app:tag" when pushing.
  local reg
  reg=$(profile_get "$proj" "$profile" registry)
  reg=${reg//\{\{account\}\}/$acct}
  reg=${reg//\{\{region\}\}/$region}

  version=$(jq -r '.version // "0.0.0"' "$root/package.json" 2>/dev/null)
  ts=$(date +%Y%m%d%H%M%S)

  printf '%s' "$str" | sed \
    -e "s|{{profile}}|${profile}|g" \
    -e "s|{{account}}|${acct}|g" \
    -e "s|{{tag}}|${tag}|g" \
    -e "s|{{region}}|${region}|g" \
    -e "s|{{registry}}|${reg}|g" \
    -e "s|{{version}}|${version}|g" \
    -e "s|{{git.sha}}|${sha}|g" \
    -e "s|{{git.shortSha}}|${short}|g" \
    -e "s|{{git.branch}}|${branch}|g" \
    -e "s|{{git.dirtySuffix}}|${dirty}|g" \
    -e "s|{{timestamp}}|${ts}|g"
}

# ----------------------------------------------------------------- builder ---

# The buildkit builder is a long-lived container shared by every build, and it
# only reads its cpu/memory settings when it is CREATED. Passing -c/-m to a
# build while it is already running is silently ignored, so resizing means
# stopping it first.
builder_ensure() {
  local want_cpus="$1" want_mem="$2"
  [ -n "$want_cpus$want_mem" ] || return 0

  local line cur_cpus cur_mem
  line=$(container builder status 2>/dev/null | awk 'NR==2')
  cur_cpus=$(printf '%s' "$line" | awk '{print $(NF-1)}')
  cur_mem=$(printf '%s' "$line" | awk '{print $NF}')   # e.g. "MB" column split

  # Normalise the requested memory to MB for comparison (8g -> 8192).
  local want_mb=""
  case "$want_mem" in
    *[gG]) want_mb=$(( ${want_mem%[gG]} * 1024 )) ;;
    *[gG][bB]) want_mb=$(( ${want_mem%[gG][bB]} * 1024 )) ;;
    *[mM]|*[mM][bB]) want_mb=$(printf '%s' "$want_mem" | tr -dc '0-9') ;;
    *) want_mb="$want_mem" ;;
  esac
  cur_mem=$(container builder status 2>/dev/null | awk 'NR==2 {print $(NF-1)}')

  if [ -n "$line" ] && [ "$cur_cpus" = "$want_cpus" ] && [ "$cur_mem" = "$want_mb" ]; then
    return 0
  fi
  if [ -n "$line" ]; then
    dim "  resizing builder to ${want_cpus} cpus / ${want_mem} (requires a restart)"
    run_cmd container builder stop >/dev/null 2>&1
    sleep 2
  fi
}

# ------------------------------------------------------------------- hooks ---

# Run a list of argv arrays from the manifest. Returns non-zero on the first
# failure so callers can decide whether that is fatal.
run_hooks() {
  local proj="$1" profile="$2" build="$3" key="$4"
  local root n i cmd_json tmp a
  root=$(proj_root "$proj")
  n=$(proj_json "$proj" | jq --arg b "$build" --arg k "$key" \
        '[(.builds[] | select(.name==$b))[$k] // []] | .[0] | length')
  [ "$n" -gt 0 ] 2>/dev/null || return 0

  i=0
  while [ "$i" -lt "$n" ]; do
    tmp=$(mktemp)
    proj_json "$proj" | jq -r --arg b "$build" --arg k "$key" --argjson i "$i" \
      '(.builds[] | select(.name==$b))[$k][$i][]' > "$tmp"
    local argv=()
    while IFS= read -r a; do
      argv+=("$(build_interpolate "$proj" "$profile" "$a")")
    done < "$tmp"
    rm -f "$tmp"

    dim "  $key: ${argv[*]}"
    if ! ( cd "$root" && "${argv[@]}" ); then
      err "$key failed: ${argv[*]}"
      return 1
    fi
    i=$((i + 1))
  done
  return 0
}

# ------------------------------------------------------------------- build ---

build_one() {
  local proj="$1" profile="$2" name="$3"
  local root; root=$(proj_root "$proj")

  # Resolve every setting through the precedence chain.
  local platform push nocache target dockerfile context image
  platform="$AC_F_PLATFORM"
  [ -n "$platform" ] || platform=$(profile_get "$proj" "$profile" platform)
  [ -n "$platform" ] || platform=$(build_get "$proj" "$name" platform)
  [ -n "$platform" ] || platform="linux/arm64"

  push="$AC_F_PUSH"
  if [ -z "$push" ]; then
    push=$(profile_get "$proj" "$profile" push)
    [ -n "$push" ] || push="false"
  fi

  target="$AC_F_TARGET"
  [ -n "$target" ] || target=$(build_get "$proj" "$name" target)

  dockerfile=$(build_get "$proj" "$name" dockerfile)
  context=$(build_get "$proj" "$name" context); [ -n "$context" ] || context="."
  image=$(build_interpolate "$proj" "$profile" "$(build_get "$proj" "$name" image)")

  # Tags: interpolate each, prefix with the image repo.
  local tags=() t
  while IFS= read -r t; do
    [ -n "$t" ] || continue
    tags+=("${image}:$(build_interpolate "$proj" "$profile" "$t")")
  done < <(proj_json "$proj" | jq -r --arg b "$name" '(.builds[] | select(.name==$b)).tags[]?')
  [ ${#tags[@]} -gt 0 ] || die "build '$name' declares no tags"

  local args=(build --platform "$platform" -f "$dockerfile")
  [ -n "$target" ] && args+=(--target "$target")
  [ -n "$AC_F_NOCACHE" ] || [ -n "${NO_CACHE:-}" ] && args+=(--no-cache)

  local cpus mem
  cpus="$AC_F_BCPUS"; [ -n "$cpus" ] || cpus=$(proj_json "$proj" | jq -r '.builder.cpus // empty')
  mem="$AC_F_BMEM";   [ -n "$mem" ]  || mem=$(proj_json "$proj" | jq -r '.builder.memory // empty')
  [ -n "$cpus" ] && args+=(--cpus "$cpus")
  [ -n "$mem" ]  && args+=(--memory "$mem")

  local line
  while IFS= read -r line; do
    [ -n "$line" ] && args+=(--build-arg "$(build_interpolate "$proj" "$profile" "$line")")
  done < <(proj_json "$proj" | jq -r --arg b "$name" \
      '((.builds[] | select(.name==$b)).buildArgs // {}) | to_entries[] | "\(.key)=\(.value)"')

  while IFS= read -r line; do
    [ -n "$line" ] && args+=(--secret "$line")
  done < <(proj_json "$proj" | jq -r --arg b "$name" \
      '((.builds[] | select(.name==$b)).secrets // [])[] | "id=\(.id)" + (if .env then ",env=\(.env)" else "" end) + (if .src then ",src=\(.src)" else "" end)')

  while IFS= read -r line; do
    [ -n "$line" ] && args+=(--label "$line")
  done < <(proj_json "$proj" | jq -r --arg b "$name" \
      '((.builds[] | select(.name==$b)).labels // {}) | to_entries[] | "\(.key)=\(.value)"')

  for t in "${tags[@]}"; do args+=(-t "$t"); done
  args+=("$context")

  info "[$name] preflight"
  run_hooks "$proj" "$profile" "$name" preflight || return 1

  dim  "  [$name] root: $root"
  info "[$name] building $platform -> ${tags[0]}"
  ( cd "$root" && run_cmd container "${args[@]}" ) || { err "[$name] build failed"; return 1; }
  ok "[$name] built"

  if [ "$push" = "true" ]; then
    for t in "${tags[@]}"; do
      info "[$name] pushing $t"
      run_cmd container image push "$t" || { err "[$name] push failed: $t"; return 1; }
    done
    ok "[$name] pushed"
    run_hooks "$proj" "$profile" "$name" postPush || return 1
  else
    dim "  [$name] push disabled for profile '$profile'"
  fi
  return 0
}

project_build() {
  local proj="$1"; shift
  local profile="${AC_PROFILE:-local}"

  proj_json "$proj" | jq -e --arg p "$profile" '.profiles[$p]' >/dev/null 2>&1 \
    || die "unknown profile '$profile' (have: $(proj_json "$proj" | jq -r '.profiles | keys | join(", ")'))"

  local targets
  if [ $# -eq 0 ]; then
    targets=$(build_names "$proj")
  else
    local b all; all=$(build_names "$proj")
    targets=""
    for b in "$@"; do
      printf '%s\n' "$all" | grep -qxF -- "$b" \
        || die "no such build '$b' (have: $(echo $all))"
      targets="$targets$b
"
    done
  fi
  [ -n "$targets" ] || die "project '$proj' declares no builds"

  daemon_ensure

  local cpus mem
  cpus="$AC_F_BCPUS"; [ -n "$cpus" ] || cpus=$(proj_json "$proj" | jq -r '.builder.cpus // empty')
  mem="$AC_F_BMEM";   [ -n "$mem" ]  || mem=$(proj_json "$proj" | jq -r '.builder.memory // empty')
  builder_ensure "$cpus" "$mem"

  # Only authenticate when something is actually going to be pushed.
  local push="$AC_F_PUSH"
  [ -n "$push" ] || push=$(profile_get "$proj" "$profile" push)
  if [ "$push" = "true" ]; then
    local bimgs=""
    for b in $targets; do
      bimgs="$bimgs $(build_interpolate "$proj" "$profile" "$(build_get "$proj" "$b" image)")"
    done
    project_login "$proj" "$profile" $bimgs || true
  fi

  local n; n=$(printf '%s\n' "$targets" | grep -c . )
  local failed=0 b

  if [ "$n" -gt 1 ] && [ -z "$AC_F_SEQUENTIAL" ]; then
    info "building $n images in parallel (--sequential to disable)"
    local pids=""
    for b in $targets; do
      ( build_one "$proj" "$profile" "$b" ) &
      pids="$pids $!"
    done
    for p in $pids; do wait "$p" || failed=1; done
  else
    for b in $targets; do
      build_one "$proj" "$profile" "$b" || failed=1
    done
  fi

  [ "$failed" -eq 0 ] || die "one or more builds failed"
  ok "all builds finished"
}
