#!/usr/bin/env bash
#
# worktree_remove_cmd for the example provider.
#
# Routes on whether the worktree is a slot:
#
#   wt--pool-slot-<letter>  ->  detach its branch, keep the worktree and its
#                               pool index
#   anything else           ->  remove the worktree and free its index
#
# Both leave the git branch intact, so a card detached and later re-attached
# lands back on its own work.
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

if [[ $name =~ ^wt--pool-slot-[a-z]$ ]]; then
  if [[ $force == 1 ]]; then
    git -C "$path" checkout --detach --force >&2
  else
    # Refuse rather than discard: a slot is shared, so uncommitted work in it
    # does not belong to whoever is detaching.
    if [[ -n "$(git -C "$path" status --porcelain)" ]]; then
      echo "$name has uncommitted changes; commit them or detach with force" >&2
      exit 1
    fi
    git -C "$path" checkout --detach >&2
  fi
  exit 0
fi

args=(worktree remove "$path")
[[ $force == 1 ]] && args=(worktree remove --force "$path")

git -C "$main" "${args[@]}" >&2
registry_release "$name"
