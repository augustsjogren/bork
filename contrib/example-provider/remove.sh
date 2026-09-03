#!/usr/bin/env bash
#
# worktree_remove_cmd for the example provider.
#
# Removes the worktree and frees its pool index. The git branch is left
# intact, so a card detached and later re-attached lands back on its own
# work.
set -euo pipefail

source "$(dirname "$0")/pool.sh"

[[ -n ${BORK_WORKTREE_NAME:-} ]] || { echo "BORK_WORKTREE_NAME is not set" >&2; exit 1; }

main="$(main_repo)"
name="$BORK_WORKTREE_NAME"
path="${BORK_WORKTREE_PATH:-$POOL_ROOT/$name}"
force="${BORK_FORCE:-0}"

if [[ ! -d $path ]]; then
  # Already gone; free the index so the pool does not leak a slot.
  registry_release "$name"
  exit 0
fi

args=(worktree remove "$path")
[[ $force == 1 ]] && args=(worktree remove --force "$path")

git -C "$main" "${args[@]}" >&2
registry_release "$name"
