# shellcheck shell=bash
#
# Shared state for the example worktree provider: a fixed pool of worktrees
# outside the project root, each holding a registry index that gives it a port
# block. Sourced by the four scripts bork calls; not a provider command itself.

POOL_ROOT="${BORK_EXAMPLE_POOL_ROOT:-$HOME/.bork-example-pool}"
POOL_SIZE="${BORK_EXAMPLE_POOL_SIZE:-4}"
REGISTRY="$POOL_ROOT/registry"

# Registry format: one line per index, position is the index, a blank line is a
# vacant slot. Positional rather than keyed so an index — and therefore a port
# block — is stable for as long as a worktree holds it.
registry_init() {
  mkdir -p "$POOL_ROOT"
  [[ -f $REGISTRY ]] && return
  local i
  for ((i = 0; i < POOL_SIZE; i++)); do
    echo "" >>"$REGISTRY"
  done
}

# The main checkout. Worktrees share its `.git`, so its parent is the repo —
# `--show-toplevel` would return whichever worktree we are standing in.
main_repo() {
  local common_dir
  common_dir="$(git rev-parse --git-common-dir 2>/dev/null)" || {
    echo "not inside a git repository; cwd is $PWD" >&2
    return 1
  }
  (cd "$common_dir/.." && pwd -P)
}

registry_index_of() {
  registry_init
  awk -v want="$1" 'NR > 0 { if ($0 == want) { print NR - 1; exit } }' "$REGISTRY"
}

# Claim the first vacant index, printing it. Fails when the pool is full, which
# is the ceiling `worktree_pool_size` mirrors on bork's side.
registry_claim() {
  registry_init
  local name="$1" index
  index="$(registry_index_of "$name")"
  if [[ -n $index ]]; then
    echo "$index"
    return
  fi

  index="$(awk '$0 == "" { print NR - 1; exit }' "$REGISTRY")"
  if [[ -z $index ]]; then
    echo "the pool of $POOL_SIZE worktrees is full" >&2
    return 1
  fi

  local tmp
  tmp="$(mktemp)"
  awk -v line="$((index + 1))" -v name="$name" \
    '{ print (NR == line) ? name : $0 }' "$REGISTRY" >"$tmp"
  mv "$tmp" "$REGISTRY"
  echo "$index"
}

registry_release() {
  registry_init
  local tmp
  tmp="$(mktemp)"
  awk -v name="$1" '{ print ($0 == name) ? "" : $0 }' "$REGISTRY" >"$tmp"
  mv "$tmp" "$REGISTRY"
}

# The port block an index owns. A real provider allocates databases and
# services the same way; this only has to be visible on a card.
port_for() {
  echo "$((8000 + $1 * 10))"
}

# What a card shows for this worktree. Slots are named so a reader recognises
# the shared one; everything else is identified by its index.
label_for() {
  local name="$1" index="$2"
  if [[ $name =~ ^wt--pool-slot-([a-z])$ ]]; then
    echo "slot ${BASH_REMATCH[1]} · :$(port_for "$index")"
  else
    echo "wt $index · :$(port_for "$index")"
  fi
}

# Escape backslashes and quotes, the only characters a path or branch name can
# realistically contain.
json_string() {
  local value=${1//\\/\\\\}
  value=${value//\"/\\\"}
  printf '"%s"' "$value"
}

# A branch name as a directory name.
sanitise() {
  local value="${1//\//-}"
  echo "${value//[^A-Za-z0-9._-]/-}"
}
